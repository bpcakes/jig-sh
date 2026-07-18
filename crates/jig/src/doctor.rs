use std::collections::{HashSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
#[cfg(unix)]
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};
use wait_timeout::ChildExt;

use crate::command::{VaultCommand, VaultStatusRequest};
use crate::context::{RepoContext, find_repo_root_from_or_env};
#[cfg(test)]
use crate::tool_defs::tool;

const COMMAND: &str = "doctor";
const SQLX_DRIVER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const CODEX_SUPPORT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OWNED_PROCESS_TREE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(any(target_os = "linux", target_os = "macos"))]
const OWNED_PROCESS_TREE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const OWNED_PROCESS_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
const OWNED_PROCESS_OUTPUT_LIMIT: usize = 16 * 1024;
const PROXY_LIST_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(unix)]
const DOCTOR_SIGNAL_QUIESCENCE_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn run() -> Result<Value> {
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    let root_result = find_repo_root_from_or_env(&cwd);
    let mut checks = Vec::new();

    let root = match root_result {
        Ok(root) => root,
        Err(error) => {
            checks.push(check(
                "repo",
                "Jig repo",
                true,
                false,
                "missing",
                error.to_string(),
            ).with_fix("Run `scripts/jig adopt . --repo-name <name> --sqlx-enabled false` from the repository root, or run `scripts/jig init <path> --preset harness-only --repo-name <name> --sqlx-enabled false --no-input --no-vault` to create a new repo."));
            return Ok(output(None, checks));
        }
    };

    checks.push(
        check(
            "repo",
            "Jig repo",
            true,
            true,
            "found",
            root.display().to_string(),
        )
        .with_data(json!({ "root": root.display().to_string() })),
    );

    let config_probe = RepoContext::validate_config_file(&root);
    let ctx_result = RepoContext::load_from_root(root.clone());
    let (config_ok, repo_name, config_jig_version) = match &config_probe {
        Ok(probe) => (
            true,
            Some(probe.repo_name.clone()),
            Some(probe.jig_version.clone()),
        ),
        Err(_) => (false, None, None),
    };
    checks.push(config_check(&root, &config_probe));
    checks.push(runtime_check(&root, config_jig_version.as_deref()));

    match &ctx_result {
        Ok(ctx) => {
            checks.push(contract_check(ctx));
            let context_checks = doctor_context_checks(ctx);
            checks.push(context_checks.required_tools);
            checks.push(context_checks.agent);
            checks.push(context_checks.proxy);
        }
        Err(error) => {
            let context_error = if config_ok {
                format!("Repo context failed to load: {error}")
            } else {
                format!("Skipped until .jig.toml is valid: {error}")
            };
            checks.push(
                check(
                    "contract",
                    "Contract",
                    true,
                    false,
                    "blocked",
                    context_error.clone(),
                )
                .with_fix("Run `scripts/jig check contract --no-receipt` after fixing the reported repo configuration issue."),
            );
            checks.push(
                check(
                    "required_tools",
                    "Required tools",
                    true,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig check contract --no-receipt` first."),
            );
            checks.push(
                check(
                    "agent_skills",
                    "Agent skills",
                    false,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig doctor` after fixing the contract issue."),
            );
            checks.push(
                check(
                    "proxy",
                    "Proxy",
                    false,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig doctor` after fixing the contract issue."),
            );
        }
    }

    checks.push(vault_check(
        ctx_result.as_ref().map_err(|error| error.to_string()),
    ));

    Ok(output(
        Some(json!({
            "root": root.display().to_string(),
            "name": repo_name,
            "jig_version": config_jig_version,
        })),
        checks,
    ))
}

pub(crate) fn format_summary(value: &Value) -> String {
    let ready = value["ok"].as_bool().unwrap_or(false);
    let mut lines = vec![format!(
        "Jig doctor: {}",
        if ready { "ready" } else { "needs attention" }
    )];
    if let Some(root) = value["repo"]["root"].as_str() {
        lines.push(format!("Repo: {root}"));
    }
    lines.push("Checks:".into());
    for check in value["checks"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let label = check["label"].as_str().unwrap_or("<unknown>");
        let status = check["status"].as_str().unwrap_or("unknown");
        let required = check["required"].as_bool().unwrap_or(false);
        let required_label = if required { "required" } else { "optional" };
        let ok = check["ok"].as_bool().unwrap_or(false);
        let marker = if ok {
            "ok"
        } else if required {
            "needs setup"
        } else {
            "optional setup"
        };
        lines.push(format!(
            "  - {label}: {marker} ({status}, {required_label})"
        ));
        if required && (!ok || status == "present_unverified") {
            if let Some(detail) = check["detail"].as_str() {
                if !detail.trim().is_empty() {
                    lines.push(format!("    Detail: {detail}"));
                }
            }
        }
    }

    match summary_step(value, "next_required_step", true) {
        Some(step) => lines.push(format!("Next required step: {step}")),
        None => lines.push("Next required step: none".into()),
    }
    match summary_step(value, "optional_setup", false) {
        Some(step) => lines.push(format!("Optional setup: {}", optional_setup_label(step))),
        None => lines.push("Optional setup: none".into()),
    }
    lines.join("\n")
}

fn output(repo: Option<Value>, checks: Vec<DoctorCheck>) -> Value {
    let required_ok = checks.iter().all(|check| !check.required || check.ok);
    let next_required_issue = checks.iter().find(|check| check.required && !check.ok);
    let next_optional_issue = required_ok
        .then(|| checks.iter().find(|check| !check.required && !check.ok))
        .flatten();
    let next_issue = next_required_issue.or(next_optional_issue);
    let next_step = next_issue.and_then(|check| check.fix.clone());
    let next_required_step = next_required_issue.and_then(|check| check.fix.clone());
    let optional_setup = next_optional_issue.and_then(|check| check.fix.clone());
    let next_issue = next_issue.map(|check| {
        json!({
            "id": &check.id,
            "label": &check.label,
            "required": check.required,
            "status": &check.status,
            "fix": &check.fix,
        })
    });
    let checks = serde_json::to_value(checks).expect("doctor checks serialize");

    json!({
        "ok": required_ok,
        "command": COMMAND,
        "repo": repo,
        "checks": checks,
        "next_issue": next_issue,
        "next_required_step": next_required_step,
        "optional_setup": optional_setup,
        "next_step": next_step,
    })
}

fn summary_step<'a>(value: &'a Value, key: &str, required: bool) -> Option<&'a str> {
    value[key]
        .as_str()
        .or_else(|| {
            (required || value["ok"].as_bool().unwrap_or(false))
                .then(|| step_from_checks(value, required))
                .flatten()
        })
        .or_else(|| legacy_next_step(value, required))
}

fn step_from_checks(value: &Value, required: bool) -> Option<&str> {
    value["checks"]
        .as_array()?
        .iter()
        .find(|check| {
            !check["ok"].as_bool().unwrap_or(false)
                && check["required"].as_bool().unwrap_or(false) == required
        })
        .and_then(|check| check["fix"].as_str())
}

fn legacy_next_step(value: &Value, required: bool) -> Option<&str> {
    let ready = value["ok"].as_bool().unwrap_or(false);
    if ready == required {
        return None;
    }
    value["next_step"].as_str()
}

fn optional_setup_label(step: &str) -> &str {
    let Some(rest) = step.strip_prefix("Run `") else {
        return step;
    };
    let Some((command, _)) = rest.split_once('`') else {
        return step;
    };
    if command.starts_with("scripts/jig ") {
        command
    } else {
        step
    }
}

fn config_check(root: &Path, result: &Result<crate::context::RepoConfigProbe>) -> DoctorCheck {
    match result {
        Ok(probe) => check(
            "config",
            ".jig.toml",
            true,
            true,
            "valid",
            format!(
                "repo_name={}, jig_version={}",
                probe.repo_name, probe.jig_version
            ),
        )
        .with_data(json!({
            "path": root.join(".jig.toml").display().to_string(),
            "repo_name": probe.repo_name,
            "jig_version": probe.jig_version,
        })),
        Err(error) => check(
            "config",
            ".jig.toml",
            true,
            false,
            "invalid",
            error.to_string(),
        )
        .with_fix("Fix `.jig.toml`, then run `scripts/jig doctor`.")
        .with_data(json!({ "path": root.join(".jig.toml").display().to_string() })),
    }
}

fn runtime_check(root: &Path, config_jig_version: Option<&str>) -> DoctorCheck {
    let current_version = env!("CARGO_PKG_VERSION");
    let script_path = root.join("scripts/jig");
    let launcher = launcher_version(&script_path);
    let script_version = launcher.version;
    let script_ok = script_path.exists();
    let launcher_ok = launcher.read_error.is_none()
        && script_version
            .as_deref()
            .is_none_or(|version| version == current_version);
    let config_ok = config_jig_version.is_none_or(|version| version == current_version);
    let version_ok = launcher_ok && config_ok;
    let ok = script_ok && version_ok;
    let detail = match (
        &script_version,
        launcher.read_error.as_deref(),
        config_jig_version,
    ) {
        (_, Some(error), Some(config_version)) => {
            format!(
                "running {current_version}, scripts/jig is unreadable ({error}), .jig.toml pins {config_version}"
            )
        }
        (_, Some(error), None) => {
            format!("running {current_version}, but scripts/jig is unreadable ({error})")
        }
        (Some(script_version), None, Some(config_version)) => {
            format!(
                "running {current_version}, launcher pins {script_version}, .jig.toml pins {config_version}"
            )
        }
        (Some(script_version), None, None) => {
            format!("running {current_version}, launcher pins {script_version}")
        }
        (None, None, Some(config_version)) if script_ok => format!(
            "running {current_version}, scripts/jig has no readable JIG_VERSION pin, .jig.toml pins {config_version}"
        ),
        (None, None, None) if script_ok => {
            format!("running {current_version}, but scripts/jig has no readable JIG_VERSION pin")
        }
        (None, None, _) => format!("running {current_version}, but scripts/jig is missing"),
    };
    let status = if ok && script_version.is_none() {
        "unverified launcher"
    } else if ok {
        "installed"
    } else {
        "mismatch"
    };
    let fix = if !script_ok || !version_ok {
        Some("Run `scripts/jig update`, then rerun `scripts/jig doctor`.")
    } else {
        None
    };

    check("runtime", "Pinned runtime", true, ok, status, detail)
        .with_optional_fix(fix)
        .with_data(json!({
                "current_version": current_version,
                "launcher_path": script_path.display().to_string(),
                "launcher_version": script_version,
                "launcher_error": launcher.read_error,
                "config_jig_version": config_jig_version,
        }))
}

struct LauncherVersion {
    version: Option<String>,
    read_error: Option<String>,
}

fn launcher_version(path: &Path) -> LauncherVersion {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LauncherVersion {
                version: None,
                read_error: None,
            };
        }
        Err(error) => {
            return LauncherVersion {
                version: None,
                read_error: Some(error.to_string()),
            };
        }
    };
    for line in text.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("JIG_VERSION=") else {
            continue;
        };
        return LauncherVersion {
            version: Some(unquote_shell_value(value.trim()).to_string()),
            read_error: None,
        };
    }
    LauncherVersion {
        version: None,
        read_error: None,
    }
}

fn unquote_shell_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn contract_check(ctx: &RepoContext) -> DoctorCheck {
    match crate::policy::contract_check(ctx) {
        Ok(output) if output.exit_status == 0 => check(
            "contract",
            "Contract",
            true,
            true,
            "valid",
            output.stdout.trim().to_string(),
        )
        .with_data(json!({ "exit_status": output.exit_status })),
        Ok(output) => check(
            "contract",
            "Contract",
            true,
            false,
            "invalid",
            output.stderr.trim().to_string(),
        )
        .with_fix("Run `scripts/jig check contract --no-receipt` for the full contract report.")
        .with_data(json!({
                "exit_status": output.exit_status,
                "stdout": output.stdout,
                "stderr": output.stderr,
        })),
        Err(error) => check(
            "contract",
            "Contract",
            true,
            false,
            "error",
            error.to_string(),
        )
        .with_fix("Run `scripts/jig check contract --no-receipt` for the full contract report."),
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SqlxDriver {
    Postgres,
    Sqlite,
}

impl SqlxDriver {
    fn from_database_url(database_url: &str) -> Option<Self> {
        let scheme = database_url.trim().split_once(':')?.0;
        if scheme.eq_ignore_ascii_case("sqlite") {
            Some(Self::Sqlite)
        } else if scheme.eq_ignore_ascii_case("postgres")
            || scheme.eq_ignore_ascii_case("postgresql")
        {
            Some(Self::Postgres)
        } else {
            None
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::Sqlite => "SQLite",
        }
    }

    fn probe_url(self) -> &'static str {
        match self {
            // The generic URL parser accepts this URL, then the PostgreSQL
            // driver rejects the invalid sslmode before opening a socket.
            Self::Postgres => "postgres://127.0.0.1/jig_doctor_probe?sslmode=jig-doctor-invalid",
            // Any migration bookkeeping is confined to this process-local DB.
            Self::Sqlite => "sqlite::memory:",
        }
    }

    fn install_command(self) -> &'static str {
        match self {
            Self::Postgres => {
                "cargo install sqlx-cli --force --no-default-features --features rustls,postgres"
            }
            Self::Sqlite => {
                "cargo install sqlx-cli --force --no-default-features --features sqlite"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SqlxDriverSource {
    CommandFlag,
    CommandAssignment,
    Environment,
    Dotenv,
    DotenvExample,
}

impl SqlxDriverSource {
    fn key(self) -> &'static str {
        match self {
            Self::CommandFlag => "command_flag",
            Self::CommandAssignment => "command_assignment",
            Self::Environment => "environment",
            Self::Dotenv => ".env",
            Self::DotenvExample => ".env.example",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::CommandFlag => "a --database-url command option",
            Self::CommandAssignment => "a command-local DATABASE_URL assignment",
            Self::Environment => "the DATABASE_URL environment variable",
            Self::Dotenv => "DATABASE_URL in .env",
            Self::DotenvExample => "DATABASE_URL in .env.example",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SqlxDriverRequirement {
    driver: SqlxDriver,
    source: SqlxDriverSource,
}

impl SqlxDriverRequirement {
    fn description(&self) -> String {
        format!(
            "{} driver required by {}",
            self.driver.label(),
            self.source.description()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SqlxDriverResolution {
    Known(SqlxDriverRequirement),
    Absent,
    Indeterminate(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SqlxDriverProbe {
    Compatible,
    Incompatible,
    Indeterminate(String),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SqlxProbeStyle {
    CargoSubcommand,
    Direct,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellEnvironmentIssue {
    BashEnv,
    PosixEnv,
    CdPath,
    ShellOptions,
    BashOptions,
    TracePrompt,
    TraceFileDescriptor,
    ImportedFunction,
}

impl ShellEnvironmentIssue {
    fn description(self) -> &'static str {
        match self {
            Self::BashEnv => "BASH_ENV can execute a startup file",
            Self::PosixEnv => "ENV can execute a startup file in POSIX shell mode",
            Self::CdPath => "CDPATH can change shell directory resolution",
            Self::ShellOptions => "SHELLOPTS can enable inherited shell options",
            Self::BashOptions => "BASHOPTS can enable inherited Bash options",
            Self::TracePrompt => "PS4 can execute expansions or alter trace output",
            Self::TraceFileDescriptor => "BASH_XTRACEFD can redirect shell trace output",
            Self::ImportedFunction => "an exported Bash function can change command dispatch",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DoctorEnvironment {
    search_path: Option<OsString>,
    path_extensions: Option<OsString>,
    database_url: Option<OsString>,
    cargo_alias_sqlx: Option<OsString>,
    cargo_home: Option<OsString>,
    home: Option<OsString>,
    probe_environment: Vec<(OsString, OsString)>,
    shell_environment_issue: Option<ShellEnvironmentIssue>,
}

#[derive(Clone, Copy)]
struct DoctorProcessControl<'a> {
    cancellation: Option<&'a dyn Fn() -> bool>,
    unavailable_reason: Option<&'static str>,
}

impl DoctorProcessControl<'_> {
    const fn allowed_without_signal_session() -> Self {
        Self {
            cancellation: None,
            unavailable_reason: None,
        }
    }

    #[cfg(unix)]
    const fn unavailable(reason: &'static str) -> Self {
        Self {
            cancellation: None,
            unavailable_reason: Some(reason),
        }
    }
}

impl DoctorEnvironment {
    fn capture() -> Self {
        let bash_env = env::var_os("BASH_ENV");
        let posix_env = env::var_os("ENV");
        let cdpath = env::var_os("CDPATH");
        let shell_options = env::var_os("SHELLOPTS");
        let bash_options = env::var_os("BASHOPTS");
        let trace_prompt = env::var_os("PS4");
        let trace_file_descriptor = env::var_os("BASH_XTRACEFD");
        let shell_environment_issue = inherited_shell_environment_issue(
            [
                (ShellEnvironmentIssue::BashEnv, bash_env.as_deref()),
                (ShellEnvironmentIssue::PosixEnv, posix_env.as_deref()),
                (ShellEnvironmentIssue::CdPath, cdpath.as_deref()),
                (
                    ShellEnvironmentIssue::ShellOptions,
                    shell_options.as_deref(),
                ),
                (ShellEnvironmentIssue::BashOptions, bash_options.as_deref()),
                (ShellEnvironmentIssue::TracePrompt, trace_prompt.as_deref()),
                (
                    ShellEnvironmentIssue::TraceFileDescriptor,
                    trace_file_descriptor.as_deref(),
                ),
            ],
            env::vars_os(),
        );
        Self {
            search_path: env::var_os("PATH"),
            path_extensions: env::var_os("PATHEXT"),
            database_url: env::var_os("DATABASE_URL"),
            cargo_alias_sqlx: env::var_os("CARGO_ALIAS_SQLX"),
            cargo_home: env::var_os("CARGO_HOME"),
            home: env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")),
            probe_environment: ["SystemRoot", "WINDIR", "COMSPEC"]
                .into_iter()
                .filter_map(|key| env::var_os(key).map(|value| (key.into(), value)))
                .collect(),
            shell_environment_issue,
        }
    }
}

fn inherited_shell_environment_issue(
    controls: [(ShellEnvironmentIssue, Option<&OsStr>); 7],
    variables: impl IntoIterator<Item = (OsString, OsString)>,
) -> Option<ShellEnvironmentIssue> {
    for (issue, value) in controls {
        if value.is_some_and(|value| !value.is_empty()) {
            return Some(issue);
        }
    }
    variables
        .into_iter()
        .any(|(key, _)| crate::shell::is_exported_bash_function_environment_key(&key))
        .then_some(ShellEnvironmentIssue::ImportedFunction)
}

#[derive(Debug)]
struct DoctorContextChecks {
    required_tools: DoctorCheck,
    agent: DoctorCheck,
    proxy: DoctorCheck,
}

fn doctor_context_checks(ctx: &RepoContext) -> DoctorContextChecks {
    let environment = DoctorEnvironment::capture();
    #[cfg(unix)]
    {
        if !doctor_process_session_required(ctx) {
            return doctor_context_checks_with_process_control(
                ctx,
                &environment,
                DoctorProcessControl::allowed_without_signal_session(),
            );
        }
        let signal_session = match DoctorSignalSession::start() {
            Ok(session) => session,
            Err(_) => {
                return doctor_context_checks_with_process_control(
                    ctx,
                    &environment,
                    DoctorProcessControl::unavailable(
                        "the process-wide doctor signal session is unavailable",
                    ),
                );
            }
        };
        let cancelled = || signal_session.cancelled();
        let mut checks = doctor_context_checks_with_process_control(
            ctx,
            &environment,
            DoctorProcessControl {
                cancellation: Some(&cancelled),
                unavailable_reason: None,
            },
        );
        if finish_doctor_signal_session(signal_session).is_err() {
            mark_doctor_signal_retirement_failure(ctx, &mut checks);
        }
        checks
    }
    #[cfg(not(unix))]
    {
        doctor_context_checks_with_process_control(
            ctx,
            &environment,
            DoctorProcessControl::allowed_without_signal_session(),
        )
    }
}

fn doctor_context_checks_with_process_control(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> DoctorContextChecks {
    let required_tools = required_tools_check_with_environment_and_process_control(
        ctx,
        environment,
        process_control,
    );
    let agent = agent_check(ctx, process_control);
    let proxy = proxy_check_with_process_control(ctx, process_control);
    DoctorContextChecks {
        required_tools,
        agent,
        proxy,
    }
}

#[cfg(unix)]
fn doctor_process_session_required(ctx: &RepoContext) -> bool {
    let sqlx_probe_required = ctx.sqlx_enabled()
        && ctx
            .required_commands()
            .iter()
            .any(|command| command == "sqlx_check_command");
    sqlx_probe_required || !ctx.codex_marketplaces().is_empty() || proxy_configured(ctx)
}

#[cfg(unix)]
fn mark_doctor_signal_retirement_failure(ctx: &RepoContext, checks: &mut DoctorContextChecks) {
    if ctx.sqlx_enabled()
        && ctx
            .required_commands()
            .iter()
            .any(|command| command == "sqlx_check_command")
    {
        if checks.required_tools.status == "present" {
            checks.required_tools.status = "present_unverified".to_string();
        }
        checks.required_tools.detail.push_str(
            "; SQLx capability verification is incomplete because the process-wide doctor signal session could not retire safely",
        );
    }
    if !ctx.codex_marketplaces().is_empty() {
        checks.agent.ok = false;
        checks.agent.status = "error".to_string();
        checks.agent.detail.push_str(
            "; Codex marketplace verification is incomplete because the process-wide doctor signal session could not retire safely",
        );
        checks.agent.fix = Some("Run `scripts/jig agent doctor` for agent tooling details.".into());
    }
    if proxy_configured(ctx) {
        checks.proxy.ok = false;
        checks.proxy.status = "error".to_string();
        checks.proxy.detail.push_str(
            "; proxy diagnostics are incomplete because the process-wide doctor signal session could not retire safely",
        );
        checks.proxy.fix = Some("Run `scripts/jig proxy list` for proxy diagnostics.".into());
    }
}

#[cfg(test)]
fn required_tools_check_with_environment(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
) -> DoctorCheck {
    required_tools_check_with_environment_and_process_control(
        ctx,
        environment,
        DoctorProcessControl::allowed_without_signal_session(),
    )
}

fn required_tools_check_with_environment_and_process_control(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let mut tools = Vec::new();
    let mut missing = Vec::new();
    let mut incompatible = Vec::new();
    let mut indeterminate = Vec::new();
    let mut remediation_driver = None;
    let mut executable_reference_count = 0;
    for command_key in ctx.required_commands() {
        let command = match ctx.command_for_key(command_key) {
            Ok(command) => command,
            Err(error) => {
                missing.push(format!("{command_key}: {error}"));
                tools.push(json!({
                    "command_key": command_key,
                    "command": null,
                    "program": null,
                    "present": false,
                    "detail": error.to_string(),
                }));
                continue;
            }
        };
        let sqlx_driver = if command_key == "sqlx_check_command" && ctx.sqlx_enabled() {
            configured_sqlx_driver(ctx.root(), command, environment.database_url.as_deref())
        } else {
            SqlxDriverResolution::Absent
        };
        let program_discovery = required_command_programs(ctx.root(), command);
        let programs = &program_discovery.programs;
        let inherited_shell_issue = environment.shell_environment_issue;
        let cargo_sqlx_dispatch_issue = command_uses_cargo_sqlx(command)
            .then(|| cargo_sqlx_dispatch_issue(ctx.root(), command, environment));
        executable_reference_count += programs.len();
        let mut sqlx_probes = HashSet::new();
        let mut sqlx_resolution_recorded = false;
        let mut probed_programs = if programs.is_empty()
            && program_discovery.ambiguity.is_none()
            && inherited_shell_issue.is_none()
        {
            vec![json!({
                "program": null,
                "present": true,
                "detail": "No external executable required.",
            })]
        } else {
            programs
                .iter()
                .map(|program_spec| {
                    let program = &program_spec.program;
                    let presence = match &program_spec.path_lookup {
                        ProgramPathLookup::Explicit | ProgramPathLookup::Captured => {
                            match resolve_program(
                                ctx.root(),
                                program,
                                environment.search_path.as_deref(),
                                environment.path_extensions.as_deref(),
                            ) {
                                Some(resolution) => ProgramPresence::Present(resolution),
                                None => ProgramPresence::Missing,
                            }
                        }
                        ProgramPathLookup::CommandLocal(search_path) => {
                            match resolve_program(
                                ctx.root(),
                                program,
                                Some(search_path.as_os_str()),
                                environment.path_extensions.as_deref(),
                            ) {
                                Some(resolution) => ProgramPresence::Present(resolution),
                                None => ProgramPresence::Missing,
                            }
                        }
                        ProgramPathLookup::CapturedAfterCwdChange
                            if search_path_is_cwd_independent(
                                environment.search_path.as_deref(),
                            ) =>
                        {
                            match resolve_program(
                                ctx.root(),
                                program,
                                environment.search_path.as_deref(),
                                environment.path_extensions.as_deref(),
                            ) {
                                Some(resolution) => ProgramPresence::Present(resolution),
                                None => ProgramPresence::Missing,
                            }
                        }
                        ProgramPathLookup::CapturedAfterCwdChange
                        | ProgramPathLookup::Unverifiable => ProgramPresence::Unverified,
                    };
                    let resolved = match &presence {
                        ProgramPresence::Present(resolution) => Some(resolution),
                        ProgramPresence::Missing | ProgramPresence::Unverified => None,
                    };
                    let (reported_program, redact_program) =
                        reported_program(command_key, program);
                    let (present, detail) = match &presence {
                        ProgramPresence::Present(_) if redact_program => (
                            Some(true),
                            "A redacted command executable is present.".to_string(),
                        ),
                        ProgramPresence::Missing if redact_program => (
                            Some(false),
                            "A redacted command executable was not found.".to_string(),
                        ),
                        ProgramPresence::Present(resolution) => {
                            let (present, detail) = program_presence(
                                ctx.root(),
                                &reported_program,
                                Some(resolution.path.as_path()),
                            );
                            (Some(present), detail)
                        }
                        ProgramPresence::Missing => {
                            let (present, detail) =
                                program_presence(ctx.root(), &reported_program, None);
                            (Some(present), detail)
                        }
                        ProgramPresence::Unverified => (
                            None,
                            if redact_program {
                                "A redacted command executable could not be resolved safely because the configured Bash command may change the executable lookup context before this invocation."
                                    .to_string()
                            } else {
                                format!(
                                    "{reported_program} could not be resolved safely because the configured Bash command may change the executable lookup context before this invocation"
                                )
                            },
                        ),
                    };
                    if present == Some(false) {
                        missing.push(format!("{command_key}: {reported_program}"));
                    }
                    let mut report = json!({
                        "program": reported_program,
                        "present": present,
                        "detail": detail,
                    });

                    let probe_style = sqlx_probe_style(program);
                    if matches!(presence, ProgramPresence::Unverified) {
                        let detail = format!(
                            "{command_key}: {reported_program} executable lookup could not be verified because the configured Bash command may change the executable lookup context before this invocation"
                        );
                        if command_key == "sqlx_check_command"
                            && (program_spec.cargo_sqlx_dispatch || probe_style.is_some())
                        {
                            sqlx_resolution_recorded = true;
                            let probe_detail = format!(
                                "{detail}; SQLx CLI capability probing was skipped; run `scripts/jig check sqlx`"
                            );
                            indeterminate.push(probe_detail.clone());
                            report["driver_probe"] = match sqlx_driver {
                                SqlxDriverResolution::Known(requirement) => json!({
                                    "driver": requirement.driver.key(),
                                    "source": requirement.source.key(),
                                    "status": "unverified",
                                    "compatible": null,
                                    "detail": probe_detail,
                                }),
                                _ => json!({
                                    "driver": null,
                                    "source": null,
                                    "status": "unverified",
                                    "compatible": null,
                                    "detail": probe_detail,
                                }),
                            };
                        } else {
                            indeterminate.push(detail);
                        }
                        return report;
                    }
                    let sqlx_inherited_shell_issue = (command_key == "sqlx_check_command"
                        && present == Some(true)
                        && (program_spec.cargo_sqlx_dispatch || probe_style.is_some()))
                    .then_some(environment.shell_environment_issue)
                    .flatten();
                    if let Some(issue) = sqlx_inherited_shell_issue {
                        let detail = format!(
                            "{command_key}: SQLx CLI capability probing was skipped because inherited shell state ({}) can alter the configured Bash command; run `scripts/jig check sqlx`",
                            issue.description(),
                        );
                        sqlx_resolution_recorded = true;
                        report["driver_probe"] = json!({
                            "driver": null,
                            "source": null,
                            "status": "unverified",
                            "compatible": null,
                            "detail": detail,
                        });
                        return report;
                    }

                    if program_spec.cargo_sqlx_dispatch {
                        let reason = cargo_sqlx_dispatch_issue.unwrap_or(
                            "Cargo subcommand dispatch cannot be verified without executing cargo",
                        );
                        let detail = format!(
                            "{command_key}: cargo sqlx dispatch could not be verified ({reason}); run `scripts/jig check sqlx`"
                        );
                        if !sqlx_resolution_recorded {
                            indeterminate.push(detail.clone());
                        }
                        sqlx_resolution_recorded = true;
                        report["driver_probe"] = match sqlx_driver {
                            SqlxDriverResolution::Known(requirement) => json!({
                                "driver": requirement.driver.key(),
                                "source": requirement.source.key(),
                                "status": "unverified",
                                "compatible": null,
                                "detail": detail,
                            }),
                            _ => json!({
                                "driver": null,
                                "source": null,
                                "status": "unverified",
                                "compatible": null,
                                "detail": detail,
                            }),
                        };
                        return report;
                    }

                    if command_key == "sqlx_check_command" && present == Some(true) {
                        match (probe_style, sqlx_driver) {
                            (Some(probe_style), SqlxDriverResolution::Known(requirement)) => {
                                let trusted_executable = resolved.as_ref().and_then(|resolution| {
                                    trusted_sqlx_probe_executable(
                                        ctx.root(),
                                        program,
                                        resolution,
                                    )
                                });
                                let Some(executable) = trusted_executable else {
                                    if !sqlx_resolution_recorded {
                                        sqlx_resolution_recorded = true;
                                        let detail = format!(
                                            "{command_key}: SQLx CLI capability probing was skipped because the configured executable is not a trusted bare PATH command; run `scripts/jig check sqlx`"
                                        );
                                        indeterminate.push(detail.clone());
                                        report["driver_probe"] = json!({
                                            "driver": requirement.driver.key(),
                                            "source": requirement.source.key(),
                                            "status": "unverified",
                                            "compatible": null,
                                            "detail": detail,
                                        });
                                    }
                                    return report;
                                };
                                if let Some(reason) = process_control.unavailable_reason {
                                    let detail = format!(
                                        "{command_key}: could not verify the {} driver in the SQLx CLI ({reason}); run `scripts/jig check sqlx`",
                                        requirement.driver.label()
                                    );
                                    indeterminate.push(detail.clone());
                                    sqlx_resolution_recorded = true;
                                    report["driver_probe"] = json!({
                                        "driver": requirement.driver.key(),
                                        "source": requirement.source.key(),
                                        "status": "unverified",
                                        "compatible": null,
                                        "detail": detail,
                                    });
                                    return report;
                                }
                                let probe_key = (executable.clone(), probe_style);
                                if !sqlx_probes.insert(probe_key) {
                                    return report;
                                }
                                let probe = probe_sqlx_driver(
                                    &executable,
                                    probe_style,
                                    requirement.driver,
                                    ctx.root(),
                                    environment,
                                    process_control.cancellation,
                                );
                                let (status, compatible, probe_detail) = match &probe {
                                    SqlxDriverProbe::Compatible => (
                                        "compatible",
                                        Some(true),
                                        format!(
                                            "SQLx CLI supports the {}",
                                            requirement.description()
                                        ),
                                    ),
                                    SqlxDriverProbe::Incompatible => {
                                        remediation_driver = Some(requirement.driver);
                                        let detail = format!(
                                            "{command_key}: SQLx CLI is installed but lacks the {}",
                                            requirement.description()
                                        );
                                        incompatible.push(detail.clone());
                                        ("missing_driver", Some(false), detail)
                                    }
                                    SqlxDriverProbe::Indeterminate(reason) => {
                                        let detail = format!(
                                            "{command_key}: could not verify the {} driver in the SQLx CLI ({reason}); run `scripts/jig check sqlx`",
                                            requirement.driver.label()
                                        );
                                        indeterminate.push(detail.clone());
                                        ("unverified", None, detail)
                                    }
                                };
                                report["driver_probe"] = json!({
                                    "driver": requirement.driver.key(),
                                    "source": requirement.source.key(),
                                    "status": status,
                                    "compatible": compatible,
                                    "detail": probe_detail,
                                });
                            }
                            (Some(_), SqlxDriverResolution::Indeterminate(reason)) => {
                                if !sqlx_resolution_recorded {
                                    sqlx_resolution_recorded = true;
                                    let detail = format!(
                                        "{command_key}: could not determine the required SQLx driver ({reason}); run `scripts/jig check sqlx`"
                                    );
                                    indeterminate.push(detail.clone());
                                    report["driver_probe"] = json!({
                                        "driver": null,
                                        "source": null,
                                        "status": "unverified",
                                        "compatible": null,
                                        "detail": detail,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }

                    report
                })
                .collect()
        };
        if let Some(ambiguity) = program_discovery.ambiguity {
            let detail = format!(
                "{command_key}: executable discovery is incomplete because {}; the configured command must be run to verify its tools",
                ambiguity.description(),
            );
            indeterminate.push(detail.clone());
            probed_programs.push(json!({
                "program": null,
                "present": null,
                "detail": detail,
            }));
        }
        if let Some(issue) = inherited_shell_issue {
            let detail = format!(
                "{command_key}: command execution is unverified because inherited shell state ({}) can alter the configured Bash command",
                issue.description(),
            );
            indeterminate.push(detail.clone());
            probed_programs.push(json!({
                "program": null,
                "present": null,
                "detail": detail,
            }));
        }
        if command_key == "sqlx_check_command" && !sqlx_resolution_recorded {
            if let SqlxDriverResolution::Indeterminate(reason) = sqlx_driver {
                let detail = format!(
                    "{command_key}: could not determine the required SQLx driver ({reason}); run `scripts/jig check sqlx`"
                );
                indeterminate.push(detail);
            }
        }
        let any_missing = probed_programs
            .iter()
            .any(|program| program["present"].as_bool() == Some(false));
        let any_unverified = probed_programs
            .iter()
            .any(|program| program["present"].is_null());
        let command_present = if any_missing {
            Some(false)
        } else if any_unverified {
            None
        } else {
            Some(true)
        };
        let reported_command = format!("<redacted: {command_key}>");
        tools.push(json!({
            "command_key": command_key,
            "command": reported_command,
            "command_redacted": true,
            "programs": probed_programs,
            "present": command_present,
        }));
    }

    let ok = missing.is_empty() && incompatible.is_empty();
    let status = match (missing.is_empty(), incompatible.is_empty()) {
        (true, true) if indeterminate.is_empty() => "present",
        (true, true) => "present_unverified",
        (false, true) => "missing",
        (true, false) => "incompatible",
        (false, false) => "unavailable",
    };
    let checked_detail = format!(
        "{} required command(s) checked; {} external executable reference(s) inspected",
        tools.len(),
        executable_reference_count
    );
    let detail = if ok && indeterminate.is_empty() {
        checked_detail
    } else if ok {
        format!(
            "{checked_detail}; Present but unverified: {}",
            indeterminate.join(", ")
        )
    } else {
        let mut details = Vec::new();
        if !missing.is_empty() {
            details.push(format!(
                "Missing command executable(s): {}",
                missing.join(", ")
            ));
        }
        if !incompatible.is_empty() {
            details.push(format!(
                "Incompatible command executable(s): {}",
                incompatible.join(", ")
            ));
        }
        if !indeterminate.is_empty() {
            details.push(format!(
                "Unverified command executable(s): {}",
                indeterminate.join(", ")
            ));
        }
        details.join("; ")
    };
    let fix = required_tools_fix(
        !missing.is_empty(),
        !incompatible.is_empty(),
        remediation_driver,
    );

    check("required_tools", "Required tools", true, ok, status, detail)
        .with_optional_fix(fix.as_deref())
        .with_data(json!({ "tools": tools }))
}

fn required_tools_fix(
    has_missing: bool,
    has_incompatible: bool,
    driver: Option<SqlxDriver>,
) -> Option<String> {
    let mut steps = Vec::new();
    if has_missing {
        steps
            .push("Install the missing executable or restore the missing repo script.".to_string());
    }
    if let Some(driver) = driver {
        let action = if has_incompatible {
            "Reinstall"
        } else {
            "Install"
        };
        steps.push(format!(
            "{action} SQLx CLI with {} support (for example, `{}`).",
            driver.label(),
            driver.install_command()
        ));
    }
    if steps.is_empty() {
        return None;
    }
    steps.push("Then run `scripts/jig doctor`.".to_string());
    Some(steps.join(" "))
}

fn configured_sqlx_driver(
    root: &Path,
    command: &str,
    ambient_database_url: Option<&OsStr>,
) -> SqlxDriverResolution {
    let command =
        active_optional_cargo_branch(root, command).unwrap_or_else(|| command.to_string());
    let parsed = parse_shell_commands(&command);
    let mut requirements = Vec::new();
    let mut saw_sqlx = false;
    let mut mutates_database_url = false;
    let mut effective_cwd = root.to_path_buf();
    let mut cwd_is_ambiguous = false;
    let mut guarded_cd = None;
    let mut wrapper_semantics_are_ambiguous = false;

    for (command_index, words) in parsed.commands.iter().enumerate() {
        let incoming_separator = command_index
            .checked_sub(1)
            .and_then(|index| parsed.separators.get(index))
            .copied();
        let outgoing_separator = parsed.separators.get(command_index).copied();
        if let Some(path) = guarded_cd.take() {
            if literal_exit_guard(words)
                && matches!(
                    outgoing_separator,
                    Some(ShellSeparator::Sequence | ShellSeparator::And)
                )
            {
                effective_cwd = path;
                continue;
            }
            cwd_is_ambiguous = true;
        }
        wrapper_semantics_are_ambiguous |= shell_command_has_ambiguous_wrapper(words);
        if shell_command_changes_directory(words) {
            let standalone = matches!(incoming_separator, None | Some(ShellSeparator::Sequence));
            let resolved = standalone
                .then(|| resolve_literal_cd(root, &effective_cwd, words))
                .flatten();
            match (resolved, outgoing_separator) {
                (Some(path), Some(ShellSeparator::And)) => effective_cwd = path,
                (Some(path), Some(ShellSeparator::Or)) => guarded_cd = Some(path),
                _ => cwd_is_ambiguous = true,
            }
            continue;
        }
        let Some(program_index) = command_program_index(words) else {
            mutates_database_url |= shell_command_mutates_database_url(words);
            continue;
        };
        let Some(invocation) = sqlx_invocation(words, program_index) else {
            mutates_database_url |= shell_command_mutates_database_url(words);
            continue;
        };

        saw_sqlx = true;
        let explicit = sqlx_driver_from_flag(words, invocation.args_index, ambient_database_url);
        let prefix_database_url = command_database_url_scope(words, program_index);
        requirements.push(match (explicit, prefix_database_url) {
            (Some(resolution), _) => resolution,
            (None, CommandDatabaseUrlScope::Assigned(value)) => sqlx_driver_from_command_value(
                &value,
                SqlxDriverSource::CommandAssignment,
                ambient_database_url,
            ),
            (None, CommandDatabaseUrlScope::Removed) => SqlxDriverResolution::Indeterminate(
                "the SQLx command removes DATABASE_URL from its environment",
            ),
            (None, CommandDatabaseUrlScope::Ambiguous) => SqlxDriverResolution::Indeterminate(
                "the SQLx command changes DATABASE_URL through an ambiguous wrapper",
            ),
            (None, CommandDatabaseUrlScope::Inherited)
                if cwd_is_ambiguous || invocation.has_ambiguous_cwd_option =>
            {
                SqlxDriverResolution::Indeterminate(
                    "the SQLx command changes directory in a way doctor cannot resolve safely",
                )
            }
            (None, CommandDatabaseUrlScope::Inherited) => configured_sqlx_driver_fallback(
                root,
                &effective_cwd,
                ambient_database_url,
                invocation.no_dotenv,
            ),
        });
    }

    if parsed.ambiguous || mutates_database_url || wrapper_semantics_are_ambiguous {
        return SqlxDriverResolution::Indeterminate(
            "the SQLx command uses shell syntax whose DATABASE_URL scope is ambiguous",
        );
    }
    if !saw_sqlx {
        return SqlxDriverResolution::Indeterminate(
            "doctor could not identify a supported SQLx CLI invocation",
        );
    }

    let mut known: Option<SqlxDriverRequirement> = None;
    let mut saw_absent = false;
    for resolution in requirements {
        match resolution {
            SqlxDriverResolution::Known(requirement) => {
                if let Some(previous) = known {
                    if previous.driver != requirement.driver {
                        return SqlxDriverResolution::Indeterminate(
                            "the SQLx command invokes different database drivers",
                        );
                    }
                } else {
                    known = Some(requirement);
                }
            }
            SqlxDriverResolution::Absent => saw_absent = true,
            SqlxDriverResolution::Indeterminate(reason) => {
                return SqlxDriverResolution::Indeterminate(reason);
            }
        }
    }
    if known.is_some() && saw_absent {
        return SqlxDriverResolution::Indeterminate(
            "some SQLx invocations have no discoverable database URL",
        );
    }
    known
        .map(SqlxDriverResolution::Known)
        .unwrap_or(SqlxDriverResolution::Indeterminate(
            "no database URL is discoverable for the SQLx command",
        ))
}

fn configured_sqlx_driver_fallback(
    root: &Path,
    cwd: &Path,
    ambient_database_url: Option<&OsStr>,
    no_dotenv: bool,
) -> SqlxDriverResolution {
    if let Some(database_url) = ambient_database_url {
        let Some(database_url) = database_url.to_str() else {
            return SqlxDriverResolution::Indeterminate(
                "DATABASE_URL in the environment is not valid UTF-8",
            );
        };
        return sqlx_driver_from_literal(database_url, SqlxDriverSource::Environment);
    }

    if no_dotenv {
        return SqlxDriverResolution::Absent;
    }

    match nearest_database_url_from_dotenv(cwd, root, ".env") {
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Literal(database_url)))) => {
            return sqlx_driver_from_literal(&database_url, SqlxDriverSource::Dotenv);
        }
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Substitution))) => {
            return SqlxDriverResolution::Indeterminate(
                "DATABASE_URL in dotenv uses variable substitution",
            );
        }
        // dotenvy stops at the nearest file even when it does not define the
        // requested variable. Do not fall through to a parent or example file.
        Ok(DotenvLookup::Present(None)) => return SqlxDriverResolution::Absent,
        Ok(DotenvLookup::Missing) => {}
        Err(()) => {
            return SqlxDriverResolution::Indeterminate("a dotenv file could not be parsed safely");
        }
    }

    match dotenv_exists_above_repo(root, ".env") {
        Ok(true) => {
            return SqlxDriverResolution::Indeterminate(
                "SQLx may load a .env file above the Jig repository",
            );
        }
        Ok(false) => {}
        Err(()) => {
            return SqlxDriverResolution::Indeterminate(
                "the dotenv search path could not be inspected safely",
            );
        }
    }

    // `.env.example` is a Jig-specific intended-driver hint, not a file SQLx
    // loads. It is only authoritative enough to inspect when no `.env` exists
    // anywhere in the real dotenv search chain.
    match nearest_database_url_from_dotenv(cwd, root, ".env.example") {
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Literal(database_url)))) => {
            sqlx_driver_from_literal(&database_url, SqlxDriverSource::DotenvExample)
        }
        Ok(DotenvLookup::Present(Some(DotenvDatabaseUrl::Substitution))) => {
            SqlxDriverResolution::Indeterminate("DATABASE_URL in dotenv uses variable substitution")
        }
        Ok(DotenvLookup::Missing | DotenvLookup::Present(None)) => SqlxDriverResolution::Absent,
        Err(()) => SqlxDriverResolution::Indeterminate("a dotenv file could not be parsed safely"),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum DotenvLookup {
    Missing,
    Present(Option<DotenvDatabaseUrl>),
}

#[derive(Debug, Eq, PartialEq)]
enum DotenvDatabaseUrl {
    Literal(String),
    Substitution,
}

fn nearest_database_url_from_dotenv(
    cwd: &Path,
    root: &Path,
    name: &str,
) -> std::result::Result<DotenvLookup, ()> {
    for directory in repo_ancestors(cwd, root) {
        let path = directory.join(name);
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                return database_url_from_dotenv(&path).map(DotenvLookup::Present);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
    }
    Ok(DotenvLookup::Missing)
}

fn repo_ancestors(cwd: &Path, root: &Path) -> Vec<PathBuf> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let cwd = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut directories = Vec::new();
    for directory in cwd.ancestors() {
        if !directory.starts_with(&root) {
            break;
        }
        directories.push(directory.to_path_buf());
        if directory == root {
            break;
        }
    }
    directories
}

fn dotenv_exists_above_repo(root: &Path, name: &str) -> std::result::Result<bool, ()> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(parent) = root.parent() else {
        return Ok(false);
    };
    for directory in parent.ancestors() {
        match fs::metadata(directory.join(name)) {
            Ok(metadata) if metadata.is_file() => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
    }
    Ok(false)
}

fn database_url_from_dotenv(path: &Path) -> std::result::Result<Option<DotenvDatabaseUrl>, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    let bytes = bytes
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .unwrap_or(bytes.as_slice());
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    if first_raw_database_url_uses_substitution(text) == Some(true) {
        return Ok(Some(DotenvDatabaseUrl::Substitution));
    }
    let variables = dotenvy::from_read_iter(bytes);
    for variable in variables {
        let (key, value) = variable.map_err(|_| ())?;
        if dotenv_database_url_key(&key) {
            return Ok(Some(DotenvDatabaseUrl::Literal(value)));
        }
    }
    Ok(None)
}

fn first_raw_database_url_uses_substitution(text: &str) -> Option<bool> {
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    for (index, ch) in text.char_indices() {
        if comment {
            if matches!(ch, '\n' | '\r') {
                if let Some(result) = raw_database_url_line_uses_substitution(&text[start..index]) {
                    return Some(result);
                }
                start = index + ch.len_utf8();
                comment = false;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' if quote.is_none() => comment = true,
            '\n' | '\r' if quote.is_none() => {
                if let Some(result) = raw_database_url_line_uses_substitution(&text[start..index]) {
                    return Some(result);
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    raw_database_url_line_uses_substitution(&text[start..])
}

fn raw_database_url_line_uses_substitution(line: &str) -> Option<bool> {
    let line = line.trim_start();
    let line = line
        .strip_prefix("export")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .map(str::trim_start)
        .unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    if !dotenv_database_url_key(key.trim()) {
        return None;
    }
    Some(dotenv_value_uses_substitution(value))
}

fn dotenv_value_uses_substitution(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' if quote.is_none() => return false,
            '$' if quote != Some('\'') => {
                if chars
                    .peek()
                    .is_some_and(|next| *next == '{' || *next == '_' || next.is_ascii_alphabetic())
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn dotenv_database_url_key(key: &str) -> bool {
    #[cfg(windows)]
    {
        key.eq_ignore_ascii_case("DATABASE_URL")
    }
    #[cfg(not(windows))]
    {
        key == "DATABASE_URL"
    }
}

#[cfg(unix)]
static DOCTOR_SIGNAL: AtomicI32 = AtomicI32::new(0);
#[cfg(unix)]
static DOCTOR_SIGNAL_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static DOCTOR_SIGNAL_GENERATION: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static DOCTOR_ACTIVE_GENERATION: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static DOCTOR_NEXT_GENERATION: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static DOCTOR_SIGNAL_HANDLERS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
#[cfg(unix)]
static DOCTOR_SIGNAL_SESSION_POISONED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static DOCTOR_SIGNAL_SESSION: Mutex<()> = Mutex::new(());

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DoctorSignals {
    first: Option<libc::c_int>,
    mask: usize,
}

#[cfg(unix)]
impl DoctorSignals {
    fn ordered(self) -> Vec<libc::c_int> {
        let mut signals = Vec::with_capacity(3);
        if let Some(first) = self.first {
            signals.push(first);
        }
        for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
            if Some(signal) != self.first && self.mask & doctor_signal_bit(signal) != 0 {
                signals.push(signal);
            }
        }
        signals
    }

    fn first(self) -> Option<libc::c_int> {
        self.first.or_else(|| {
            [libc::SIGINT, libc::SIGHUP, libc::SIGTERM]
                .into_iter()
                .find(|signal| self.mask & doctor_signal_bit(*signal) != 0)
        })
    }
}

#[cfg(unix)]
const fn doctor_signal_bit(signal: libc::c_int) -> usize {
    match signal {
        libc::SIGINT => 1 << 0,
        libc::SIGHUP => 1 << 1,
        libc::SIGTERM => 1 << 2,
        _ => 0,
    }
}

#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_PAUSE_HANDLER: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_HANDLER_PAUSED: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_RELEASE_HANDLER: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(test, unix))]
static SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(test, unix))]
extern "C" fn record_sqlx_probe_test_redelivery(signal: libc::c_int) {
    let index = SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
    if index < 3 {
        let code = match signal {
            libc::SIGINT => 1,
            libc::SIGHUP => 2,
            libc::SIGTERM => 3,
            _ => 0,
        };
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.fetch_or(code << (index * 2), Ordering::SeqCst);
    }
}

#[cfg(unix)]
extern "C" fn record_doctor_signal(signal: libc::c_int) {
    #[cfg(test)]
    if SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.load(Ordering::SeqCst) {
        SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.store(true, Ordering::SeqCst);
        while !SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
    }

    // POSIX does not make disposition selection and the first user-space
    // handler instruction atomic across threads. Claim the generation that is
    // active when this callback actually enters: a delayed callback therefore
    // joins a later active session, while an idle callback fails closed below.
    DOCTOR_SIGNAL_HANDLERS_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let generation = DOCTOR_ACTIVE_GENERATION.load(Ordering::SeqCst);

    #[cfg(test)]
    if SQLX_PROBE_TEST_PAUSE_HANDLER.load(Ordering::SeqCst) {
        SQLX_PROBE_TEST_HANDLER_PAUSED.store(true, Ordering::SeqCst);
        while !SQLX_PROBE_TEST_RELEASE_HANDLER.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
    }

    if generation != 0 && DOCTOR_SIGNAL_GENERATION.load(Ordering::SeqCst) == generation {
        let _ = DOCTOR_SIGNAL.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
        DOCTOR_SIGNAL_MASK.fetch_or(doctor_signal_bit(signal), Ordering::SeqCst);
    }

    #[cfg(test)]
    if SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD.load(Ordering::SeqCst) {
        SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.store(true, Ordering::SeqCst);
        while !SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
    }

    DOCTOR_SIGNAL_HANDLERS_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);

    if generation == 0 || DOCTOR_SIGNAL_SESSION_POISONED.load(Ordering::SeqCst) {
        // A failed disposition restoration may leave this handler installed
        // after its session, and an unsafe retirement can no longer hand a
        // signal back through the restored disposition. Never swallow either
        // termination request.
        // SAFETY: `_exit` is async-signal-safe and this handler owns no
        // resources when there is no active probe generation.
        unsafe { libc::_exit(128 + signal) }
    }
}

#[cfg(unix)]
struct DoctorSignalSession {
    _guard: MutexGuard<'static, ()>,
    generation: usize,
    previous_actions: Vec<(libc::c_int, libc::sigaction)>,
    retired: bool,
}

#[cfg(unix)]
#[derive(Default)]
struct DoctorSignalRestoration {
    error: Option<std::io::Error>,
    handlers_may_remain: bool,
}

#[cfg(unix)]
impl DoctorSignalSession {
    fn start() -> std::io::Result<Self> {
        let guard = DOCTOR_SIGNAL_SESSION
            .lock()
            .map_err(|_| std::io::Error::other("the signal-session mutex is poisoned"))?;
        if DOCTOR_SIGNAL_SESSION_POISONED.load(Ordering::SeqCst) {
            return Err(std::io::Error::other(
                "a prior signal session could not retire safely",
            ));
        }
        let previous_generation = DOCTOR_NEXT_GENERATION
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |generation| {
                generation.checked_add(1)
            })
            .map_err(|_| std::io::Error::other("signal-session generations are exhausted"))?;
        let generation = previous_generation + 1;
        DOCTOR_SIGNAL.store(0, Ordering::SeqCst);
        DOCTOR_SIGNAL_MASK.store(0, Ordering::SeqCst);
        DOCTOR_SIGNAL_GENERATION.store(generation, Ordering::SeqCst);
        DOCTOR_ACTIVE_GENERATION.store(generation, Ordering::SeqCst);

        let mut session = Self {
            _guard: guard,
            generation,
            previous_actions: Vec::new(),
            retired: false,
        };
        for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
            // SAFETY: zero is a valid starting representation for sigaction;
            // the mask is then initialized with sigemptyset before use.
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = record_doctor_signal as *const () as usize;
            // SAFETY: `action.sa_mask` is writable storage owned by this call.
            if unsafe { libc::sigemptyset(&mut action.sa_mask) } == -1 {
                let error = std::io::Error::last_os_error();
                session.retire_after_failed_start();
                return Err(error);
            }
            action.sa_flags = 0;

            // SAFETY: `action` is initialized, `previous` points to writable
            // storage, and the signal is one of the supported termination
            // signals. The prior action is retained for scoped restoration.
            let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
            if unsafe { libc::sigaction(signal, &action, &mut previous) } == -1 {
                let error = std::io::Error::last_os_error();
                session.retire_after_failed_start();
                return Err(error);
            }
            session.previous_actions.push((signal, previous));
        }
        Ok(session)
    }

    fn cancelled(&self) -> bool {
        DOCTOR_SIGNAL_GENERATION.load(Ordering::SeqCst) == self.generation
            && DOCTOR_SIGNAL.load(Ordering::SeqCst) != 0
    }

    fn finish(mut self) -> std::io::Result<()> {
        let (signals, restored) = self.retire();
        complete_doctor_signal_retirement(signals, restored)
    }

    fn retire_after_failed_start(&mut self) {
        let (signals, restored) = self.retire();
        let _ = complete_doctor_signal_retirement(signals, restored);
    }

    fn retire(&mut self) -> (DoctorSignals, std::io::Result<()>) {
        if self.retired {
            return (DoctorSignals::default(), Ok(()));
        }
        let mut restoration = self.restore_handlers();
        if DOCTOR_ACTIVE_GENERATION.compare_exchange(
            self.generation,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) != Ok(self.generation)
        {
            restoration.error.get_or_insert_with(|| {
                std::io::Error::other("the active signal-session generation changed unexpectedly")
            });
            restoration.handlers_may_remain = true;
        }
        let quiesced = wait_for_doctor_signal_quiescence(DOCTOR_SIGNAL_QUIESCENCE_TIMEOUT);
        if !quiesced {
            restoration.error.get_or_insert_with(|| {
                std::io::Error::other("signal handlers did not become quiescent")
            });
        }

        let recorded_generation_retired = quiesced
            && DOCTOR_SIGNAL_GENERATION.compare_exchange(
                self.generation,
                0,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) == Ok(self.generation);
        if !recorded_generation_retired {
            restoration.error.get_or_insert_with(|| {
                std::io::Error::other("the recorded signal generation changed unexpectedly")
            });
        }
        let unsafe_retirement =
            !quiesced || !recorded_generation_retired || restoration.handlers_may_remain;
        if unsafe_retirement {
            // Publish the fail-closed claim before taking the recorded signal
            // snapshot. A handler that already passed the poison observation
            // recorded before this snapshot; a handler that has not passed it
            // will observe poison and terminate the process itself.
            DOCTOR_SIGNAL_SESSION_POISONED.store(true, Ordering::SeqCst);
        }
        let signals = take_doctor_signals();
        self.retired = true;
        (signals, restoration.error.map_or(Ok(()), Err))
    }

    fn restore_handlers(&mut self) -> DoctorSignalRestoration {
        let mut restoration = DoctorSignalRestoration::default();
        for (signal, action) in self.previous_actions.iter().rev() {
            // SAFETY: each action was returned by sigaction for this exact
            // signal when the scoped session started.
            if unsafe { libc::sigaction(*signal, action, std::ptr::null_mut()) } == -1 {
                let restore_error = std::io::Error::last_os_error();
                restoration.error.get_or_insert(restore_error);
                if install_default_doctor_signal_handler(*signal).is_err() {
                    restoration.handlers_may_remain = true;
                }
            }
        }
        restoration
    }
}

#[cfg(unix)]
impl Drop for DoctorSignalSession {
    fn drop(&mut self) {
        if self.retired {
            return;
        }
        let (signals, restored) = self.retire();
        let _ = complete_doctor_signal_retirement(signals, restored);
    }
}

#[cfg(unix)]
fn take_doctor_signals() -> DoctorSignals {
    let first = match DOCTOR_SIGNAL.swap(0, Ordering::SeqCst) {
        0 => None,
        signal => Some(signal),
    };
    DoctorSignals {
        first,
        mask: DOCTOR_SIGNAL_MASK.swap(0, Ordering::SeqCst),
    }
}

#[cfg(unix)]
fn install_default_doctor_signal_handler(signal: libc::c_int) -> std::io::Result<()> {
    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = libc::SIG_DFL;
    action.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this call.
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `action` is fully initialized and the signal is one installed
    // by this scoped session.
    if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn wait_for_doctor_signal_quiescence(timeout: Duration) -> bool {
    let deadline = Instant::now().checked_add(timeout);
    while DOCTOR_SIGNAL_HANDLERS_IN_FLIGHT.load(Ordering::SeqCst) != 0 {
        let Some(remaining) =
            deadline.and_then(|deadline| deadline.checked_duration_since(Instant::now()))
        else {
            #[cfg(test)]
            if SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT.load(Ordering::SeqCst) {
                SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.store(true, Ordering::SeqCst);
                while !SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.load(Ordering::SeqCst) {
                    std::hint::spin_loop();
                }
            }
            return false;
        };
        std::thread::sleep(remaining.min(Duration::from_millis(1)));
    }
    true
}

#[cfg(unix)]
fn redeliver_doctor_signal(signal: libc::c_int) {
    // The scoped handlers have been restored and the probe process tree is
    // already reaped. Raising now preserves the caller's original signal
    // semantics, including default termination and custom handlers.
    // SAFETY: `signal` was supplied by the OS to this process's handler.
    let _ = unsafe { libc::raise(signal) };
}

#[cfg(unix)]
fn redeliver_doctor_signals(signals: DoctorSignals) {
    for signal in signals.ordered() {
        redeliver_doctor_signal(signal);
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DoctorSignalFinishAction {
    Continue,
    Redeliver(DoctorSignals),
    Exit(libc::c_int),
}

#[cfg(unix)]
fn doctor_signal_finish_action(signals: DoctorSignals, restored: bool) -> DoctorSignalFinishAction {
    match (signals.first(), restored) {
        (Some(_), true) => DoctorSignalFinishAction::Redeliver(signals),
        (Some(signal), false) => DoctorSignalFinishAction::Exit(128 + signal),
        (None, _) => DoctorSignalFinishAction::Continue,
    }
}

#[cfg(unix)]
fn complete_doctor_signal_retirement(
    termination_signals: DoctorSignals,
    restored: std::io::Result<()>,
) -> std::io::Result<()> {
    match doctor_signal_finish_action(termination_signals, restored.is_ok()) {
        DoctorSignalFinishAction::Continue => {}
        DoctorSignalFinishAction::Redeliver(signals) => {
            redeliver_doctor_signals(signals);
        }
        DoctorSignalFinishAction::Exit(status) => {
            // Restoration failure means raising could invoke the Jig recorder
            // again and swallow termination. The probe tree is already gone.
            // SAFETY: this is the fail-closed process termination path.
            unsafe { libc::_exit(status) }
        }
    }
    restored
}

#[cfg(unix)]
fn finish_doctor_signal_session(signal_session: DoctorSignalSession) -> std::io::Result<()> {
    // `finish` retains the process-wide mutex guard until handlers are restored
    // and every recorded signal has reached its prior disposition.
    signal_session.finish()
}

pub(crate) fn standalone_codex_support_probe(
    codex_bin: &str,
    timeout: Duration,
) -> crate::runtime::CodexSupportProbeResult {
    #[cfg(all(unix, not(test)))]
    {
        standalone_codex_support_probe_with_signal_session(codex_bin, timeout)
    }
    #[cfg(any(not(unix), test))]
    {
        crate::runtime::probe_codex_marketplace_support(codex_bin, timeout, || false)
    }
}

#[cfg(unix)]
fn standalone_codex_support_probe_with_signal_session(
    codex_bin: &str,
    timeout: Duration,
) -> crate::runtime::CodexSupportProbeResult {
    let signal_session = DoctorSignalSession::start().map_err(|_| {
        "Codex marketplace support probe was not started because the process-wide signal session is unavailable".to_string()
    })?;
    let cancelled = || signal_session.cancelled();
    let probe = crate::runtime::probe_codex_marketplace_support(codex_bin, timeout, &cancelled);
    finish_doctor_signal_session(signal_session).map_err(|_| {
        "Codex marketplace support probe supervision could not retire safely".to_string()
    })?;
    probe
}

fn probe_sqlx_driver(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> SqlxDriverProbe {
    probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        executable,
        style,
        driver,
        SQLX_DRIVER_PROBE_TIMEOUT,
        root,
        environment,
        cancellation,
    )
}

#[cfg(all(test, unix))]
fn probe_sqlx_driver_with_timeout(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    timeout: Duration,
) -> SqlxDriverProbe {
    probe_sqlx_driver_with_timeout_and_environment(
        executable,
        style,
        driver,
        timeout,
        Path::new("/"),
        &DoctorEnvironment::default(),
    )
}

#[cfg(all(test, unix))]
fn probe_sqlx_driver_with_timeout_and_environment(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    timeout: Duration,
    root: &Path,
    environment: &DoctorEnvironment,
) -> SqlxDriverProbe {
    probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        executable,
        style,
        driver,
        timeout,
        root,
        environment,
        None,
    )
}

fn probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    timeout: Duration,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> SqlxDriverProbe {
    let Ok(temp) = tempfile::tempdir() else {
        return SqlxDriverProbe::Indeterminate(
            "could not create an isolated probe directory".into(),
        );
    };
    let migrations = temp.path().join("migrations");
    if fs::create_dir(&migrations).is_err() {
        return SqlxDriverProbe::Indeterminate(
            "could not create an isolated migration source".into(),
        );
    }
    let mut command = Command::new(executable);
    if style == SqlxProbeStyle::CargoSubcommand {
        // Cargo subcommand shims receive the subcommand name as argv[1].
        command.arg("sqlx");
    }
    command
        .args(["migrate", "info", "--source"])
        .arg(&migrations)
        .arg("--no-dotenv")
        .args(["--database-url", driver.probe_url()])
        .current_dir(temp.path())
        .env_clear()
        .env("NO_COLOR", "1")
        .env("LC_ALL", "C")
        .env("HOME", temp.path())
        .env("USERPROFILE", temp.path())
        .env("TMPDIR", temp.path())
        .env("TMP", temp.path())
        .env("TEMP", temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = sanitized_probe_search_path(root, executable) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(path_extensions) = sanitized_windows_pathext(environment.path_extensions.as_deref())
    {
        command.env("PATHEXT", path_extensions);
    }
    for (key, value) in &environment.probe_environment {
        command.env(key, value);
    }

    let output_result = run_owned_process_tree_with_output(&mut command, timeout, || {
        cancellation.is_some_and(|cancelled| cancelled())
    });

    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            return SqlxDriverProbe::Indeterminate(error.driver_probe_reason().into());
        }
    };
    let Some(stdout) = output.stdout.as_ref() else {
        return SqlxDriverProbe::Indeterminate("the driver probe output was not captured".into());
    };
    let Some(stderr) = output.stderr.as_ref() else {
        return SqlxDriverProbe::Indeterminate("the driver probe output was not captured".into());
    };
    if !stdout.complete || !stderr.complete {
        return SqlxDriverProbe::Indeterminate(
            "the driver probe output capture did not complete".into(),
        );
    }
    if stdout.truncated || stderr.truncated {
        return SqlxDriverProbe::Indeterminate(
            "the driver probe output exceeded the diagnostic capture limit".into(),
        );
    }
    classify_sqlx_driver_probe(
        driver,
        output.status.success(),
        &stdout.to_string_lossy(),
        &stderr.to_string_lossy(),
    )
}

pub(crate) struct OwnedProcessTreeOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Option<BoundedProcessOutput>,
    pub(crate) stderr: Option<BoundedProcessOutput>,
}

pub(crate) struct BoundedProcessOutput {
    bytes: Vec<u8>,
    pub(crate) truncated: bool,
    pub(crate) complete: bool,
}

impl BoundedProcessOutput {
    pub(crate) fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

#[derive(Debug)]
pub(crate) enum OwnedProcessTreeError {
    Start(std::io::Error),
    TimedOut,
    Cancelled,
    Await,
    Cleanup,
}

impl OwnedProcessTreeError {
    fn driver_probe_reason(&self) -> &'static str {
        match self {
            Self::Start(_) => "the driver probe could not start",
            Self::TimedOut => "the driver probe timed out",
            Self::Cancelled => "the driver probe was cancelled",
            Self::Await => "the driver probe could not be awaited",
            Self::Cleanup => "the driver probe process tree could not be cleaned up safely",
        }
    }
}

impl std::fmt::Display for OwnedProcessTreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "the process tree could not start: {error}"),
            Self::TimedOut => formatter.write_str("the process tree timed out"),
            Self::Cancelled => formatter.write_str("the process tree was cancelled"),
            Self::Await => formatter.write_str("the process tree could not be awaited"),
            Self::Cleanup => formatter.write_str("the process tree could not be cleaned up safely"),
        }
    }
}

impl std::error::Error for OwnedProcessTreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error) => Some(error),
            _ => None,
        }
    }
}

pub(crate) fn run_owned_process_tree_with_output(
    command: &mut Command,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_limits(
        command,
        timeout,
        ProcessOutputLimits::diagnostic_default(),
        cancelled,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProcessOutputLimits {
    stdout: usize,
    stderr: usize,
}

impl ProcessOutputLimits {
    const fn diagnostic_default() -> Self {
        Self {
            stdout: OWNED_PROCESS_OUTPUT_LIMIT,
            stderr: OWNED_PROCESS_OUTPUT_LIMIT,
        }
    }

    const fn proxy_list() -> Self {
        Self {
            stdout: 8 * 1024 * 1024,
            stderr: OWNED_PROCESS_OUTPUT_LIMIT,
        }
    }
}

fn run_owned_process_tree_with_output_limits(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    mut cancelled: impl FnMut() -> bool,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    if cancelled() {
        return Err(OwnedProcessTreeError::Cancelled);
    }
    let mut process = spawn_probe_process(command).map_err(OwnedProcessTreeError::Start)?;
    let mut drains = match ProbeOutputDrains::start(&mut process.child, limits) {
        Ok(drains) => drains,
        Err(_) => {
            return match process.terminate_and_reap() {
                Ok(_) => Err(OwnedProcessTreeError::Await),
                Err(_) => Err(OwnedProcessTreeError::Cleanup),
            };
        }
    };
    let wait_result = wait_for_probe_leader(&mut process, timeout, &mut cancelled, &mut drains);
    let status = finish_probe_wait(&mut process, wait_result);
    let (stdout, stderr) = drains.finish(OWNED_PROCESS_OUTPUT_DRAIN_TIMEOUT);
    status.map(|status| OwnedProcessTreeOutput {
        status,
        stdout,
        stderr,
    })
}

enum ProcessPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

impl ProcessPipe {
    #[cfg(unix)]
    fn prepare(&self) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;

        let descriptor = match self {
            Self::Stdout(reader) => reader.as_raw_fd(),
            Self::Stderr(reader) => reader.as_raw_fd(),
        };
        // SAFETY: the descriptor is owned by the live pipe reader. F_GETFL
        // only inspects its current status flags.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the descriptor remains live and F_SETFL preserves every
        // existing flag while adding nonblocking reads.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(windows)]
    fn prepare(&self) -> std::io::Result<()> {
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn prepare(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "nonblocking process-pipe reads are unavailable on this platform",
        ))
    }

    #[cfg(unix)]
    fn read_available(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Stdout(reader) => reader.read(buffer),
            Self::Stderr(reader) => reader.read(buffer),
        }
    }

    #[cfg(windows)]
    fn read_available(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, HANDLE};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        let handle = match self {
            Self::Stdout(reader) => reader.as_raw_handle(),
            Self::Stderr(reader) => reader.as_raw_handle(),
        } as HANDLE;
        let mut available = 0_u32;
        // SAFETY: `handle` is a live anonymous-pipe read handle and the only
        // output pointer names a writable u32. No bytes are copied by this
        // availability-only call.
        let peeked = unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_NO_DATA as i32
            ) {
                return Ok(0);
            }
            return Err(error);
        }
        if available == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        }
        let read_limit = buffer.len().min(available as usize);
        match self {
            Self::Stdout(reader) => reader.read(&mut buffer[..read_limit]),
            Self::Stderr(reader) => reader.read(&mut buffer[..read_limit]),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn read_available(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "nonblocking process-pipe reads are unavailable on this platform",
        ))
    }
}

struct OutputDrain {
    reader: Option<ProcessPipe>,
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
    complete: bool,
}

impl OutputDrain {
    fn start(reader: ProcessPipe, limit: usize) -> std::io::Result<Self> {
        reader.prepare()?;
        Ok(Self {
            reader: Some(reader),
            bytes: Vec::new(),
            limit,
            truncated: false,
            complete: false,
        })
    }

    fn poll(&mut self) {
        const MAX_READS_PER_POLL: usize = 16;

        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        let mut chunk = [0_u8; 4096];
        for _ in 0..MAX_READS_PER_POLL {
            match reader.read_available(&mut chunk) {
                Ok(0) => {
                    self.complete = true;
                    self.reader = None;
                    return;
                }
                Ok(read) => {
                    let remaining = self.limit.saturating_sub(self.bytes.len());
                    let retained = remaining.min(read);
                    self.bytes.extend_from_slice(&chunk[..retained]);
                    self.truncated |= retained < read;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return,
                Err(_) => {
                    // Closing the reader makes an I/O failure terminal and
                    // records the capture as incomplete without retaining a
                    // blocked worker or retry loop.
                    self.reader = None;
                    return;
                }
            }
        }
    }

    fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    fn finish(self) -> BoundedProcessOutput {
        BoundedProcessOutput {
            bytes: self.bytes,
            truncated: self.truncated,
            complete: self.complete,
        }
    }
}

struct ProbeOutputDrains {
    stdout: Option<OutputDrain>,
    stderr: Option<OutputDrain>,
}

impl ProbeOutputDrains {
    fn start(child: &mut Child, limits: ProcessOutputLimits) -> std::io::Result<Self> {
        let stdout = child
            .stdout
            .take()
            .map(|reader| OutputDrain::start(ProcessPipe::Stdout(reader), limits.stdout))
            .transpose()?;
        let stderr = child
            .stderr
            .take()
            .map(|reader| OutputDrain::start(ProcessPipe::Stderr(reader), limits.stderr))
            .transpose()?;
        Ok(Self { stdout, stderr })
    }

    fn poll(&mut self) {
        if let Some(stdout) = &mut self.stdout {
            stdout.poll();
        }
        if let Some(stderr) = &mut self.stderr {
            stderr.poll();
        }
    }

    fn is_terminal(&self) -> bool {
        self.stdout.as_ref().is_none_or(OutputDrain::is_terminal)
            && self.stderr.as_ref().is_none_or(OutputDrain::is_terminal)
    }

    fn finish(
        mut self,
        timeout: Duration,
    ) -> (Option<BoundedProcessOutput>, Option<BoundedProcessOutput>) {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        while !self.is_terminal() && Instant::now() < deadline {
            self.poll();
            if !self.is_terminal() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        // Dropping any still-open reader here closes the local pipe promptly.
        // No worker owns another copy, so an escaped silent writer cannot keep
        // a detached capture thread alive.
        let stdout = self.stdout.map(OutputDrain::finish);
        let stderr = self.stderr.map(OutputDrain::finish);
        (stdout, stderr)
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PinnedProbeProcessGroup {
    id: libc::pid_t,
}

struct ProbeProcess {
    child: Child,
    #[cfg(unix)]
    process_group: Option<PinnedProbeProcessGroup>,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
    reaped_status: Option<ExitStatus>,
    cleanup_complete: bool,
    cleanup_finalized: bool,
    cleanup_error: Option<StoredProbeCleanupError>,
    cleanup_deadline: Option<Instant>,
}

#[derive(Clone, Debug)]
struct StoredProbeCleanupError {
    kind: std::io::ErrorKind,
    message: String,
}

impl StoredProbeCleanupError {
    fn capture(error: &std::io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    fn to_io_error(&self) -> std::io::Error {
        std::io::Error::new(self.kind, self.message.clone())
    }
}

impl ProbeProcess {
    fn cleanup_deadline(&mut self) -> Instant {
        *self.cleanup_deadline.get_or_insert_with(|| {
            Instant::now()
                .checked_add(OWNED_PROCESS_TREE_CLEANUP_TIMEOUT)
                .unwrap_or_else(Instant::now)
        })
    }

    fn terminate_and_reap(&mut self) -> std::io::Result<ExitStatus> {
        if self.cleanup_finalized {
            return if self.cleanup_complete {
                self.reaped_status.ok_or_else(|| {
                    std::io::Error::other("probe cleanup completed without a leader status")
                })
            } else {
                Err(self
                    .cleanup_error
                    .as_ref()
                    .map(StoredProbeCleanupError::to_io_error)
                    .unwrap_or_else(|| {
                        std::io::Error::other("probe cleanup failed without a retained error")
                    }))
            };
        }

        let deadline = self.cleanup_deadline();
        let mut tree_cleanup_error = terminate_probe_process_tree(self, deadline).err();
        let mut direct_fallback_error = None;
        if tree_cleanup_error.is_some() && self.reaped_status.is_none() {
            direct_fallback_error = terminate_probe_leader_fallback(self).err();
        }

        let mut reap_error = None;
        if self.reaped_status.is_none() {
            // Every permitted signal is attempted while the direct child's
            // unconsumed wait status still pins its Unix PID/PGID generation.
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            match self.child.wait_timeout(remaining) {
                Ok(Some(status)) => {
                    self.reaped_status = Some(status);
                    #[cfg(unix)]
                    {
                        self.process_group = None;
                    }
                }
                Ok(None) => {
                    reap_error = Some(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "probe process cleanup timed out while reaping the direct child",
                    ));
                }
                Err(error) => {
                    #[cfg(unix)]
                    update_probe_identity_after_wait_error(self, &error);
                    reap_error = Some(error);
                }
            }
        }

        if let Some(error) = tree_cleanup_error.take() {
            let error = append_probe_cleanup_error(
                error,
                "direct-child fallback also failed",
                direct_fallback_error,
            );
            let error =
                append_probe_cleanup_error(error, "direct-child reap also failed", reap_error);
            return self.finalize_cleanup(Err(error));
        }
        if let Some(error) = reap_error {
            return self.finalize_cleanup(Err(error));
        }
        let status = self.reaped_status.ok_or_else(|| {
            std::io::Error::other("probe cleanup completed without a leader status")
        });
        self.finalize_cleanup(status)
    }

    fn finalize_cleanup(
        &mut self,
        result: std::io::Result<ExitStatus>,
    ) -> std::io::Result<ExitStatus> {
        self.cleanup_finalized = true;
        match result {
            Ok(status) => {
                self.cleanup_complete = true;
                Ok(status)
            }
            Err(error) => {
                self.cleanup_error = Some(StoredProbeCleanupError::capture(&error));
                Err(error)
            }
        }
    }
}

fn append_probe_cleanup_error(
    primary: std::io::Error,
    label: &str,
    secondary: Option<std::io::Error>,
) -> std::io::Error {
    match secondary {
        Some(secondary) => {
            std::io::Error::new(primary.kind(), format!("{primary}; {label}: {secondary}"))
        }
        None => primary,
    }
}

impl Drop for ProbeProcess {
    fn drop(&mut self) {
        let _ = self.terminate_and_reap();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeLeaderWait {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    ExitedUnreaped,
    #[cfg(windows)]
    ExitedReaped(ExitStatus),
    TimedOut,
    Cancelled,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeLeaderObservation {
    Running,
    Exited,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_probe_leader(
    process: &mut ProbeProcess,
    timeout: Duration,
    cancelled: &mut impl FnMut() -> bool,
    drains: &mut ProbeOutputDrains,
) -> std::io::Result<ProbeLeaderWait> {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        drains.poll();
        if cancelled() {
            return Ok(ProbeLeaderWait::Cancelled);
        }
        if observe_probe_leader(process)? == ProbeLeaderObservation::Exited {
            return Ok(ProbeLeaderWait::ExitedUnreaped);
        }

        match deadline {
            Some(deadline) => {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    return Ok(ProbeLeaderWait::TimedOut);
                };
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[cfg(unix)]
fn update_probe_identity_after_wait_error(process: &mut ProbeProcess, error: &std::io::Error) {
    // ECHILD proves that this process no longer owns an unconsumed wait status;
    // another SIGCHLD consumer may have reaped the leader and released its
    // PID/PGID. EINVAL, ENOSYS, and other observation errors do not consume the
    // status, so the direct child continues to pin the group identity.
    if error.raw_os_error() == Some(libc::ECHILD) {
        process.process_group = None;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminate_probe_leader_fallback(process: &mut ProbeProcess) -> std::io::Result<()> {
    if process.process_group.is_none() {
        return Err(std::io::Error::other(
            "probe child identity is no longer pinned; refusing direct fallback",
        ));
    }
    match process.child.kill() {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
            if observe_probe_leader(process)? == ProbeLeaderObservation::Exited {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn terminate_probe_leader_fallback(process: &mut ProbeProcess) -> std::io::Result<()> {
    // `Child` retains the exact process HANDLE even if Job Object termination
    // or confirmation failed, so this fallback cannot target a recycled PID.
    process.child.kill()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminate_probe_leader_fallback(_process: &mut ProbeProcess) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(windows)]
fn wait_for_probe_leader(
    process: &mut ProbeProcess,
    timeout: Duration,
    cancelled: &mut impl FnMut() -> bool,
    drains: &mut ProbeOutputDrains,
) -> std::io::Result<ProbeLeaderWait> {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        drains.poll();
        if cancelled() {
            return Ok(ProbeLeaderWait::Cancelled);
        }
        let remaining = match deadline {
            Some(deadline) => {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    return Ok(ProbeLeaderWait::TimedOut);
                };
                remaining
            }
            None => Duration::from_millis(10),
        };
        if let Some(status) = process
            .child
            .wait_timeout(remaining.min(Duration::from_millis(10)))?
        {
            return Ok(ProbeLeaderWait::ExitedReaped(status));
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn wait_for_probe_leader(
    _process: &mut ProbeProcess,
    _timeout: Duration,
    _cancelled: &mut impl FnMut() -> bool,
    drains: &mut ProbeOutputDrains,
) -> std::io::Result<ProbeLeaderWait> {
    drains.poll();
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

fn finish_probe_wait(
    process: &mut ProbeProcess,
    wait_result: std::io::Result<ProbeLeaderWait>,
) -> std::result::Result<ExitStatus, OwnedProcessTreeError> {
    #[cfg(windows)]
    let wait_result = match wait_result {
        Ok(ProbeLeaderWait::ExitedReaped(status)) => {
            // A Windows Job Object remains a stable tree identity after its
            // leader exits. Cache the consumed status, terminate the owned
            // job, and only then mark cleanup complete.
            process.reaped_status = Some(status);
            return process
                .terminate_and_reap()
                .map_err(|_| OwnedProcessTreeError::Cleanup);
        }
        other => other,
    };
    let outcome = match wait_result {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        Ok(ProbeLeaderWait::ExitedUnreaped) => None,
        #[cfg(windows)]
        Ok(ProbeLeaderWait::ExitedReaped(status)) => Some(Ok(status)),
        Ok(ProbeLeaderWait::TimedOut) => Some(Err(OwnedProcessTreeError::TimedOut)),
        Ok(ProbeLeaderWait::Cancelled) => Some(Err(OwnedProcessTreeError::Cancelled)),
        Err(_) => Some(Err(OwnedProcessTreeError::Await)),
    };
    // A probe leader can exit while a background descendant keeps running.
    // End the owned tree on every outcome before reading captured output.
    let cleanup = process.terminate_and_reap();
    if cleanup.is_err() {
        return Err(OwnedProcessTreeError::Cleanup);
    }
    match outcome {
        Some(outcome) => outcome,
        None => cleanup.map_err(|_| OwnedProcessTreeError::Await),
    }
}

fn terminate_spawn_failure_child(child: &mut Child, deadline: Instant) {
    let _ = child.kill();
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_default();
    let _ = child.wait_timeout(remaining);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_probe_process(command: &mut Command) -> std::io::Result<ProbeProcess> {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
    let mut child = command.spawn()?;
    let Ok(process_group) = libc::pid_t::try_from(child.id()) else {
        let deadline = Instant::now()
            .checked_add(OWNED_PROCESS_TREE_CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(std::io::Error::other(
            "probe process identifier is not representable",
        ));
    };
    Ok(ProbeProcess {
        child,
        process_group: Some(PinnedProbeProcessGroup { id: process_group }),
        reaped_status: None,
        cleanup_complete: false,
        cleanup_finalized: false,
        cleanup_error: None,
        cleanup_deadline: None,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn spawn_probe_process(_command: &mut Command) -> std::io::Result<ProbeProcess> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe_probe_leader(process: &mut ProbeProcess) -> std::io::Result<ProbeLeaderObservation> {
    let process_group = process
        .process_group
        .ok_or_else(|| std::io::Error::other("probe process-group identity is no longer pinned"))?;
    let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: `information` is writable storage, the identifier names our
    // direct child, and WNOWAIT retains its status so the PID continues to pin
    // this exact process-group generation through cleanup.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_group.id as _,
            information.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == 0 {
        // SAFETY: successful `waitid` initialized the siginfo value and its
        // SIGCHLD union member.
        let information = unsafe { information.assume_init() };
        let observed_pid = unsafe { information.si_pid() };
        return classify_probe_waitid_observation(
            process_group.id,
            observed_pid,
            information.si_code,
        );
    }

    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::Interrupted {
        return Ok(ProbeLeaderObservation::Running);
    }
    update_probe_identity_after_wait_error(process, &error);
    Err(error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn classify_probe_waitid_observation(
    expected_pid: libc::pid_t,
    observed_pid: libc::pid_t,
    code: libc::c_int,
) -> std::io::Result<ProbeLeaderObservation> {
    if observed_pid == 0 {
        return Ok(ProbeLeaderObservation::Running);
    }
    if observed_pid != expected_pid {
        return Err(std::io::Error::other(format!(
            "waitid observed unexpected probe child PID {observed_pid} instead of {expected_pid}"
        )));
    }
    match code {
        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED => {
            Ok(ProbeLeaderObservation::Exited)
        }
        libc::CLD_STOPPED | libc::CLD_TRAPPED | libc::CLD_CONTINUED => {
            Ok(ProbeLeaderObservation::Running)
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("waitid returned an unrecognized probe child state code {code}"),
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminate_probe_process_tree(
    process: &mut ProbeProcess,
    deadline: Instant,
) -> std::io::Result<()> {
    ensure_probe_cleanup_budget(deadline, "before process-group termination")?;
    let process_group = process.process_group.ok_or_else(|| {
        std::io::Error::other(
            "probe process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    if process_group.id <= 0 {
        return Err(std::io::Error::other(
            "probe process-group identity is not positive",
        ));
    }
    confirm_probe_process_group_quiescent(process, process_group.id, deadline)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeProcessGroupSignalResult {
    Delivered,
    Inconclusive,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pinned_probe_process_group_for_retry(
    process: &ProbeProcess,
    expected_process_group: libc::pid_t,
) -> std::io::Result<PinnedProbeProcessGroup> {
    let process_group = process.process_group.ok_or_else(|| {
        std::io::Error::other(
            "probe process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    if process_group.id != expected_process_group {
        return Err(std::io::Error::other(format!(
            "probe process-group identity changed from pinned group {expected_process_group} to {}",
            process_group.id
        )));
    }
    if process_group.id <= 0 {
        return Err(std::io::Error::other(
            "probe process-group identity is not positive",
        ));
    }
    Ok(process_group)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe_probe_leader_before_group_signal_with<T>(
    state: &mut T,
    mut observe: impl FnMut(&mut T) -> std::io::Result<ProbeLeaderObservation>,
    signal: impl FnOnce(
        &mut T,
        ProbeLeaderObservation,
    ) -> std::io::Result<ProbeProcessGroupSignalResult>,
) -> std::io::Result<ProbeProcessGroupSignalResult> {
    let observation = observe(state)?;
    signal(state, observation)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn signal_pinned_probe_process_group(
    process: &mut ProbeProcess,
    expected_process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<ProbeProcessGroupSignalResult> {
    ensure_probe_cleanup_budget(deadline, "before process-group SIGKILL")?;
    pinned_probe_process_group_for_retry(process, expected_process_group)?;
    observe_probe_leader_before_group_signal_with(
        process,
        observe_probe_leader,
        |process, leader_observation| {
            // The exact WNOWAIT observation above must precede every numeric
            // group signal. If another waiter consumed the status, ECHILD has
            // already cleared the cached identity and this closure is never
            // entered.
            ensure_probe_cleanup_budget(deadline, "after pre-signal leader observation")?;
            let process_group =
                pinned_probe_process_group_for_retry(process, expected_process_group)?;
            // SAFETY: the positive group identifier was revalidated after a
            // fresh non-consuming observation of our direct child. Its
            // unconsumed wait status pins this exact process-group generation.
            if unsafe { libc::kill(-process_group.id, libc::SIGKILL) } == 0 {
                return Ok(ProbeProcessGroupSignalResult::Delivered);
            }

            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                // ESRCH only says that this pinned generation had no signalable
                // member at this instant. A concurrently starting descendant
                // may still become visible, so only the following platform
                // proof may finish cleanup.
                return Ok(ProbeProcessGroupSignalResult::Inconclusive);
            }
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM) {
                return resolve_macos_probe_group_signal_eperm(error, Ok(leader_observation));
            }
            #[cfg(not(target_os = "macos"))]
            let _ = leader_observation;
            Err(error)
        },
    )
}

#[cfg(target_os = "macos")]
fn resolve_macos_probe_group_signal_eperm(
    signal_error: std::io::Error,
    leader_observation: std::io::Result<ProbeLeaderObservation>,
) -> std::io::Result<ProbeProcessGroupSignalResult> {
    match leader_observation {
        Ok(ProbeLeaderObservation::Exited) => {
            // Darwin can report EPERM for a group containing only its zombie
            // leader, but EPERM is not absence. The confirmation loop must
            // still take a fresh atomic sole-leader snapshot before success.
            Ok(ProbeProcessGroupSignalResult::Inconclusive)
        }
        Ok(ProbeLeaderObservation::Running) => Err(signal_error),
        Err(observation_error) => Err(std::io::Error::new(
            observation_error.kind(),
            format!(
                "process-group SIGKILL failed: {signal_error}; failed to verify the pinned leader after EPERM: {observation_error}"
            ),
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn confirm_probe_process_group_quiescent_with<T>(
    state: &mut T,
    process_group: libc::pid_t,
    deadline: Instant,
    required_consecutive_proofs: u8,
    timeout_phase: &str,
    mut signal: impl FnMut(
        &mut T,
        libc::pid_t,
        Instant,
    ) -> std::io::Result<ProbeProcessGroupSignalResult>,
    mut prove_quiescent: impl FnMut(&mut T, libc::pid_t, Instant) -> std::io::Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> std::io::Result<()> {
    if required_consecutive_proofs == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "probe process-group confirmation requires at least one proof",
        ));
    }

    let mut consecutive_proofs = 0_u8;
    loop {
        probe_cleanup_remaining_at(deadline, now(), timeout_phase)?;
        // Signal before every proof. A descendant can become visible in this
        // pinned group after an earlier group signal, so polling alone cannot
        // make a prior SIGKILL authoritative for a later membership snapshot.
        let _signal_result = signal(state, process_group, deadline)?;
        probe_cleanup_remaining_at(deadline, now(), "after process-group SIGKILL")?;
        let quiescent = prove_quiescent(state, process_group, deadline)?;
        // Never accept a proof that completed outside the original absolute
        // cleanup budget.
        probe_cleanup_remaining_at(deadline, now(), "after process-group confirmation")?;
        if quiescent {
            consecutive_proofs += 1;
            if consecutive_proofs == required_consecutive_proofs {
                return Ok(());
            }
        } else {
            consecutive_proofs = 0;
        }

        let remaining = probe_cleanup_remaining_at(deadline, now(), timeout_phase)?;
        sleep(remaining.min(OWNED_PROCESS_TREE_POLL_INTERVAL));
    }
}

#[cfg(target_os = "linux")]
fn confirm_probe_process_group_quiescent(
    process: &mut ProbeProcess,
    process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<()> {
    confirm_probe_process_group_quiescent_with(
        process,
        process_group,
        deadline,
        2,
        "while confirming the Linux process group",
        signal_pinned_probe_process_group,
        |_process, process_group, deadline| {
            linux_probe_group_has_live_members(process_group, deadline).map(|live| !live)
        },
        Instant::now,
        std::thread::sleep,
    )
}

#[cfg(target_os = "macos")]
fn confirm_probe_process_group_quiescent(
    process: &mut ProbeProcess,
    process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<()> {
    confirm_probe_process_group_quiescent_with(
        process,
        process_group,
        deadline,
        1,
        "while confirming the macOS process group",
        signal_pinned_probe_process_group,
        |process, process_group, deadline| {
            pinned_probe_process_group_for_retry(process, process_group)?;
            let leader_exited = observe_probe_leader(process)? == ProbeLeaderObservation::Exited;
            ensure_probe_cleanup_budget(deadline, "after macOS leader observation")?;
            if !leader_exited {
                return Ok(false);
            }
            let sole_pinned_leader = macos_probe_group_contains_only_pinned_leader(process_group)?;
            ensure_probe_cleanup_budget(deadline, "after macOS process-group snapshot")?;
            Ok(sole_pinned_leader)
        },
        Instant::now,
        std::thread::sleep,
    )
}

#[cfg(target_os = "macos")]
fn macos_probe_group_contains_only_pinned_leader(
    process_group: libc::pid_t,
) -> std::io::Result<bool> {
    let mut members = [0 as libc::pid_t; 2];
    let buffer_size = i32::try_from(std::mem::size_of_val(&members)).map_err(|_| {
        std::io::Error::other("macOS process-group snapshot buffer was not representable")
    })?;
    // SAFETY: `members` is writable storage for two pid_t values and the byte
    // count describes that full buffer. A full buffer means at least two live
    // group entries, so collection is intentionally capped at two.
    let count =
        unsafe { libc::proc_listpgrppids(process_group, members.as_mut_ptr().cast(), buffer_size) };
    if count <= 0 {
        return Err(std::io::Error::other(format!(
            "failed to atomically list probe process group {process_group}: {}",
            std::io::Error::last_os_error()
        )));
    }
    classify_macos_probe_group_snapshot(process_group, count, members)
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_probe_group_snapshot(
    process_group: i32,
    count: i32,
    members: [i32; 2],
) -> std::io::Result<bool> {
    if process_group <= 0 {
        return Err(std::io::Error::other(
            "macOS process-group snapshot used a non-positive pinned leader",
        ));
    }
    let count = usize::try_from(count).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "macOS process-group snapshot returned a negative member count",
        )
    })?;
    if count == 0 || count > members.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("macOS process-group snapshot returned an untrusted member count of {count}"),
        ));
    }
    let observed = &members[..count];
    if observed.iter().any(|pid| *pid <= 0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "macOS process-group snapshot returned a non-positive member identifier",
        ));
    }
    if count == members.len() {
        return Ok(false);
    }
    if observed[0] != process_group {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "macOS process-group snapshot did not contain the exact pinned leader {process_group}"
            ),
        ));
    }
    Ok(true)
}

#[cfg(target_os = "linux")]
fn linux_probe_group_has_live_members(
    process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<bool> {
    let mut within_budget = || {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| !remaining.is_zero())
    };
    ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
    let entries = std::fs::read_dir("/proc");
    ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
    let pids = collect_linux_probe_process_ids_with(
        process_group,
        entries?,
        |entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<libc::pid_t>().ok())
        },
        &mut within_budget,
    )?;
    linux_probe_group_has_live_members_with(
        process_group,
        pids,
        |pid| std::fs::read_to_string(format!("/proc/{pid}/stat")),
        linux_probe_process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_probe_process_ids_with<T>(
    process_group: i32,
    mut entries: impl Iterator<Item = std::io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<i32>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<Vec<i32>> {
    let mut pids = Vec::new();
    loop {
        ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
        let entry = entries.next();
        ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
        let Some(entry) = entry else {
            return Ok(pids);
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if let Some(pid) = process_id(entry) {
            pids.push(pid);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn ensure_linux_probe_scan_budget(
    _process_group: i32,
    within_budget: &mut impl FnMut() -> bool,
) -> std::io::Result<()> {
    if within_budget() {
        Ok(())
    } else {
        Err(probe_cleanup_timeout("while scanning Linux processes"))
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_probe_group_has_live_members_with(
    process_group: i32,
    pids: impl IntoIterator<Item = i32>,
    mut read_stat: impl FnMut(i32) -> std::io::Result<String>,
    mut process_group_for_pid: impl FnMut(i32) -> std::io::Result<Option<i32>>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<bool> {
    ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
    for pid in pids {
        ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
        let observation = read_stat(pid).and_then(|stat| parse_linux_probe_process_stat(&stat));
        ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
        let observation = match observation {
            Ok(observation) => observation,
            Err(stat_error) => {
                ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
                let observed_group = process_group_for_pid(pid);
                ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
                match observed_group {
                    Ok(None) => continue,
                    Ok(Some(other_group)) if other_group != process_group => continue,
                    Ok(Some(_)) => {
                        return Err(std::io::Error::new(
                            stat_error.kind(),
                            format!(
                                "could not inspect process {pid} in owned group {process_group}: {stat_error}"
                            ),
                        ));
                    }
                    Err(group_error) => {
                        return Err(std::io::Error::new(
                            stat_error.kind(),
                            format!(
                                "could not inspect process {pid} or prove it is outside owned group {process_group}: {stat_error}; group lookup failed: {group_error}"
                            ),
                        ));
                    }
                }
            }
        };
        if observation.process_group == process_group && observation.live {
            ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
            return Ok(true);
        }
    }
    ensure_linux_probe_scan_budget(process_group, &mut within_budget)?;
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxProbeProcessObservation {
    process_group: i32,
    live: bool,
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_probe_process_stat(stat: &str) -> std::io::Result<LinuxProbeProcessObservation> {
    let (_, fields) = stat.rsplit_once(") ").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing Linux process stat command field",
        )
    })?;
    let mut fields = fields.split_whitespace();
    let state = fields.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing process state")
    })?;
    let process_group = fields
        .nth(1)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing process group")
        })?
        .parse::<i32>()
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid process group")
        })?;
    Ok(LinuxProbeProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
fn linux_probe_process_group_for_pid(pid: libc::pid_t) -> std::io::Result<Option<libc::pid_t>> {
    // SAFETY: `pid` is a positive identifier enumerated from /proc and this
    // call only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return Ok(Some(process_group));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ensure_probe_cleanup_budget(deadline: Instant, phase: &str) -> std::io::Result<()> {
    probe_cleanup_remaining_at(deadline, Instant::now(), phase).map(|_| ())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn probe_cleanup_remaining_at(
    deadline: Instant,
    now: Instant,
    phase: &str,
) -> std::io::Result<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| probe_cleanup_timeout(phase))
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn probe_cleanup_timeout(phase: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("probe process-tree cleanup timed out {phase}"),
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminate_probe_process_tree(
    _process: &mut ProbeProcess,
    _deadline: Instant,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(windows)]
fn spawn_probe_process(command: &mut Command) -> std::io::Result<ProbeProcess> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = create_probe_job()?;
    command.creation_flags(windows_probe_creation_flags());
    let mut child = command.spawn()?;
    // SAFETY: both handles are live handles owned by `job` and `child`.
    let assigned = unsafe {
        AssignProcessToJobObject(
            job.as_raw_handle() as HANDLE,
            child.as_raw_handle() as HANDLE,
        )
    };
    if assigned == 0 || resume_probe_process(child.id()).is_err() {
        let deadline = Instant::now()
            .checked_add(OWNED_PROCESS_TREE_CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        let _ = terminate_windows_job(&job, deadline);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(std::io::Error::other(
            "could not isolate the probe process tree",
        ));
    }
    Ok(ProbeProcess {
        child,
        job,
        reaped_status: None,
        cleanup_complete: false,
        cleanup_finalized: false,
        cleanup_error: None,
        cleanup_deadline: None,
    })
}

#[cfg(windows)]
const fn windows_probe_creation_flags() -> u32 {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP
}

#[cfg(windows)]
fn create_probe_job() -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // SAFETY: null attributes and name request a private unnamed Job Object.
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw_job` is a newly created owned handle and is transferred
    // exactly once into `OwnedHandle`.
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job as RawHandle) };
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the information pointer and byte length describe a live value of
    // the class requested, and the Job Object handle remains owned by `job`.
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw const information).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(job)
}

#[cfg(windows)]
fn resume_probe_process(pid: u32) -> std::io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot call has no borrowed pointer arguments.
    let raw_snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw_snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw_snapshot` is a newly created owned handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw_snapshot as RawHandle) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: `entry` is initialized with the required size and remains valid
    // for the duration of the snapshot enumeration.
    let mut has_entry = unsafe { Thread32First(raw_snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the enumerated thread ID belongs to the suspended child;
            // no handle is inherited by the resumed process.
            let raw_thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if raw_thread.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: `raw_thread` is a newly opened owned handle.
            let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread as RawHandle) };
            // SAFETY: `thread` names the initial thread created suspended by
            // `CREATE_SUSPENDED` and remains live for this call.
            let previous_count = unsafe { ResumeThread(raw_thread) };
            drop(thread);
            drop(snapshot);
            if previous_count == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: same initialized entry and live snapshot as above.
        has_entry = unsafe { Thread32Next(raw_snapshot, &mut entry) } != 0;
    }
    Err(std::io::Error::other(
        "could not find the suspended probe thread",
    ))
}

#[cfg(windows)]
fn terminate_windows_job(
    job: &std::os::windows::io::OwnedHandle,
    deadline: Instant,
) -> std::io::Result<()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, TerminateJobObject,
    };

    // SAFETY: `job` is a live Job Object owned by the caller.
    let result = unsafe { TerminateJobObject(job.as_raw_handle() as HANDLE, 1) };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    wait_for_no_active_processes_until(deadline, || {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the Job Object handle is live, the output pointer names a
        // correctly sized accounting value, and no return-length is needed.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectBasicAccountingInformation,
                (&raw mut information).cast::<c_void>(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(information.ActiveProcesses)
        }
    })
}

#[cfg(windows)]
fn terminate_probe_process_tree(
    process: &mut ProbeProcess,
    deadline: Instant,
) -> std::io::Result<()> {
    terminate_windows_job(&process.job, deadline)
}

#[cfg(test)]
fn wait_for_no_active_processes(
    timeout: Duration,
    active_processes: impl FnMut() -> std::io::Result<u32>,
) -> std::io::Result<()> {
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(std::time::Instant::now);
    wait_for_no_active_processes_until(deadline, active_processes)
}

#[cfg(any(windows, test))]
fn wait_for_no_active_processes_until(
    deadline: Instant,
    mut active_processes: impl FnMut() -> std::io::Result<u32>,
) -> std::io::Result<()> {
    loop {
        if active_processes()? == 0 {
            return Ok(());
        }
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "probe process tree cleanup timed out",
            ));
        };
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

fn classify_sqlx_driver_probe(
    driver: SqlxDriver,
    success: bool,
    stdout: &str,
    stderr: &str,
) -> SqlxDriverProbe {
    let output = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    if output.contains("no driver found for url scheme") {
        return SqlxDriverProbe::Incompatible;
    }
    let postgres_driver_rejected_sslmode = driver == SqlxDriver::Postgres
        && output.lines().any(|line| {
            line.contains("jig-doctor-invalid")
                && (line.contains("sslmode") || line.contains("ssl_mode"))
                && (line.contains("unknown value") || line.contains("invalid value"))
        });
    if success || postgres_driver_rejected_sslmode {
        return SqlxDriverProbe::Compatible;
    }
    SqlxDriverProbe::Indeterminate("cargo-sqlx returned an unexpected result".into())
}

fn agent_check(ctx: &RepoContext, process_control: DoctorProcessControl<'_>) -> DoctorCheck {
    let output = crate::runtime::agent_doctor_with_codex_support_probe(ctx, |codex_bin| {
        if let Some(reason) = process_control.unavailable_reason {
            return Err(format!(
                "Codex marketplace support probe was not started because {reason}"
            ));
        }
        if process_control
            .cancellation
            .is_some_and(|cancelled| cancelled())
        {
            return Err("Codex marketplace support probe was cancelled before start".into());
        }
        crate::runtime::probe_codex_marketplace_support(
            codex_bin,
            CODEX_SUPPORT_PROBE_TIMEOUT,
            || {
                process_control
                    .cancellation
                    .is_some_and(|cancelled| cancelled())
            },
        )
    });
    match output {
        Ok(output) => {
            let ok = output["ok"].as_bool().unwrap_or(false);
            let probe_incomplete = output["codex"]["probe_error"].is_string();
            let configured = output["marketplaces"].as_array().map(Vec::len).unwrap_or(0);
            let registered = output["marketplaces"]
                .as_array()
                .map(|marketplaces| {
                    marketplaces
                        .iter()
                        .filter(|marketplace| marketplace["registered"].as_bool().unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);
            let detail = if probe_incomplete {
                "Codex marketplace capability verification is incomplete".into()
            } else if configured == 0 {
                "no agent skill marketplaces configured".into()
            } else {
                format!("{registered}/{configured} configured marketplace(s) registered")
            };
            let fix = output["next_steps"]
                .as_array()
                .and_then(|steps| agent_next_step(steps))
                .map(str::to_string);
            // Agent skills improve the Codex/MCP experience, but a repository
            // with valid config, runtime, contract, and tools is operational.
            check(
                "agent_skills",
                "Agent skills",
                false,
                ok,
                if probe_incomplete {
                    "error"
                } else if ok {
                    "installed"
                } else {
                    "missing"
                },
                detail,
            )
            .with_optional_fix(fix.as_deref())
            .with_data(output)
        }
        Err(error) => check(
            "agent_skills",
            "Agent skills",
            false,
            false,
            "error",
            error.to_string(),
        )
        .with_fix("Run `scripts/jig agent doctor` for agent tooling details."),
    }
}

fn agent_next_step(steps: &[Value]) -> Option<&str> {
    steps
        .iter()
        .filter_map(Value::as_str)
        .find(|step| step.contains("`scripts/jig "))
        .or_else(|| steps.iter().filter_map(Value::as_str).next())
}

fn proxy_configured(ctx: &RepoContext) -> bool {
    !ctx.frontend_apps().is_empty()
        || !ctx.dev_config().apps.is_empty()
        || ctx.dev_config().workspace_discovery
}

fn proxy_check_with_process_control(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let configured = proxy_configured(ctx);
    if !configured {
        return check(
            "proxy",
            "Proxy",
            false,
            true,
            "not configured",
            "no dev apps configured",
        )
        .with_data(json!({ "configured": false }));
    }

    match proxy_list_output(ctx, process_control) {
        Ok(output) => proxy_check_from_output(configured, output),
        Err(error) => check("proxy", "Proxy", false, false, "error", error.to_string())
            .with_fix("Run `scripts/jig proxy list` for proxy diagnostics.")
            .with_data(json!({ "configured": configured })),
    }
}

fn proxy_list_output(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> Result<Value> {
    if let Some(reason) = process_control.unavailable_reason {
        return Err(anyhow!(
            "proxy diagnostics were not started because {reason}"
        ));
    }
    if process_control
        .cancellation
        .is_some_and(|cancelled| cancelled())
    {
        return Err(anyhow!("proxy diagnostics were cancelled before start"));
    }
    let (launcher, mut command) = proxy_list_command(ctx.root())?;
    proxy_list_output_with_timeout_and_limits_and_cancellation(
        &mut command,
        PROXY_LIST_DIAGNOSTIC_TIMEOUT,
        ProcessOutputLimits::proxy_list(),
        || {
            process_control
                .cancellation
                .is_some_and(|cancelled| cancelled())
        },
    )
    .with_context(|| proxy_list_failure_context(&launcher))
}

fn proxy_list_command(root: &Path) -> Result<(PathBuf, Command)> {
    let root = std::path::absolute(root).with_context(|| {
        format!(
            "Failed to resolve the absolute proxy diagnostics root path {}",
            root.display()
        )
    })?;
    let launcher = root.join("scripts/jig");
    #[cfg(windows)]
    let (root, launcher) = (
        crate::shell::windows_bash_compatible_path(&root).with_context(|| {
            format!(
                "Failed to prepare proxy diagnostics root {} for Bash",
                root.display()
            )
        })?,
        crate::shell::windows_bash_compatible_path(&launcher).with_context(|| {
            format!(
                "Failed to prepare proxy diagnostics launcher {} for Bash",
                launcher.display()
            )
        })?,
    );
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("bash");
        command.arg(&launcher);
        command
    };
    #[cfg(not(windows))]
    let mut command = Command::new(&launcher);
    crate::shell::sanitize_bash_environment(&mut command);
    command.args(["proxy", "list", "--json"]).current_dir(&root);
    Ok((launcher, command))
}

fn proxy_list_failure_context(launcher: &Path) -> String {
    #[cfg(windows)]
    {
        format!(
            "Failed to run proxy diagnostics through {} with Bash; run Jig from Git Bash or WSL and ensure `bash` is on PATH",
            launcher.display()
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "Failed to run proxy diagnostics through {}",
            launcher.display()
        )
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
fn proxy_list_output_with_timeout(command: &mut Command, timeout: Duration) -> Result<Value> {
    proxy_list_output_with_timeout_and_limits_and_cancellation(
        command,
        timeout,
        ProcessOutputLimits::proxy_list(),
        || false,
    )
}

fn proxy_list_output_with_timeout_and_limits_and_cancellation(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    cancelled: impl FnMut() -> bool,
) -> Result<Value> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_owned_process_tree_with_output_limits(command, timeout, limits, cancelled)
        .map_err(|error| anyhow!("`scripts/jig proxy list --json` failed: {error}"))?;
    let stdout = output
        .stdout
        .ok_or_else(|| anyhow!("`scripts/jig proxy list --json` stdout was not captured"))?;
    let stderr = output
        .stderr
        .ok_or_else(|| anyhow!("`scripts/jig proxy list --json` stderr was not captured"))?;

    if !stdout.complete || !stderr.complete {
        return Err(anyhow!(
            "`scripts/jig proxy list --json` output capture did not complete"
        ));
    }
    if stdout.truncated || stderr.truncated {
        return Err(anyhow!(
            "`scripts/jig proxy list --json` output exceeded the diagnostic capture limit"
        ));
    }

    if !output.status.success() {
        return Err(anyhow!(
            "`scripts/jig proxy list --json` exited with status {}",
            output.status
        ));
    }

    serde_json::from_slice(&stdout.bytes)
        .context("Failed to parse `scripts/jig proxy list --json` JSON")
}

fn proxy_check_from_output(configured: bool, output: Value) -> DoctorCheck {
    let running = output["running"].as_bool().unwrap_or(false);
    let status = match (configured, running) {
        (true, true) => "running",
        (true, false) => "not running",
        (false, true) => "running unconfigured",
        (false, false) => "not configured",
    };
    check(
        "proxy",
        "Proxy",
        false,
        running,
        status,
        proxy_detail(configured, running, &output),
    )
    .with_optional_fix((configured && !running).then_some("Run `scripts/jig proxy start`."))
    .with_data(json!({
            "configured": configured,
            "status": output,
    }))
}

fn proxy_detail(configured: bool, running: bool, output: &Value) -> String {
    let state_dir = output["state_dir"].as_str().unwrap_or("<unknown>");
    match (configured, running) {
        (true, true) => format!("configured and running; state_dir={state_dir}"),
        (true, false) => format!("configured but not running; state_dir={state_dir}"),
        (false, true) => format!("running, but no dev apps are configured; state_dir={state_dir}"),
        (false, false) => format!("no dev apps configured; state_dir={state_dir}"),
    }
}

fn vault_check(ctx: std::result::Result<&RepoContext, String>) -> DoctorCheck {
    let ctx = match ctx {
        Ok(ctx) => ctx,
        Err(error) => {
            return check(
                "vault",
                "Vault",
                false,
                false,
                "blocked",
                format!("Skipped until repo context loads successfully: {error}"),
            )
            .with_fix("Fix the reported repo context issue, then run `scripts/jig vault status`.");
        }
    };
    // Vault status is intentionally a cheap metadata probe and must not prompt
    // for a passphrase; doctor relies on that non-authenticated boundary.
    match crate::runtime::dispatch_vault(VaultCommand::Status(VaultStatusRequest {
        vault: crate::runtime::vault_options_for_context(Some(ctx)),
    })) {
        Ok(output) => {
            let initialized = output["exists"].as_bool().unwrap_or(false);
            check(
                "vault",
                "Vault",
                false,
                initialized,
                if initialized {
                    "initialized"
                } else {
                    "not initialized"
                },
                vault_detail(&output),
            )
            .with_optional_fix((!initialized).then_some("Run `scripts/jig vault init`."))
            .with_data(output)
        }
        Err(error) => check("vault", "Vault", false, false, "error", error.to_string())
            .with_fix("Run `scripts/jig vault status` for vault diagnostics."),
    }
}

fn vault_detail(output: &Value) -> String {
    let mut detail = format!(
        "vault_home={}",
        output["vault_home"].as_str().unwrap_or("<unknown>")
    );
    if let Some(scope) = output["vault_scope"].as_str() {
        detail.push_str(&format!(" scope={scope}"));
    }
    if let Some(scope_id) = output["vault_scope_id"].as_str() {
        detail.push_str(&format!(" scope_id={scope_id}"));
    }
    detail
}

#[derive(Clone, Debug, Serialize)]
struct DoctorCheck {
    id: String,
    label: String,
    required: bool,
    ok: bool,
    status: String,
    detail: String,
    fix: Option<String>,
    data: Value,
}

fn check(
    id: &str,
    label: &str,
    required: bool,
    ok: bool,
    status: &str,
    detail: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id: id.to_string(),
        label: label.to_string(),
        required,
        ok,
        status: status.to_string(),
        detail: detail.into(),
        fix: None,
        data: json!({}),
    }
}

impl DoctorCheck {
    fn with_fix(mut self, fix: &str) -> Self {
        self.fix = Some(fix.to_string());
        self
    }

    fn with_optional_fix(mut self, fix: Option<&str>) -> Self {
        self.fix = fix.map(str::to_string);
        self
    }

    fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}

#[cfg(test)]
fn command_programs(root: &Path, command: &str) -> Vec<String> {
    required_command_programs(root, command)
        .programs
        .into_iter()
        .map(|program| program.program)
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequiredProgramAmbiguity {
    ShellSyntax,
    ShellState,
    Wrapper,
}

impl RequiredProgramAmbiguity {
    fn description(self) -> &'static str {
        match self {
            Self::ShellSyntax => "the configured Bash syntax cannot be analyzed safely",
            Self::ShellState => {
                "an earlier Bash builtin can change command dispatch or execute hidden commands"
            }
            Self::Wrapper => "a command wrapper can change which executable runs",
        }
    }
}

#[derive(Debug, Default)]
struct RequiredCommandPrograms {
    programs: Vec<RequiredProgram>,
    ambiguity: Option<RequiredProgramAmbiguity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequiredProgram {
    program: String,
    cargo_sqlx_dispatch: bool,
    path_lookup: ProgramPathLookup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProgramPathLookup {
    Explicit,
    Captured,
    CommandLocal(OsString),
    CapturedAfterCwdChange,
    Unverifiable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProgramPresence {
    Present(ProgramResolution),
    Missing,
    Unverified,
}

fn required_command_programs(root: &Path, command: &str) -> RequiredCommandPrograms {
    if let Some(branch) = active_optional_cargo_branch(root, command) {
        return required_command_programs_for_shell(&branch);
    }
    required_command_programs_for_shell(command)
}

fn active_optional_cargo_branch(root: &Path, command: &str) -> Option<String> {
    let (then_branch, else_branch) = crate::shell::optional_cargo_command_branches(command)?;
    Some(
        if root.join("Cargo.toml").exists() {
            then_branch
        } else {
            else_branch
        }
        .to_string(),
    )
}

#[cfg(test)]
fn command_program(command: &str) -> Option<String> {
    // Best-effort shell token recognition for diagnostics only. Runtime command
    // execution still goes through the configured shell command unchanged.
    command_programs_for_shell(command).into_iter().next()
}

#[cfg(test)]
fn command_programs_for_shell(command: &str) -> Vec<String> {
    required_command_programs_for_shell(command)
        .programs
        .into_iter()
        .map(|program| program.program)
        .collect()
}

fn required_command_programs_for_shell(command: &str) -> RequiredCommandPrograms {
    let parsed = parse_shell_commands(command);
    let mut discovery = RequiredCommandPrograms {
        ambiguity: parsed
            .ambiguous
            .then_some(RequiredProgramAmbiguity::ShellSyntax),
        ..RequiredCommandPrograms::default()
    };
    let mut prior_path_lookup_is_unverifiable = false;
    let mut prior_cwd_is_unverifiable = false;
    let mut prior_dispatch_is_unverifiable = false;
    for words in &parsed.commands {
        let (programs, ambiguity) = required_command_programs_for_words(
            words,
            prior_path_lookup_is_unverifiable || prior_dispatch_is_unverifiable,
            prior_cwd_is_unverifiable,
        );
        discovery.programs.extend(programs);
        if discovery.ambiguity.is_none() {
            discovery.ambiguity = ambiguity;
        }
        prior_path_lookup_is_unverifiable |= shell_command_may_persist_path_change(words);
        prior_cwd_is_unverifiable |= shell_command_changes_directory(words);
        if shell_command_may_change_dispatch_or_inject_execution(words) {
            prior_dispatch_is_unverifiable = true;
            if discovery.ambiguity.is_none() {
                discovery.ambiguity = Some(RequiredProgramAmbiguity::ShellState);
            }
        }
    }
    if discovery.ambiguity == Some(RequiredProgramAmbiguity::ShellSyntax) {
        for program in &mut discovery.programs {
            program.path_lookup = ProgramPathLookup::Unverifiable;
        }
    }
    discovery
}

fn required_command_programs_for_words(
    words: &[ShellWord],
    prior_path_lookup_is_unverifiable: bool,
    prior_cwd_is_unverifiable: bool,
) -> (Vec<RequiredProgram>, Option<RequiredProgramAmbiguity>) {
    let command_name = shell_command_name(words);
    let external_wrappers = command_name.external_wrappers().to_vec();
    let (target, mut ambiguity) = match &command_name {
        ShellCommandName::Executable {
            index,
            ambiguous_wrapper,
            force_external,
            changes_cwd,
            path_lookup,
            allow_keyword,
            ..
        } => (
            Some((
                *index,
                *force_external,
                *allow_keyword,
                *path_lookup,
                *changes_cwd,
            )),
            ambiguous_wrapper.then_some(RequiredProgramAmbiguity::Wrapper),
        ),
        ShellCommandName::NoExternalExecutable { .. } => (None, None),
        ShellCommandName::AmbiguousWrapper { .. } => {
            (None, Some(RequiredProgramAmbiguity::Wrapper))
        }
    };

    let mut programs = external_wrappers
        .into_iter()
        .map(|wrapper| {
            required_program_for_index(
                words,
                wrapper.index,
                false,
                prior_path_lookup_is_unverifiable,
                wrapper.path_lookup,
                prior_cwd_is_unverifiable || wrapper.changes_cwd,
            )
        })
        .collect::<Vec<_>>();

    if let Some((index, force_external, allow_keyword, path_lookup, changes_cwd)) = target {
        if words[index].active_dollar || words[index].dynamic {
            ambiguity = Some(RequiredProgramAmbiguity::Wrapper);
        } else if !force_external
            && (bash_builtin(&shell_word_value(&words[index]))
                || allow_keyword && shell_word_is_keyword(&words[index]))
        {
            if matches!(
                shell_word_value(&words[index]).as_str(),
                "." | "eval" | "source"
            ) {
                ambiguity = Some(RequiredProgramAmbiguity::ShellSyntax);
            }
        } else {
            let cargo_sqlx_dispatch =
                executable_is_named(&shell_word_value(&words[index]), "cargo")
                    && cargo_subcommand(words, index + 1).as_deref() == Some("sqlx");
            programs.push(required_program_for_index(
                words,
                index,
                cargo_sqlx_dispatch,
                prior_path_lookup_is_unverifiable,
                path_lookup,
                prior_cwd_is_unverifiable || changes_cwd,
            ));
        }
    }

    (programs, ambiguity)
}

fn required_program_for_index(
    words: &[ShellWord],
    index: usize,
    cargo_sqlx_dispatch: bool,
    prior_path_lookup_is_unverifiable: bool,
    shell_path_lookup: ShellPathLookup,
    cwd_lookup_is_unverifiable: bool,
) -> RequiredProgram {
    let word = shell_word_value(&words[index]);
    let path_lookup = if program_has_explicit_path(&word) {
        if cwd_lookup_is_unverifiable && !Path::new(&word).is_absolute() {
            ProgramPathLookup::Unverifiable
        } else {
            ProgramPathLookup::Explicit
        }
    } else if prior_path_lookup_is_unverifiable {
        ProgramPathLookup::Unverifiable
    } else {
        match shell_path_lookup {
            ShellPathLookup::Captured if cwd_lookup_is_unverifiable => {
                ProgramPathLookup::CapturedAfterCwdChange
            }
            ShellPathLookup::Captured => ProgramPathLookup::Captured,
            ShellPathLookup::CommandLocal(index) => {
                let value = path_assignment_value(&words[index])
                    .expect("command-local PATH state must reference an assignment");
                if cwd_lookup_is_unverifiable
                    && !search_path_is_cwd_independent(Some(value.as_os_str()))
                {
                    ProgramPathLookup::Unverifiable
                } else {
                    ProgramPathLookup::CommandLocal(value)
                }
            }
            ShellPathLookup::Unverifiable => ProgramPathLookup::Unverifiable,
        }
    };
    RequiredProgram {
        cargo_sqlx_dispatch,
        program: word,
        path_lookup,
    }
}

fn command_uses_cargo_sqlx(command: &str) -> bool {
    parse_shell_commands(command).commands.iter().any(|words| {
        let Some(index) = command_program_index(words) else {
            return false;
        };
        executable_is_named(&shell_word_value(&words[index]), "cargo")
            && cargo_subcommand(words, index + 1).as_deref() == Some("sqlx")
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ShellCommandName {
    Executable {
        index: usize,
        ambiguous_wrapper: bool,
        force_external: bool,
        changes_cwd: bool,
        path_lookup: ShellPathLookup,
        allow_keyword: bool,
        external_wrappers: Vec<ExternalWrapperReference>,
    },
    NoExternalExecutable {
        external_wrappers: Vec<ExternalWrapperReference>,
        changes_cwd: bool,
    },
    AmbiguousWrapper {
        external_wrappers: Vec<ExternalWrapperReference>,
        changes_cwd: bool,
    },
}

impl ShellCommandName {
    fn external_wrappers(&self) -> &[ExternalWrapperReference] {
        match self {
            Self::Executable {
                external_wrappers, ..
            }
            | Self::NoExternalExecutable {
                external_wrappers, ..
            }
            | Self::AmbiguousWrapper {
                external_wrappers, ..
            } => external_wrappers,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExternalWrapperReference {
    index: usize,
    path_lookup: ShellPathLookup,
    changes_cwd: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellPathLookup {
    Captured,
    CommandLocal(usize),
    Unverifiable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellWrapperKind {
    Builtin,
    Command,
    Exec,
    Nohup,
    Time,
}

impl ShellWrapperKind {
    const fn is_external(self) -> bool {
        matches!(self, Self::Nohup | Self::Time)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WrapperTarget {
    Executable { index: usize, ambiguous: bool },
    NoExternalExecutable,
    Ambiguous,
}

fn command_program_index(words: &[ShellWord]) -> Option<usize> {
    let ShellCommandName::Executable {
        index,
        force_external,
        allow_keyword,
        ..
    } = shell_command_name(words)
    else {
        return None;
    };
    (force_external
        || (!bash_builtin(&shell_word_value(&words[index]))
            && (!allow_keyword || !shell_word_is_keyword(&words[index]))))
    .then_some(index)
}

fn shell_command_name(words: &[ShellWord]) -> ShellCommandName {
    let mut index = 0;
    let mut ambiguous_wrapper = false;
    let mut allow_shell_assignments = true;
    let mut allow_prefix_keyword = true;
    let mut force_external = false;
    let mut require_builtin = false;
    let mut changes_cwd = false;
    let mut path_lookup = ShellPathLookup::Captured;
    let mut taint_immediate_external_lookup = false;
    let mut taint_after_immediate_external_wrapper = false;
    let mut allow_keyword = true;
    let mut external_wrappers = Vec::new();
    while let Some(word) = words.get(index) {
        let word = shell_word_value(word);
        if word.is_empty() {
            // An empty quoted command name is a real shell word. Skipping it
            // could misidentify a later `cargo sqlx` argument as the program.
            return ShellCommandName::AmbiguousWrapper {
                external_wrappers,
                changes_cwd,
            };
        }
        let prefix_keyword = allow_shell_assignments
            && shell_word_is_prefix_keyword(&words[index], allow_prefix_keyword);
        let shell_assignment = allow_shell_assignments && shell_word_is_assignment(&words[index]);
        if prefix_keyword || shell_assignment {
            if shell_assignment {
                // Bash recognizes reserved prefix words before command
                // assignments, not after them. Once an assignment word starts
                // the simple command, a later reserved word is the command
                // name or a syntax error; it cannot expose a later executable.
                allow_prefix_keyword = false;
                if bash_assignment_name(&word).is_some_and(path_variable_name) {
                    path_lookup = if shell_path_assignment_is_literal(&words[index]) {
                        ShellPathLookup::CommandLocal(index)
                    } else {
                        ShellPathLookup::Unverifiable
                    };
                }
            }
            index += 1;
            continue;
        }
        if words[index].active_dollar || words[index].dynamic {
            return ShellCommandName::AmbiguousWrapper {
                external_wrappers,
                changes_cwd,
            };
        }
        if require_builtin && !bash_builtin(&word) {
            // `builtin` never falls back to PATH lookup. An unknown literal
            // target fails inside Bash without executing an external command.
            return ShellCommandName::NoExternalExecutable {
                external_wrappers,
                changes_cwd,
            };
        }
        if !allow_shell_assignments && looks_like_shell_assignment(&word) {
            // Assignment words are only recognized before the shell command
            // name. `command`, `exec`, and `nohup` treat an assignment-looking
            // target as the executable name; do not skip across it and expose
            // a later argument as a tool.
            return ShellCommandName::AmbiguousWrapper {
                external_wrappers,
                changes_cwd,
            };
        }
        let shell_builtin_dispatch = !force_external && bash_builtin(&word);
        let shell_keyword_dispatch =
            !force_external && allow_keyword && shell_word_is_keyword(&words[index]);
        let wrapper = if shell_builtin_dispatch {
            match word.as_str() {
                "builtin" => Some((
                    ShellWrapperKind::Builtin,
                    builtin_wrapper_target(words, index + 1),
                )),
                "command" => Some((
                    ShellWrapperKind::Command,
                    command_wrapper_target(words, index + 1),
                )),
                "exec" => Some((
                    ShellWrapperKind::Exec,
                    exec_wrapper_target(words, index + 1),
                )),
                _ => None,
            }
        } else if executable_is_named(&word, "nohup") {
            Some((
                ShellWrapperKind::Nohup,
                nohup_wrapper_target(words, index + 1),
            ))
        } else if !shell_keyword_dispatch && executable_is_named(&word, "time") {
            Some((
                ShellWrapperKind::Time,
                time_wrapper_target(words, index + 1),
            ))
        } else {
            None
        };
        if let Some((wrapper_kind, wrapper_target)) = wrapper {
            if wrapper_kind.is_external() {
                external_wrappers.push(ExternalWrapperReference {
                    index,
                    path_lookup: if taint_immediate_external_lookup {
                        ShellPathLookup::Unverifiable
                    } else {
                        path_lookup
                    },
                    changes_cwd,
                });
                taint_immediate_external_lookup = false;
                if taint_after_immediate_external_wrapper {
                    path_lookup = ShellPathLookup::Unverifiable;
                    taint_after_immediate_external_wrapper = false;
                }
            }
            match wrapper_target {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if wrapper_kind == ShellWrapperKind::Exec
                        && exec_wrapper_clears_environment(words, index + 1, target)
                    {
                        taint_after_immediate_external_wrapper = true;
                    }
                    taint_immediate_external_lookup |= ambiguous;
                    index = target;
                    ambiguous_wrapper |= ambiguous;
                    allow_shell_assignments = false;
                    match wrapper_kind {
                        ShellWrapperKind::Builtin => {
                            force_external = false;
                            require_builtin = true;
                            allow_keyword = false;
                        }
                        ShellWrapperKind::Command => {
                            force_external = false;
                            require_builtin = false;
                            allow_keyword = false;
                        }
                        ShellWrapperKind::Exec
                        | ShellWrapperKind::Nohup
                        | ShellWrapperKind::Time => {
                            force_external = true;
                            require_builtin = false;
                            allow_keyword = false;
                        }
                    }
                    continue;
                }
                WrapperTarget::NoExternalExecutable => {
                    return ShellCommandName::NoExternalExecutable {
                        external_wrappers,
                        changes_cwd,
                    };
                }
                WrapperTarget::Ambiguous => {
                    return ShellCommandName::AmbiguousWrapper {
                        external_wrappers,
                        changes_cwd,
                    };
                }
            }
        }
        if executable_is_named(&word, "env") && (!require_builtin || force_external) {
            external_wrappers.push(ExternalWrapperReference {
                index,
                path_lookup: if taint_immediate_external_lookup {
                    ShellPathLookup::Unverifiable
                } else {
                    path_lookup
                },
                changes_cwd,
            });
            taint_immediate_external_lookup = false;
            if taint_after_immediate_external_wrapper {
                path_lookup = ShellPathLookup::Unverifiable;
                taint_after_immediate_external_wrapper = false;
            }
            let env = parse_env_wrapper(words, index + 1);
            changes_cwd |= env.changes_directory;
            path_lookup = env_wrapper_path_lookup(words, &env, path_lookup);
            match env.target_index {
                Some(target) => {
                    index = target;
                    ambiguous_wrapper |= env.ambiguous;
                    allow_shell_assignments = false;
                    force_external = true;
                    require_builtin = false;
                    allow_keyword = false;
                    continue;
                }
                None if env.ambiguous => {
                    return ShellCommandName::AmbiguousWrapper {
                        external_wrappers,
                        changes_cwd,
                    };
                }
                None => {
                    return ShellCommandName::NoExternalExecutable {
                        external_wrappers,
                        changes_cwd,
                    };
                }
            }
        }
        return ShellCommandName::Executable {
            index,
            ambiguous_wrapper,
            force_external,
            changes_cwd,
            path_lookup: if taint_immediate_external_lookup {
                ShellPathLookup::Unverifiable
            } else {
                path_lookup
            },
            allow_keyword,
            external_wrappers,
        };
    }
    ShellCommandName::NoExternalExecutable {
        external_wrappers,
        changes_cwd,
    }
}

fn shell_command_has_ambiguous_wrapper(words: &[ShellWord]) -> bool {
    matches!(
        shell_command_name(words),
        ShellCommandName::Executable {
            ambiguous_wrapper: true,
            ..
        } | ShellCommandName::AmbiguousWrapper { .. }
    )
}

fn builtin_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    if words.get(index).map(shell_word_value).as_deref() == Some("--") {
        index += 1;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            ambiguous: false,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn command_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    let mut uses_default_path = false;
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            index += 1;
            break;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            break;
        };
        if !options
            .chars()
            .all(|option| matches!(option, 'p' | 'v' | 'V'))
        {
            return WrapperTarget::Ambiguous;
        }
        if options.chars().any(|option| matches!(option, 'v' | 'V')) {
            return WrapperTarget::NoExternalExecutable;
        }
        uses_default_path |= options.contains('p');
        index += 1;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            // `command -p` uses an implementation-defined default search
            // path, not the captured PATH doctor can resolve faithfully.
            ambiguous: uses_default_path,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn exec_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    let mut changes_argv_zero = false;
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            index += 1;
            break;
        }
        if word == "-a" {
            if words.get(index + 1).is_none() {
                return WrapperTarget::Ambiguous;
            }
            changes_argv_zero = true;
            index += 2;
            continue;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            break;
        };
        if !options.chars().all(|option| matches!(option, 'c' | 'l')) {
            return WrapperTarget::Ambiguous;
        }
        changes_argv_zero |= options.contains('l');
        index += 1;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            // `-a` and `-l` alter argv[0], which the capability probe does
            // not reproduce portably.
            ambiguous: changes_argv_zero,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn nohup_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    if words.get(index).map(shell_word_value).as_deref() == Some("--") {
        index += 1;
        return words
            .get(index)
            .map(|_| WrapperTarget::Executable {
                index,
                ambiguous: false,
            })
            .unwrap_or(WrapperTarget::NoExternalExecutable);
    }
    match words.get(index).map(shell_word_value) {
        Some(word) if matches!(word.as_str(), "--help" | "--version") => {
            WrapperTarget::NoExternalExecutable
        }
        Some(word) if word.starts_with('-') => WrapperTarget::Ambiguous,
        Some(_) => WrapperTarget::Executable {
            index,
            ambiguous: false,
        },
        None => WrapperTarget::NoExternalExecutable,
    }
}

fn time_wrapper_target(words: &[ShellWord], mut index: usize) -> WrapperTarget {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            index += 1;
            break;
        }
        if matches!(word.as_str(), "--help" | "--version") {
            return WrapperTarget::NoExternalExecutable;
        }
        if word == "-p" {
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            return WrapperTarget::Ambiguous;
        }
        break;
    }
    words
        .get(index)
        .map(|_| WrapperTarget::Executable {
            index,
            ambiguous: false,
        })
        .unwrap_or(WrapperTarget::NoExternalExecutable)
}

fn cargo_subcommand(words: &[ShellWord], index: usize) -> Option<String> {
    let index = cargo_subcommand_index(words, index)?;
    Some(shell_word_value(&words[index]))
}

fn cargo_subcommand_index(words: &[ShellWord], mut index: usize) -> Option<usize> {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word.starts_with('+') {
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            index += if cargo_global_option_takes_value(&word) {
                2
            } else {
                1
            };
            continue;
        }
        return Some(index);
    }
    None
}

fn cargo_global_option_takes_value(option: &str) -> bool {
    matches!(option, "--color" | "--config" | "-C" | "-Z")
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EnvWrapperParse {
    target_index: Option<usize>,
    ambiguous: bool,
    clears_environment: bool,
    unset_names: Vec<String>,
    assignment_indices: Vec<usize>,
    changes_directory: bool,
    uses_alternate_path: bool,
    null_output: bool,
}

fn parse_env_wrapper(words: &[ShellWord], mut index: usize) -> EnvWrapperParse {
    let mut parsed = EnvWrapperParse::default();
    let mut options_allowed = true;

    while let Some(shell_word) = words.get(index) {
        let word = shell_word_value(shell_word);
        if options_allowed && (shell_word.active_dollar || shell_word.dynamic) {
            parsed.ambiguous = true;
            return parsed;
        }

        if options_allowed {
            if word == "--" {
                options_allowed = false;
                index += 1;
                continue;
            }
            if word == "-" {
                parsed.clears_environment = true;
                index += 1;
                continue;
            }
            if let Some(long) = word.strip_prefix("--") {
                match long {
                    "ignore-environment" => parsed.clears_environment = true,
                    "unset" => {
                        let Some(name) = words.get(index + 1) else {
                            parsed.ambiguous = true;
                            return parsed;
                        };
                        if name.active_dollar || name.dynamic {
                            parsed.ambiguous = true;
                        }
                        parsed.unset_names.push(shell_word_value(name));
                        index += 2;
                        continue;
                    }
                    "chdir" => {
                        if words.get(index + 1).is_none() {
                            parsed.ambiguous = true;
                            return parsed;
                        }
                        parsed.changes_directory = true;
                        index += 2;
                        continue;
                    }
                    "split-string" => {
                        parsed.ambiguous = true;
                        return parsed;
                    }
                    "argv0" => {
                        if words.get(index + 1).is_none() {
                            parsed.ambiguous = true;
                            return parsed;
                        }
                        parsed.ambiguous = true;
                        index += 2;
                        continue;
                    }
                    "help" | "version" => return parsed,
                    "null" => parsed.null_output = true,
                    "debug" => {}
                    _ if long.starts_with("unset=") => {
                        parsed
                            .unset_names
                            .push(long.trim_start_matches("unset=").to_string());
                    }
                    _ if long.starts_with("chdir=") => parsed.changes_directory = true,
                    _ if long.starts_with("split-string=") => {
                        parsed.ambiguous = true;
                        return parsed;
                    }
                    _ if long.starts_with("argv0=") => parsed.ambiguous = true,
                    _ => {
                        parsed.ambiguous = true;
                        return parsed;
                    }
                }
                index += 1;
                continue;
            }
            if let Some(short) = word.strip_prefix('-').filter(|short| !short.is_empty()) {
                let options = short.char_indices();
                let mut consumed_next = false;
                for (offset, option) in options {
                    match option {
                        '0' | 'i' | 'v' => {
                            if option == '0' {
                                parsed.null_output = true;
                            }
                            if option == 'i' {
                                parsed.clears_environment = true;
                            }
                        }
                        'S' => {
                            // Split-string recursively creates new env options,
                            // assignments, and the utility itself. Its quoting,
                            // escapes, comments, and substitution are deliberately
                            // not reimplemented here.
                            parsed.ambiguous = true;
                            return parsed;
                        }
                        'u' | 'C' | 'P' | 'a' => {
                            let value_offset = offset + option.len_utf8();
                            let attached = &short[value_offset..];
                            let value = if attached.is_empty() {
                                let Some(value) = words.get(index + 1) else {
                                    parsed.ambiguous = true;
                                    return parsed;
                                };
                                consumed_next = true;
                                shell_word_value(value)
                            } else {
                                attached.to_string()
                            };
                            match option {
                                'u' => parsed.unset_names.push(value),
                                'C' => parsed.changes_directory = true,
                                'P' => {
                                    parsed.uses_alternate_path = true;
                                    parsed.ambiguous = true;
                                }
                                'a' => parsed.ambiguous = true,
                                _ => unreachable!(),
                            }
                            break;
                        }
                        _ => {
                            parsed.ambiguous = true;
                            return parsed;
                        }
                    }
                }
                index += usize::from(consumed_next) + 1;
                continue;
            }
        }

        if env_assignment_name(&word).is_some() {
            options_allowed = false;
            parsed.assignment_indices.push(index);
            index += 1;
            continue;
        }
        if shell_word.active_dollar || shell_word.dynamic {
            parsed.ambiguous = true;
            return parsed;
        }
        if parsed.null_output {
            // GNU and BSD `env` reserve null-delimited output for printing the
            // environment; combining it with a utility is an error and never
            // executes that utility.
            parsed.ambiguous = true;
            return parsed;
        }
        parsed.target_index = Some(index);
        return parsed;
    }

    parsed
}

fn env_wrapper_path_lookup(
    words: &[ShellWord],
    env: &EnvWrapperParse,
    incoming: ShellPathLookup,
) -> ShellPathLookup {
    if env.uses_alternate_path {
        return ShellPathLookup::Unverifiable;
    }
    let mut lookup =
        if env.clears_environment || env.unset_names.iter().any(|name| path_variable_name(name)) {
            ShellPathLookup::Unverifiable
        } else {
            incoming
        };
    for index in &env.assignment_indices {
        let word = shell_word_value(&words[*index]);
        if env_assignment_name(&word).is_some_and(path_variable_name) {
            lookup = if path_assignment_is_literal(&words[*index]) {
                ShellPathLookup::CommandLocal(*index)
            } else {
                ShellPathLookup::Unverifiable
            };
        }
    }
    lookup
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SqlxInvocation {
    args_index: usize,
    no_dotenv: bool,
    has_ambiguous_cwd_option: bool,
}

fn sqlx_invocation(words: &[ShellWord], program_index: usize) -> Option<SqlxInvocation> {
    let program = shell_word_value(&words[program_index]);
    let basename = executable_basename(&program)?;
    let args_index = if basename.eq_ignore_ascii_case("cargo") {
        let sqlx_index = cargo_subcommand_index(words, program_index + 1)?;
        (shell_word_value(&words[sqlx_index]) == "sqlx").then_some(sqlx_index + 1)?
    } else if basename.eq_ignore_ascii_case("sqlx") {
        program_index + 1
    } else if basename.eq_ignore_ascii_case("cargo-sqlx") {
        let sqlx_index = program_index + 1;
        (words.get(sqlx_index).map(shell_word_value).as_deref() == Some("sqlx"))
            .then_some(sqlx_index + 1)?
    } else {
        return None;
    };
    let no_dotenv = words[args_index..]
        .iter()
        .take_while(|word| shell_word_value(word) != "--")
        .any(|word| shell_word_value(word) == "--no-dotenv");
    let prefix = &words[..args_index];
    let has_ambiguous_cwd_option = prefix.iter().any(|word| {
        let word = shell_word_value(word);
        matches!(word.as_str(), "-C" | "--chdir")
            || (word.starts_with("-C") && word.len() > 2)
            || word.starts_with("--chdir=")
    });
    Some(SqlxInvocation {
        args_index,
        no_dotenv,
        has_ambiguous_cwd_option,
    })
}

fn executable_basename(program: &str) -> Option<&str> {
    let basename = Path::new(program).file_name()?.to_str()?;
    if let Some(suffix_start) = basename.len().checked_sub(4) {
        if basename
            .get(suffix_start..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
        {
            return basename.get(..suffix_start);
        }
    }
    Some(basename)
}

fn executable_is_named(program: &str, expected: &str) -> bool {
    executable_basename(program).is_some_and(|basename| basename.eq_ignore_ascii_case(expected))
}

fn sqlx_probe_style(program: &str) -> Option<SqlxProbeStyle> {
    let basename = executable_basename(program)?;
    if basename.eq_ignore_ascii_case("cargo-sqlx") {
        Some(SqlxProbeStyle::CargoSubcommand)
    } else if basename.eq_ignore_ascii_case("sqlx") {
        Some(SqlxProbeStyle::Direct)
    } else {
        None
    }
}

fn sqlx_driver_from_flag(
    words: &[ShellWord],
    mut index: usize,
    ambient_database_url: Option<&OsStr>,
) -> Option<SqlxDriverResolution> {
    let mut resolution = None;
    while let Some(word) = words.get(index) {
        let value = shell_word_value(word);
        if value == "--" {
            break;
        }
        let database_url = if matches!(value.as_str(), "--database-url" | "-D") {
            index += 1;
            let Some(value) = words.get(index) else {
                return Some(SqlxDriverResolution::Indeterminate(
                    "--database-url is missing its value",
                ));
            };
            Some(value.clone())
        } else if let Some(value) = value.strip_prefix("--database-url=") {
            Some(word.with_value(value))
        } else if let Some(value) = value.strip_prefix("-D=") {
            Some(word.with_value(value))
        } else if let Some(value) = value.strip_prefix("-D") {
            (!value.is_empty()).then(|| word.with_value(value))
        } else {
            None
        };
        if let Some(database_url) = database_url {
            if resolution.is_some() {
                return Some(SqlxDriverResolution::Indeterminate(
                    "the SQLx command contains multiple --database-url options",
                ));
            }
            resolution = Some(sqlx_driver_from_command_value(
                &database_url,
                SqlxDriverSource::CommandFlag,
                ambient_database_url,
            ));
        }
        index += 1;
    }
    resolution
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandDatabaseUrlScope {
    Inherited,
    Assigned(ShellWord),
    Removed,
    Ambiguous,
}

fn command_database_url_scope(
    words: &[ShellWord],
    program_index: usize,
) -> CommandDatabaseUrlScope {
    let mut scope = CommandDatabaseUrlScope::Inherited;
    let mut index = 0;
    let mut allow_prefix_keyword = true;

    while index < program_index {
        let Some(word) = words.get(index) else {
            break;
        };
        if shell_word_is_assignment(word) {
            allow_prefix_keyword = false;
            apply_database_url_assignment(&mut scope, word, true);
        } else if !shell_word_is_prefix_keyword(word, allow_prefix_keyword) {
            break;
        }
        index += 1;
    }

    while index < program_index {
        let word = shell_word_value(&words[index]);
        match word.as_str() {
            "builtin" => match builtin_wrapper_target(words, index + 1) {
                WrapperTarget::Executable { index: target, .. }
                    if bash_builtin(&shell_word_value(&words[target])) =>
                {
                    index = target;
                }
                WrapperTarget::Executable { .. }
                | WrapperTarget::NoExternalExecutable
                | WrapperTarget::Ambiguous => return CommandDatabaseUrlScope::Ambiguous,
            },
            "command" => match command_wrapper_target(words, index + 1) {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if ambiguous {
                        scope = CommandDatabaseUrlScope::Ambiguous;
                    }
                    index = target;
                }
                WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                    return CommandDatabaseUrlScope::Ambiguous;
                }
            },
            "exec" => match exec_wrapper_target(words, index + 1) {
                WrapperTarget::Executable {
                    index: target,
                    ambiguous,
                } => {
                    if exec_wrapper_clears_environment(words, index + 1, target) {
                        scope = CommandDatabaseUrlScope::Removed;
                    }
                    if ambiguous {
                        scope = CommandDatabaseUrlScope::Ambiguous;
                    }
                    index = target;
                }
                WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                    return CommandDatabaseUrlScope::Ambiguous;
                }
            },
            _ if executable_is_named(&word, "nohup") => {
                match nohup_wrapper_target(words, index + 1) {
                    WrapperTarget::Executable {
                        index: target,
                        ambiguous,
                    } => {
                        if ambiguous {
                            scope = CommandDatabaseUrlScope::Ambiguous;
                        }
                        index = target;
                    }
                    WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                        return CommandDatabaseUrlScope::Ambiguous;
                    }
                }
            }
            _ if executable_is_named(&word, "time") => {
                match time_wrapper_target(words, index + 1) {
                    WrapperTarget::Executable {
                        index: target,
                        ambiguous,
                    } => {
                        if ambiguous {
                            scope = CommandDatabaseUrlScope::Ambiguous;
                        }
                        index = target;
                    }
                    WrapperTarget::NoExternalExecutable | WrapperTarget::Ambiguous => {
                        return CommandDatabaseUrlScope::Ambiguous;
                    }
                }
            }
            _ if executable_is_named(&word, "env") => {
                let env = parse_env_wrapper(words, index + 1);
                if env.ambiguous {
                    scope = CommandDatabaseUrlScope::Ambiguous;
                }
                if env.clears_environment
                    || env.unset_names.iter().any(|name| database_url_name(name))
                {
                    scope = CommandDatabaseUrlScope::Removed;
                }
                for assignment_index in env.assignment_indices {
                    apply_database_url_assignment(&mut scope, &words[assignment_index], false);
                }
                let Some(target) = env.target_index else {
                    return if env.ambiguous {
                        CommandDatabaseUrlScope::Ambiguous
                    } else {
                        scope
                    };
                };
                index = target;
            }
            _ => break,
        }
    }

    scope
}

fn apply_database_url_assignment(
    scope: &mut CommandDatabaseUrlScope,
    word: &ShellWord,
    require_plain_name: bool,
) {
    if require_plain_name && !word.assignment_name_plain {
        return;
    }
    let value = shell_word_value(word);
    let Some((raw_name, value)) = value.split_once('=') else {
        return;
    };
    let append = require_plain_name && raw_name.ends_with('+');
    let name = if require_plain_name {
        bash_assignment_base_name(raw_name).unwrap_or(raw_name)
    } else {
        raw_name
    };
    if database_url_name(name) {
        *scope = if append || require_plain_name && raw_name.contains('[') {
            CommandDatabaseUrlScope::Ambiguous
        } else {
            CommandDatabaseUrlScope::Assigned(word.with_value(value))
        };
    }
}

fn database_url_name(name: &str) -> bool {
    #[cfg(windows)]
    {
        name.eq_ignore_ascii_case("DATABASE_URL")
    }
    #[cfg(not(windows))]
    {
        name == "DATABASE_URL"
    }
}

fn exec_wrapper_clears_environment(
    words: &[ShellWord],
    mut index: usize,
    program_index: usize,
) -> bool {
    while index < program_index {
        let word = shell_word_value(&words[index]);
        if word == "--" {
            return false;
        }
        if word == "-a" {
            index += 2;
            continue;
        }
        let Some(options) = word.strip_prefix('-').filter(|options| !options.is_empty()) else {
            return false;
        };
        if !options.chars().all(|option| matches!(option, 'c' | 'l')) {
            return false;
        }
        if options.contains('c') {
            return true;
        }
        index += 1;
    }
    false
}

fn sqlx_driver_from_command_value(
    value: &ShellWord,
    source: SqlxDriverSource,
    ambient_database_url: Option<&OsStr>,
) -> SqlxDriverResolution {
    let text = value.value.trim();
    if matches!(text, "$DATABASE_URL" | "${DATABASE_URL}") {
        if value.literal_dollar || !value.active_dollar {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is quoted or escaped literally",
            );
        }
        if source == SqlxDriverSource::CommandFlag && value.syntactically_plain {
            return SqlxDriverResolution::Indeterminate(
                "an unquoted DATABASE_URL command option can split or expand into multiple arguments",
            );
        }
        let Some(database_url) = ambient_database_url else {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is unset",
            );
        };
        let Some(database_url) = database_url.to_str() else {
            return SqlxDriverResolution::Indeterminate(
                "an explicit DATABASE_URL reference is not valid UTF-8",
            );
        };
        return sqlx_driver_from_literal(database_url, source);
    }
    if text.is_empty()
        || value.active_dollar
        || value.literal_dollar
        || value.dynamic
        || text.contains('`')
        || text.contains("$(")
    {
        return SqlxDriverResolution::Indeterminate(
            "an explicit database URL is empty or dynamically expanded",
        );
    }
    sqlx_driver_from_literal(text, source)
}

fn sqlx_driver_from_literal(value: &str, source: SqlxDriverSource) -> SqlxDriverResolution {
    SqlxDriver::from_database_url(value)
        .map(|driver| SqlxDriverResolution::Known(SqlxDriverRequirement { driver, source }))
        .unwrap_or(SqlxDriverResolution::Indeterminate(
            "DATABASE_URL does not identify a supported SQLx driver",
        ))
}

fn shell_command_mutates_database_url(words: &[ShellWord]) -> bool {
    shell_command_may_persist_variable_change(words, &database_url_name)
}

fn shell_command_may_persist_path_change(words: &[ShellWord]) -> bool {
    shell_command_may_persist_variable_change(words, &path_variable_name)
}

fn shell_command_may_change_dispatch_or_inject_execution(words: &[ShellWord]) -> bool {
    let ShellCommandName::Executable {
        index,
        force_external: false,
        ..
    } = shell_command_name(words)
    else {
        return false;
    };
    let command = shell_word_value(&words[index]);
    let arguments = &words[index + 1..];
    if arguments
        .iter()
        .any(|word| word.active_dollar || word.dynamic)
    {
        return matches!(command.as_str(), "hash" | "enable" | "trap");
    }
    match command.as_str() {
        "hash" => hash_command_may_change_dispatch(arguments),
        "enable" => enable_command_may_change_dispatch(arguments),
        "trap" => trap_command_may_inject_execution(arguments),
        _ => false,
    }
}

fn hash_command_may_change_dispatch(arguments: &[ShellWord]) -> bool {
    let mut query_or_delete_only = false;
    for argument in arguments {
        let value = shell_word_value(argument);
        if value == "--" {
            query_or_delete_only = false;
            continue;
        }
        if let Some(options) = value
            .strip_prefix('-')
            .filter(|options| !options.is_empty())
        {
            if options.contains('p') {
                return true;
            }
            if !options
                .chars()
                .all(|option| matches!(option, 'd' | 'l' | 'r' | 't'))
            {
                return true;
            }
            query_or_delete_only = options
                .chars()
                .any(|option| matches!(option, 'd' | 'l' | 't'));
            continue;
        }
        if !query_or_delete_only {
            return true;
        }
    }
    false
}

fn enable_command_may_change_dispatch(arguments: &[ShellWord]) -> bool {
    for argument in arguments {
        let value = shell_word_value(argument);
        if value == "--" {
            continue;
        }
        if let Some(options) = value
            .strip_prefix('-')
            .filter(|options| !options.is_empty())
        {
            if options
                .chars()
                .any(|option| matches!(option, 'd' | 'f' | 'n'))
            {
                return true;
            }
            if !options
                .chars()
                .all(|option| matches!(option, 'a' | 'p' | 's'))
            {
                return true;
            }
            continue;
        }
        return true;
    }
    false
}

fn trap_command_may_inject_execution(arguments: &[ShellWord]) -> bool {
    let Some(first) = arguments.first().map(shell_word_value) else {
        return false;
    };
    !matches!(first.as_str(), "-l" | "-p")
}

fn shell_command_may_persist_variable_change(
    words: &[ShellWord],
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut leading_assignment_end = 0;
    while words
        .get(leading_assignment_end)
        .is_some_and(shell_word_is_assignment)
    {
        leading_assignment_end += 1;
    }
    let leading_variable_assignment = words[..leading_assignment_end]
        .iter()
        .any(|word| bash_assignment_name(&shell_word_value(word)).is_some_and(variable_matches));
    if leading_assignment_end == words.len() {
        return leading_variable_assignment;
    }

    let index = match shell_command_name(words) {
        ShellCommandName::Executable {
            index,
            force_external: false,
            ..
        } => index,
        ShellCommandName::Executable {
            force_external: true,
            ..
        } => return false,
        ShellCommandName::AmbiguousWrapper { .. } => return true,
        ShellCommandName::NoExternalExecutable { .. } => return false,
    };
    let command = shell_word_value(&words[index]);
    if matches!(command.as_str(), "." | "source" | "eval") {
        return true;
    }
    if matches!(
        command.as_str(),
        "declare" | "export" | "local" | "readonly" | "typeset" | "unset"
    ) {
        return leading_variable_assignment
            || declaration_uses_nameref(words, index + 1)
            || words
                .iter()
                .skip(index + 1)
                .any(|word| shell_word_names_variable(word, variable_matches));
    }
    if command == "read" {
        return leading_variable_assignment
            || read_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "printf" {
        return leading_variable_assignment
            || printf_mutates_variable(words, index + 1, variable_matches);
    }
    if matches!(command.as_str(), "mapfile" | "readarray") {
        return leading_variable_assignment
            || mapfile_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "getopts" {
        return leading_variable_assignment
            || getopts_mutates_variable(words, index + 1, variable_matches);
    }
    if command == "let" {
        // Arithmetic expressions can assign through array subscripts and
        // namerefs. Reimplementing Bash arithmetic expansion here would risk
        // a false-negative driver or PATH result, so treat it conservatively.
        return true;
    }
    false
}

fn declaration_uses_nameref(words: &[ShellWord], mut index: usize) -> bool {
    while let Some(word) = words.get(index).map(shell_word_value) {
        if word == "--" {
            return false;
        }
        let Some(options) = word
            .strip_prefix('-')
            .or_else(|| word.strip_prefix('+'))
            .filter(|options| !options.is_empty())
        else {
            return false;
        };
        if options.contains('n') {
            return true;
        }
        index += 1;
    }
    false
}

fn shell_word_names_variable(word: &ShellWord, variable_matches: &dyn Fn(&str) -> bool) -> bool {
    if word.active_dollar || word.dynamic {
        return true;
    }
    let word = shell_word_value(word);
    shell_variable_base_name(&word).is_some_and(variable_matches)
}

fn shell_variable_base_name(value: &str) -> Option<&str> {
    let candidate = value.split_once('=').map_or(value, |(name, _)| name);
    bash_assignment_base_name(candidate)
}

fn bash_assignment_base_name(candidate: &str) -> Option<&str> {
    let candidate = candidate.strip_suffix('+').unwrap_or(candidate);
    let candidate = candidate
        .split_once('[')
        .map_or(candidate, |(name, _)| name);
    let mut chars = candidate.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
    .then_some(candidate)
}

fn read_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut options_allowed = true;
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if options_allowed && value == "--" {
            options_allowed = false;
            index += 1;
            continue;
        }
        if options_allowed {
            if let Some(options) = value.strip_prefix('-').filter(|value| !value.is_empty()) {
                let chars = options.char_indices();
                let mut consumed_next = false;
                for (offset, option) in chars {
                    match option {
                        'e' | 'r' | 's' => {}
                        'a' | 'd' | 'i' | 'n' | 'N' | 'p' | 't' | 'u' => {
                            let value_offset = offset + option.len_utf8();
                            let attached = &options[value_offset..];
                            let argument = if attached.is_empty() {
                                let Some(argument) = words.get(index + 1) else {
                                    return true;
                                };
                                consumed_next = true;
                                argument
                            } else {
                                word
                            };
                            if option == 'a' {
                                if attached.is_empty() {
                                    if shell_word_names_variable(argument, variable_matches) {
                                        return true;
                                    }
                                } else if bash_assignment_base_name(attached)
                                    .is_some_and(variable_matches)
                                {
                                    return true;
                                }
                            }
                            break;
                        }
                        _ => return true,
                    }
                }
                index += usize::from(consumed_next) + 1;
                continue;
            }
        }
        options_allowed = false;
        if shell_word_names_variable(word, variable_matches) {
            return true;
        }
        index += 1;
    }
    false
}

fn printf_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if value == "--" {
            return false;
        }
        if value == "-v" {
            let Some(variable) = words.get(index + 1) else {
                return true;
            };
            return shell_word_names_variable(variable, variable_matches);
        }
        if let Some(variable) = value.strip_prefix("-v").filter(|value| !value.is_empty()) {
            return bash_assignment_base_name(variable).is_some_and(variable_matches);
        }
        if value.starts_with('-') {
            index += 1;
            continue;
        }
        return false;
    }
    false
}

fn mapfile_mutates_variable(
    words: &[ShellWord],
    mut index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut options_allowed = true;
    while let Some(word) = words.get(index) {
        if word.active_dollar || word.dynamic {
            return true;
        }
        let value = shell_word_value(word);
        if options_allowed && value == "--" {
            options_allowed = false;
            index += 1;
            continue;
        }
        if options_allowed {
            if let Some(options) = value.strip_prefix('-').filter(|value| !value.is_empty()) {
                let chars = options.char_indices();
                let mut consumed_next = false;
                for (offset, option) in chars {
                    match option {
                        't' => {}
                        'C' => {
                            // The callback is evaluated as Bash code and can
                            // mutate arbitrary variables through namerefs.
                            return true;
                        }
                        'c' | 'd' | 'n' | 'O' | 's' | 'u' => {
                            let value_offset = offset + option.len_utf8();
                            if options[value_offset..].is_empty() {
                                if words.get(index + 1).is_none() {
                                    return true;
                                }
                                consumed_next = true;
                            }
                            break;
                        }
                        _ => return true,
                    }
                }
                index += usize::from(consumed_next) + 1;
                continue;
            }
        }
        return shell_word_names_variable(word, variable_matches);
    }
    false
}

fn getopts_mutates_variable(
    words: &[ShellWord],
    index: usize,
    variable_matches: &dyn Fn(&str) -> bool,
) -> bool {
    let Some(optstring) = words.get(index) else {
        return false;
    };
    if optstring.active_dollar || optstring.dynamic {
        return true;
    }
    let Some(name) = words.get(index + 1) else {
        return false;
    };
    shell_word_names_variable(name, variable_matches)
}

fn path_variable_name(name: &str) -> bool {
    #[cfg(windows)]
    {
        name.eq_ignore_ascii_case("PATH")
    }
    #[cfg(not(windows))]
    {
        name == "PATH"
    }
}

fn env_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    (!name.is_empty()).then_some(name)
}

fn shell_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut chars = name.chars();
    let first = chars.next()?;
    ((first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
    .then_some(name)
}

fn bash_assignment_name(value: &str) -> Option<&str> {
    let (name, _) = value.split_once('=')?;
    bash_assignment_base_name(name)
}

fn shell_word_is_assignment(word: &ShellWord) -> bool {
    word.assignment_name_plain && bash_assignment_name(&word.value).is_some()
}

fn shell_path_assignment_is_literal(word: &ShellWord) -> bool {
    let Some((name, _)) = word.value.split_once('=') else {
        return false;
    };
    !name.ends_with('+')
        && !name.contains('[')
        && bash_assignment_base_name(name).is_some_and(path_variable_name)
        && path_assignment_is_literal(word)
}

fn path_assignment_is_literal(word: &ShellWord) -> bool {
    !word.active_dollar
        && !word.dynamic
        && path_assignment_value(word).is_some_and(|value| {
            env::split_paths(&value)
                .all(|entry| !entry.to_str().is_some_and(|entry| entry.starts_with('~')))
        })
}

fn path_assignment_value(word: &ShellWord) -> Option<OsString> {
    word.value
        .split_once('=')
        .map(|(_, value)| OsString::from(value))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ShellWord {
    value: String,
    syntactically_plain: bool,
    assignment_name_plain: bool,
    active_dollar: bool,
    literal_dollar: bool,
    dynamic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShellWordOrigin {
    syntactically_plain: bool,
    assignment_name_plain: bool,
}

impl Default for ShellWordOrigin {
    fn default() -> Self {
        Self {
            syntactically_plain: true,
            assignment_name_plain: true,
        }
    }
}

impl ShellWord {
    fn with_value(&self, value: &str) -> Self {
        Self {
            value: value.to_string(),
            syntactically_plain: self.syntactically_plain,
            assignment_name_plain: self.assignment_name_plain,
            active_dollar: self.active_dollar,
            literal_dollar: self.literal_dollar,
            dynamic: self.dynamic,
        }
    }
}

fn shell_word_value(word: &ShellWord) -> String {
    word.value.clone()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellSeparator {
    And,
    Or,
    Sequence,
    Pipe,
    Background,
    Group,
}

#[derive(Debug)]
struct ShellParse {
    commands: Vec<Vec<ShellWord>>,
    separators: Vec<ShellSeparator>,
    ambiguous: bool,
}

fn parse_shell_commands(command: &str) -> ShellParse {
    let (command, heredoc_ambiguous) = strip_heredoc_bodies(command);
    let mut lexed = shell_tokens(&command);
    lexed.ambiguous |= heredoc_ambiguous;
    let mut commands = Vec::new();
    let mut separators = Vec::new();
    let mut current = Vec::new();
    let mut skip_next_word = false;

    for token in lexed.tokens {
        match token {
            ShellToken::Word(word) => {
                if skip_next_word {
                    skip_next_word = false;
                } else {
                    current.push(word);
                }
            }
            ShellToken::Redirection(redirection) => {
                skip_next_word = !redirection_has_inline_target(&redirection);
            }
            ShellToken::Separator(separator) => {
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                    separators.push(separator);
                }
                skip_next_word = false;
            }
        }
    }

    if !current.is_empty() {
        commands.push(current);
    }

    let uses_control_flow = commands.iter().any(|words| {
        let uses_non_time_control_flow = words.iter().any(|word| {
            word.syntactically_plain
                && matches!(
                    shell_word_value(word).as_str(),
                    "[[" | "]]"
                        | "case"
                        | "coproc"
                        | "do"
                        | "done"
                        | "elif"
                        | "else"
                        | "esac"
                        | "fi"
                        | "for"
                        | "function"
                        | "if"
                        | "in"
                        | "select"
                        | "then"
                        | "until"
                        | "while"
                        | "{"
                        | "}"
                )
        });
        let uses_time_keyword = matches!(
            shell_command_name(words),
            ShellCommandName::Executable {
                index,
                force_external: false,
                allow_keyword: true,
                ..
            } if shell_word_value(&words[index]) == "time" && shell_word_is_keyword(&words[index])
        );
        uses_non_time_control_flow || uses_time_keyword
    });
    ShellParse {
        commands,
        separators,
        ambiguous: lexed.ambiguous || uses_control_flow,
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ShellToken {
    Word(ShellWord),
    Separator(ShellSeparator),
    Redirection(String),
}

#[derive(Debug)]
struct ShellLex {
    tokens: Vec<ShellToken>,
    ambiguous: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct HeredocSpec {
    delimiter: String,
    strip_tabs: bool,
    expands_body: bool,
}

fn strip_heredoc_bodies(command: &str) -> (String, bool) {
    let mut rendered = String::with_capacity(command.len());
    let mut pending: VecDeque<HeredocSpec> = VecDeque::new();
    let mut ambiguous = false;

    for line_with_ending in command.split_inclusive('\n') {
        let line = line_with_ending
            .strip_suffix('\n')
            .unwrap_or(line_with_ending)
            .strip_suffix('\r')
            .unwrap_or_else(|| {
                line_with_ending
                    .strip_suffix('\n')
                    .unwrap_or(line_with_ending)
            });
        if let Some(spec) = pending.front() {
            let candidate = if spec.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if candidate == spec.delimiter {
                pending.pop_front();
            } else if spec.expands_body && heredoc_body_has_active_command_substitution(line) {
                ambiguous = true;
            }
            continue;
        }

        rendered.push_str(line_with_ending);
        let (specs, line_ambiguous) = heredoc_specs_on_line(line);
        pending.extend(specs);
        ambiguous |= line_ambiguous;
    }

    ambiguous |= !pending.is_empty();
    (rendered, ambiguous)
}

fn heredoc_specs_on_line(line: &str) -> (Vec<HeredocSpec>, bool) {
    let chars = line.chars().collect::<Vec<_>>();
    let mut specs = Vec::new();
    let mut ambiguous = false;
    let mut quote = None;
    let mut at_word_start = true;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                index += 1;
            }
            index += 1;
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                at_word_start = false;
                index += 1;
            }
            '\\' => {
                at_word_start = false;
                index = (index + 2).min(chars.len());
            }
            '#' if at_word_start => break,
            '<' if chars.get(index + 1) == Some(&'<') => {
                if chars.get(index + 2) == Some(&'<') {
                    ambiguous = true;
                    index += 3;
                    continue;
                }
                index += 2;
                let strip_tabs = chars.get(index) == Some(&'-');
                if strip_tabs {
                    index += 1;
                }
                while chars.get(index).is_some_and(|ch| matches!(ch, ' ' | '\t')) {
                    index += 1;
                }
                let mut delimiter = String::new();
                let mut delimiter_quote = None;
                let mut delimiter_was_quoted = false;
                while let Some(&delimiter_ch) = chars.get(index) {
                    if let Some(quote_ch) = delimiter_quote {
                        if delimiter_ch == quote_ch {
                            delimiter_quote = None;
                        } else if delimiter_ch == '\\' && quote_ch == '"' {
                            index += 1;
                            if let Some(escaped) = chars.get(index) {
                                delimiter.push(*escaped);
                            } else {
                                ambiguous = true;
                            }
                        } else {
                            delimiter.push(delimiter_ch);
                        }
                        index += 1;
                        continue;
                    }
                    if matches!(delimiter_ch, '\'' | '"') {
                        delimiter_was_quoted = true;
                        delimiter_quote = Some(delimiter_ch);
                        index += 1;
                        continue;
                    }
                    if delimiter_ch == '\\' {
                        delimiter_was_quoted = true;
                        index += 1;
                        if let Some(escaped) = chars.get(index) {
                            delimiter.push(*escaped);
                            index += 1;
                        } else {
                            ambiguous = true;
                        }
                        continue;
                    }
                    if delimiter_ch.is_whitespace()
                        || is_shell_separator_char(delimiter_ch)
                        || matches!(delimiter_ch, '<' | '>')
                    {
                        break;
                    }
                    delimiter.push(delimiter_ch);
                    index += 1;
                }
                if delimiter.is_empty() || delimiter_quote.is_some() {
                    ambiguous = true;
                } else {
                    specs.push(HeredocSpec {
                        delimiter,
                        strip_tabs,
                        expands_body: !delimiter_was_quoted,
                    });
                }
                at_word_start = true;
            }
            ch if ch.is_whitespace() || is_shell_separator_char(ch) => {
                at_word_start = true;
                index += 1;
            }
            _ => {
                at_word_start = false;
                index += 1;
            }
        }
    }
    ambiguous |= quote.is_some();
    (specs, ambiguous)
}

fn heredoc_body_has_active_command_substitution(line: &str) -> bool {
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '`' => return true,
            '$' if chars.peek() == Some(&'(') => return true,
            _ => {}
        }
    }
    false
}

fn shell_tokens(command: &str) -> ShellLex {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut word_started = false;
    let mut origin = ShellWordOrigin::default();
    let mut active_dollar = false;
    let mut literal_dollar = false;
    let mut dynamic = false;
    let mut quote = None;
    let mut ambiguous = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                if let Some(escaped) = chars.next() {
                    if escaped == '\n' {
                        continue;
                    }
                    if escaped == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                        continue;
                    }
                    if escaped == '$' {
                        literal_dollar = true;
                    }
                    current.push(escaped);
                } else {
                    ambiguous = true;
                }
            } else {
                if ch == '$' {
                    if quote_ch == '\'' {
                        literal_dollar = true;
                    } else {
                        active_dollar = true;
                        if chars.peek() == Some(&'(') {
                            ambiguous = true;
                            dynamic = true;
                            current.push(ch);
                            push_command_substitution_tail(&mut current, &mut chars);
                            continue;
                        }
                    }
                } else if ch == '`' && quote_ch != '\'' {
                    dynamic = true;
                    ambiguous = true;
                    current.push(ch);
                    push_backtick_substitution_tail(&mut current, &mut chars);
                    continue;
                }
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                word_started = true;
                origin.syntactically_plain = false;
                if !current.contains('=') {
                    origin.assignment_name_plain = false;
                }
                quote = Some(ch);
            }
            '\\' => {
                if let Some(escaped) = chars.next() {
                    if escaped == '\n' {
                        continue;
                    }
                    if escaped == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                        continue;
                    }
                    origin.syntactically_plain = false;
                    if !current.contains('=') {
                        origin.assignment_name_plain = false;
                    }
                    if escaped == '$' {
                        literal_dollar = true;
                    }
                    word_started = true;
                    current.push(escaped);
                } else {
                    ambiguous = true;
                }
            }
            '\n' | '\r' => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                tokens.push(ShellToken::Separator(ShellSeparator::Sequence));
            }
            ch if ch.is_whitespace() => push_shell_word(
                &mut tokens,
                &mut current,
                &mut word_started,
                &mut origin,
                &mut active_dollar,
                &mut literal_dollar,
                &mut dynamic,
            ),
            ';' | '(' | ')' => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                tokens.push(ShellToken::Separator(if ch == ';' {
                    ShellSeparator::Sequence
                } else {
                    ShellSeparator::Group
                }));
                ambiguous |= matches!(ch, '(' | ')');
            }
            '&' if chars.peek() == Some(&'>') => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                chars.next();
                let mut redirection = String::from("&>");
                if chars.peek() == Some(&'>') {
                    redirection.push(chars.next().expect("peeked append redirection"));
                }
                push_inline_redirection_target(&mut redirection, &mut chars);
                ambiguous |= shell_fragment_has_active_command_substitution(&redirection);
                tokens.push(ShellToken::Redirection(redirection));
            }
            '&' | '|' => {
                push_shell_word(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                let doubled = chars.peek() == Some(&ch);
                if doubled {
                    chars.next();
                }
                let separator = match (ch, doubled) {
                    ('&', true) => ShellSeparator::And,
                    ('|', true) => ShellSeparator::Or,
                    ('&', false) => ShellSeparator::Background,
                    ('|', false) => ShellSeparator::Pipe,
                    _ => unreachable!(),
                };
                tokens.push(ShellToken::Separator(separator));
            }
            '<' | '>' => {
                push_shell_word_or_drop_fd_prefix(
                    &mut tokens,
                    &mut current,
                    &mut word_started,
                    &mut origin,
                    &mut active_dollar,
                    &mut literal_dollar,
                    &mut dynamic,
                );
                let mut redirection = String::from(ch);
                if chars.peek() == Some(&ch) {
                    redirection.push(chars.next().expect("peeked redirection operator"));
                    if ch == '<' && chars.peek().is_some_and(|next| matches!(*next, '-' | '<')) {
                        redirection.push(chars.next().expect("peeked heredoc modifier"));
                    }
                } else if (ch == '>' && chars.peek() == Some(&'|'))
                    || (ch == '<' && chars.peek() == Some(&'>'))
                {
                    redirection.push(chars.next().expect("peeked compound redirection"));
                }
                if chars.peek() == Some(&'&') {
                    redirection.push(chars.next().expect("peeked redirection target marker"));
                }
                push_inline_redirection_target(&mut redirection, &mut chars);
                ambiguous |= shell_fragment_has_active_command_substitution(&redirection);
                tokens.push(ShellToken::Redirection(redirection));
            }
            '#' if !word_started => {
                while let Some(comment_ch) = chars.next() {
                    if matches!(comment_ch, '\n' | '\r') {
                        if comment_ch == '\r' && chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        tokens.push(ShellToken::Separator(ShellSeparator::Sequence));
                        break;
                    }
                }
            }
            '$' => {
                word_started = true;
                active_dollar = true;
                current.push(ch);
                if chars.peek() == Some(&'(') {
                    ambiguous = true;
                    dynamic = true;
                    push_command_substitution_tail(&mut current, &mut chars);
                }
            }
            '`' => {
                word_started = true;
                dynamic = true;
                ambiguous = true;
                current.push(ch);
                push_backtick_substitution_tail(&mut current, &mut chars);
            }
            _ => {
                word_started = true;
                current.push(ch);
            }
        }
    }

    ambiguous |= quote.is_some();
    push_shell_word(
        &mut tokens,
        &mut current,
        &mut word_started,
        &mut origin,
        &mut active_dollar,
        &mut literal_dollar,
        &mut dynamic,
    );
    ShellLex { tokens, ambiguous }
}

fn push_command_substitution_tail(
    rendered: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    let Some(opener) = chars.next() else {
        return;
    };
    debug_assert_eq!(opener, '(');
    rendered.push(opener);
    let mut depth = 1usize;
    let mut quote = None;
    while let Some(ch) = chars.next() {
        rendered.push(ch);
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch != '\'' {
                if let Some(escaped) = chars.next() {
                    rendered.push(escaped);
                }
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '\\' => {
                if let Some(escaped) = chars.next() {
                    rendered.push(escaped);
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            _ => {}
        }
    }
}

fn push_backtick_substitution_tail(
    rendered: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    while let Some(ch) = chars.next() {
        rendered.push(ch);
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                rendered.push(escaped);
            }
        } else if ch == '`' {
            return;
        }
    }
}

fn push_shell_word(
    tokens: &mut Vec<ShellToken>,
    current: &mut String,
    word_started: &mut bool,
    origin: &mut ShellWordOrigin,
    active_dollar: &mut bool,
    literal_dollar: &mut bool,
    dynamic: &mut bool,
) {
    if *word_started {
        tokens.push(ShellToken::Word(ShellWord {
            value: std::mem::take(current),
            syntactically_plain: origin.syntactically_plain,
            assignment_name_plain: origin.assignment_name_plain,
            active_dollar: std::mem::take(active_dollar),
            literal_dollar: std::mem::take(literal_dollar),
            dynamic: std::mem::take(dynamic),
        }));
    }
    *word_started = false;
    *origin = ShellWordOrigin::default();
}

fn push_shell_word_or_drop_fd_prefix(
    tokens: &mut Vec<ShellToken>,
    current: &mut String,
    word_started: &mut bool,
    origin: &mut ShellWordOrigin,
    active_dollar: &mut bool,
    literal_dollar: &mut bool,
    dynamic: &mut bool,
) {
    if origin.syntactically_plain
        && !current.is_empty()
        && current.chars().all(|ch| ch.is_ascii_digit())
    {
        current.clear();
        *word_started = false;
        *origin = ShellWordOrigin::default();
        *active_dollar = false;
        *literal_dollar = false;
        *dynamic = false;
    } else {
        push_shell_word(
            tokens,
            current,
            word_started,
            origin,
            active_dollar,
            literal_dollar,
            dynamic,
        );
    }
}

fn push_inline_redirection_target(
    redirection: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    let mut quote = None;
    let mut substitution_quote = None;
    let mut substitution_depth = 0usize;
    while let Some(next) = chars.peek().copied() {
        if substitution_depth > 0 {
            let ch = chars.next().expect("peeked command substitution character");
            redirection.push(ch);
            if let Some(quote_ch) = substitution_quote {
                if ch == quote_ch {
                    substitution_quote = None;
                } else if ch == '\\' && quote_ch != '\'' {
                    if let Some(escaped) = chars.next() {
                        redirection.push(escaped);
                    }
                }
                continue;
            }
            match ch {
                '\'' | '"' => substitution_quote = Some(ch),
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        redirection.push(escaped);
                    }
                }
                '(' => substitution_depth += 1,
                ')' => substitution_depth -= 1,
                _ => {}
            }
            continue;
        }
        if quote.is_none()
            && (next.is_whitespace() || is_shell_separator_char(next) || matches!(next, '<' | '>'))
        {
            break;
        }
        let ch = chars.next().expect("peeked redirection target");
        redirection.push(ch);
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                if let Some(escaped) = chars.next() {
                    redirection.push(escaped);
                }
            } else if ch == '$' && quote_ch != '\'' && chars.peek() == Some(&'(') {
                redirection.push(chars.next().expect("peeked command substitution opener"));
                substitution_depth = 1;
            } else if ch == '`' && quote_ch != '\'' {
                push_backtick_substitution_tail(redirection, chars);
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '\\' => {
                if let Some(escaped) = chars.next() {
                    redirection.push(escaped);
                }
            }
            '$' if chars.peek() == Some(&'(') => {
                redirection.push(chars.next().expect("peeked command substitution opener"));
                substitution_depth = 1;
            }
            '`' => push_backtick_substitution_tail(redirection, chars),
            _ => {}
        }
    }
}

fn shell_fragment_has_active_command_substitution(fragment: &str) -> bool {
    let mut quote = None;
    let mut chars = fragment.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else if ch == '\\' && quote_ch == '"' {
                chars.next();
            } else if quote_ch != '\'' && (ch == '`' || ch == '$' && chars.peek() == Some(&'(')) {
                return true;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '\\' => {
                chars.next();
            }
            '`' => return true,
            '$' if chars.peek() == Some(&'(') => return true,
            _ => {}
        }
    }
    false
}

fn redirection_has_inline_target(redirection: &str) -> bool {
    let operator_len = [
        "&>>", "<<-", "<<<", "&>", ">>", "<<", ">&", "<&", ">|", "<>",
    ]
    .into_iter()
    .find(|operator| redirection.starts_with(operator))
    .map_or(1, str::len);
    redirection.len() > operator_len
}

fn is_shell_separator_char(ch: char) -> bool {
    matches!(ch, ';' | '&' | '|' | '(' | ')')
}

fn shell_command_changes_directory(words: &[ShellWord]) -> bool {
    let ShellCommandName::Executable {
        index,
        force_external: false,
        ..
    } = shell_command_name(words)
    else {
        return false;
    };
    matches!(
        shell_word_value(&words[index]).as_str(),
        "cd" | "pushd" | "popd"
    )
}

fn resolve_literal_cd(root: &Path, cwd: &Path, words: &[ShellWord]) -> Option<PathBuf> {
    if shell_word_value(words.first()?) != "cd" {
        return None;
    }
    let path_word = match words {
        [_, path] => path,
        [_, option, path] if shell_word_value(option) == "--" => path,
        _ => return None,
    };
    let value = shell_word_value(path_word);
    if value.is_empty()
        || value.starts_with('~')
        || path_word.active_dollar
        || path_word.literal_dollar
        || path_word.dynamic
        || value.chars().any(|ch| matches!(ch, '*' | '?' | '['))
    {
        return None;
    }
    let candidate = PathBuf::from(value);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    };
    let root = fs::canonicalize(root).ok()?;
    let candidate = fs::canonicalize(candidate).ok()?;
    (candidate.is_dir() && candidate.starts_with(root)).then_some(candidate)
}

fn literal_exit_guard(words: &[ShellWord]) -> bool {
    match words {
        [command] => shell_word_value(command) == "exit",
        [command, status] => {
            shell_word_value(command) == "exit"
                && !status.active_dollar
                && !status.literal_dollar
                && !status.dynamic
                && !status.value.is_empty()
                && status.value.chars().all(|ch| ch.is_ascii_digit())
        }
        _ => false,
    }
}

fn reported_program(command_key: &str, program: &str) -> (String, bool) {
    let sqlx_safe_name = matches!(program, "cargo" | "cargo-sqlx" | "sqlx");
    let redact = (command_key == "sqlx_check_command" && !sqlx_safe_name)
        || credential_like_program(program);
    if redact {
        ("<redacted: command executable>".to_string(), true)
    } else {
        (program.to_string(), false)
    }
}

fn credential_like_program(program: &str) -> bool {
    let lowercase = program.to_ascii_lowercase();
    program.contains("://")
        || (program.contains('@') && program.contains(':'))
        || ["password", "passwd", "secret", "token", "apikey", "api_key"]
            .iter()
            .any(|marker| lowercase.contains(marker))
}

fn looks_like_shell_assignment(value: &str) -> bool {
    bash_assignment_name(value).is_some()
}

fn shell_command_prefix_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "do" | "done" | "elif" | "else" | "esac" | "fi" | "if" | "then" | "until" | "while"
    )
}

fn shell_word_is_prefix_keyword(word: &ShellWord, allow_prefix_keyword: bool) -> bool {
    allow_prefix_keyword && word.syntactically_plain && shell_command_prefix_keyword(&word.value)
}

fn shell_word_is_keyword(word: &ShellWord) -> bool {
    word.syntactically_plain && bash_keyword(&word.value)
}

fn bash_builtin(program: &str) -> bool {
    matches!(
        program,
        "." | ":"
            | "["
            | "alias"
            | "bg"
            | "bind"
            | "break"
            | "builtin"
            | "caller"
            | "cd"
            | "command"
            | "compgen"
            | "complete"
            | "compopt"
            | "continue"
            | "declare"
            | "dirs"
            | "disown"
            | "echo"
            | "enable"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "false"
            | "fc"
            | "fg"
            | "getopts"
            | "hash"
            | "help"
            | "history"
            | "jobs"
            | "kill"
            | "let"
            | "local"
            | "logout"
            | "mapfile"
            | "popd"
            | "printf"
            | "pushd"
            | "pwd"
            | "read"
            | "readarray"
            | "readonly"
            | "return"
            | "set"
            | "shift"
            | "shopt"
            | "source"
            | "suspend"
            | "test"
            | "times"
            | "trap"
            | "true"
            | "type"
            | "typeset"
            | "ulimit"
            | "umask"
            | "unalias"
            | "unset"
            | "wait"
    )
}

fn bash_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "[["
            | "]]"
            | "case"
            | "coproc"
            | "do"
            | "done"
            | "elif"
            | "else"
            | "esac"
            | "fi"
            | "for"
            | "function"
            | "if"
            | "in"
            | "select"
            | "then"
            | "time"
            | "until"
            | "while"
            | "{"
            | "}"
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProgramOrigin {
    ExplicitPath,
    SearchPath { entry: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramResolution {
    path: PathBuf,
    origin: ProgramOrigin,
}

fn search_path_is_cwd_independent(search_path: Option<&OsStr>) -> bool {
    let Some(search_path) = search_path else {
        return false;
    };
    env::split_paths(search_path).all(|entry| !entry.as_os_str().is_empty() && entry.is_absolute())
}

fn resolve_program(
    command_cwd: &Path,
    program: &str,
    search_path: Option<&OsStr>,
    path_extensions: Option<&OsStr>,
) -> Option<ProgramResolution> {
    if program_has_explicit_path(program) {
        let path = PathBuf::from(program);
        let path = if path.is_absolute() {
            path
        } else {
            command_cwd.join(path)
        };
        return executable_candidates(&path, path_extensions)
            .into_iter()
            .find(|path| executable_exists(path))
            .map(|path| ProgramResolution {
                path,
                origin: ProgramOrigin::ExplicitPath,
            });
    }

    let search_path = search_path?;
    for entry in env::split_paths(search_path) {
        let directory = if entry.is_absolute() {
            entry.clone()
        } else {
            command_cwd.join(&entry)
        };
        if let Some(path) =
            search_path_executable_candidates(&directory.join(program), path_extensions)
                .into_iter()
                .find(|path| executable_exists(path))
        {
            return Some(ProgramResolution {
                path,
                origin: ProgramOrigin::SearchPath { entry },
            });
        }
    }
    None
}

fn program_has_explicit_path(program: &str) -> bool {
    let path = Path::new(program);
    path.is_absolute()
        || path.components().count() > 1
        || program.contains('/')
        || cfg!(windows) && program.contains('\\')
}

fn executable_candidates(path: &Path, path_extensions: Option<&OsStr>) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        windows_executable_candidates(path, path_extensions)
    }
    #[cfg(not(windows))]
    {
        let _ = path_extensions;
        vec![path.to_path_buf()]
    }
}

fn search_path_executable_candidates(path: &Path, path_extensions: Option<&OsStr>) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        windows_search_path_executable_candidates(path, path_extensions)
    }
    #[cfg(not(windows))]
    {
        let _ = path_extensions;
        vec![path.to_path_buf()]
    }
}

#[cfg(any(windows, test))]
fn windows_executable_candidates(path: &Path, path_extensions: Option<&OsStr>) -> Vec<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    if path.extension().is_some() {
        return candidates;
    }
    candidates.extend(
        validated_windows_path_extensions(path_extensions)
            .into_iter()
            .map(|extension| {
                let mut candidate = path.as_os_str().to_os_string();
                candidate.push(extension);
                PathBuf::from(candidate)
            }),
    );
    candidates
}

#[cfg(any(windows, test))]
fn windows_search_path_executable_candidates(
    path: &Path,
    path_extensions: Option<&OsStr>,
) -> Vec<PathBuf> {
    if path.extension().is_some() {
        return vec![path.to_path_buf()];
    }
    let mut candidates = validated_windows_path_extensions(path_extensions)
        .into_iter()
        .map(|extension| {
            let mut candidate = path.as_os_str().to_os_string();
            candidate.push(extension);
            PathBuf::from(candidate)
        })
        .collect::<Vec<_>>();
    // `Command`-style bare-name lookup follows PATHEXT on Windows. Retain an
    // extensionless fallback for executable files, but do not let unrelated
    // extensionless data shadow an adjacent `.exe`/`.cmd` program.
    candidates.push(path.to_path_buf());
    candidates
}

#[cfg(any(windows, test))]
fn validated_windows_path_extensions(path_extensions: Option<&OsStr>) -> Vec<String> {
    const DEFAULT: &str = ".COM;.EXE;.BAT;.CMD";
    let configured = path_extensions
        .and_then(OsStr::to_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT);
    let mut seen = HashSet::new();
    let mut extensions = configured
        .split(';')
        .map(str::trim)
        .filter(|extension| {
            extension.len() > 1
                && extension.starts_with('.')
                && extension[1..].chars().all(|ch| ch.is_ascii_alphanumeric())
        })
        .filter_map(|extension| {
            let normalized = extension.to_ascii_uppercase();
            seen.insert(normalized.clone()).then_some(normalized)
        })
        .collect::<Vec<_>>();
    if extensions.is_empty() {
        extensions = DEFAULT.split(';').map(str::to_string).collect();
    }
    extensions
}

#[cfg(windows)]
fn sanitized_windows_pathext(path_extensions: Option<&OsStr>) -> Option<OsString> {
    let extensions = validated_windows_path_extensions(path_extensions);
    (!extensions.is_empty()).then(|| extensions.join(";").into())
}

fn trusted_sqlx_probe_executable(
    root: &Path,
    program: &str,
    resolution: &ProgramResolution,
) -> Option<PathBuf> {
    if !bare_sqlx_probe_program(program) {
        return None;
    }
    trusted_bare_path_executable(root, resolution)
}

fn trusted_bare_path_executable(root: &Path, resolution: &ProgramResolution) -> Option<PathBuf> {
    let ProgramOrigin::SearchPath { entry } = &resolution.origin else {
        return None;
    };
    if entry.as_os_str().is_empty() || !entry.is_absolute() {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    if entry.starts_with(&root) || resolution.path.starts_with(&root) {
        return None;
    }
    if path_has_symlink_or_reparse_component(&resolution.path)? {
        return None;
    }
    let entry = fs::canonicalize(entry).ok()?;
    let executable = fs::canonicalize(&resolution.path).ok()?;
    if entry.starts_with(&root) || executable.starts_with(&root) {
        return None;
    }
    #[cfg(windows)]
    {
        let extension = executable.extension()?.to_str()?;
        if !matches!(extension.to_ascii_lowercase().as_str(), "exe" | "com") {
            return None;
        }
    }
    Some(executable)
}

fn cargo_sqlx_dispatch_issue(
    root: &Path,
    command: &str,
    environment: &DoctorEnvironment,
) -> &'static str {
    if environment.cargo_alias_sqlx.is_some() {
        return "CARGO_ALIAS_SQLX is set";
    }
    if cargo_sqlx_command_changes_dispatch_environment(command) {
        return "the command changes Cargo alias or home environment";
    }
    if cargo_sqlx_command_has_inline_config(command) {
        return "the cargo command has an inline --config override";
    }
    if effective_cargo_config_obscures_sqlx_alias(root, command, environment) {
        return "an effective Cargo config may change subcommand dispatch";
    }
    "an external cargo path does not prove which sqlx subcommand will run"
}

fn cargo_sqlx_command_changes_dispatch_environment(command: &str) -> bool {
    parse_shell_commands(command)
        .commands
        .iter()
        .any(|words| shell_command_changes_cargo_dispatch_environment(words))
}

fn shell_command_changes_cargo_dispatch_environment(words: &[ShellWord]) -> bool {
    if shell_command_may_persist_variable_change(words, &cargo_dispatch_environment_name) {
        return true;
    }
    let command_name = shell_command_name(words);
    let (program_index, force_external) = match command_name {
        ShellCommandName::Executable {
            index,
            force_external,
            ..
        } => (Some(index), force_external),
        ShellCommandName::NoExternalExecutable { .. }
        | ShellCommandName::AmbiguousWrapper { .. } => (None, false),
    };
    let mut leading_assignment_end = 0;
    while words
        .get(leading_assignment_end)
        .is_some_and(shell_word_is_assignment)
    {
        if bash_assignment_name(&shell_word_value(&words[leading_assignment_end]))
            .is_some_and(cargo_dispatch_environment_name)
        {
            return true;
        }
        leading_assignment_end += 1;
    }
    let prefix_end = program_index.unwrap_or(words.len());
    if words[..prefix_end].iter().any(|word| {
        shell_assignment_name(&shell_word_value(word)).is_some_and(cargo_dispatch_environment_name)
    }) {
        return true;
    }
    if program_index.is_none()
        && words.iter().any(|word| {
            shell_assignment_name(&shell_word_value(word))
                .is_some_and(cargo_dispatch_environment_name)
        })
    {
        return true;
    }

    let Some(program_index) = program_index else {
        return false;
    };
    let program = shell_word_value(&words[program_index]);
    if !force_external
        && matches!(
            program.as_str(),
            "export" | "local" | "readonly" | "typeset"
        )
        && words[program_index + 1..].iter().any(|word| {
            let word = shell_word_value(word);
            cargo_dispatch_environment_name(shell_assignment_name(&word).unwrap_or(word.as_str()))
        })
    {
        return true;
    }
    if !force_external
        && program == "unset"
        && words[program_index + 1..]
            .iter()
            .map(shell_word_value)
            .any(|name| cargo_dispatch_environment_name(&name))
    {
        return true;
    }

    let mut saw_env = false;
    let mut index = 0;
    while index < program_index {
        let word = shell_word_value(&words[index]);
        if word == "exec" && exec_wrapper_clears_environment(words, index + 1, program_index) {
            return true;
        }
        if executable_is_named(&word, "env") {
            saw_env = true;
        } else if saw_env {
            if matches!(word.as_str(), "-" | "-i" | "--ignore-environment") {
                return true;
            }
            if shell_assignment_name(&word).is_some_and(cargo_dispatch_environment_name) {
                return true;
            }
            if word == "-u" || word == "--unset" {
                index += 1;
                if words
                    .get(index)
                    .map(shell_word_value)
                    .as_deref()
                    .is_some_and(cargo_dispatch_environment_name)
                {
                    return true;
                }
            } else if word
                .strip_prefix("--unset=")
                .or_else(|| word.strip_prefix("-u"))
                .is_some_and(cargo_dispatch_environment_name)
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn cargo_dispatch_environment_name(name: &str) -> bool {
    const NAMES: [&str; 6] = [
        "CARGO_ALIAS_SQLX",
        "CARGO_HOME",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
    ];
    #[cfg(windows)]
    {
        NAMES
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
    }
    #[cfg(not(windows))]
    {
        NAMES.contains(&name)
    }
}

fn cargo_sqlx_command_has_inline_config(command: &str) -> bool {
    parse_shell_commands(command).commands.iter().any(|words| {
        let Some(program_index) = command_program_index(words) else {
            return false;
        };
        if !executable_is_named(&shell_word_value(&words[program_index]), "cargo") {
            return false;
        }
        let Some(sqlx_index) = cargo_subcommand_index(words, program_index + 1) else {
            return false;
        };
        if shell_word_value(&words[sqlx_index]) != "sqlx" {
            return false;
        }
        let mut index = program_index + 1;
        while index < sqlx_index {
            let value = shell_word_value(&words[index]);
            let config = if value == "--config" {
                index += 1;
                words.get(index).cloned()
            } else {
                value
                    .strip_prefix("--config=")
                    .map(|value| words[index].with_value(value))
            };
            if config
                .as_ref()
                .is_some_and(cargo_config_override_obscures_sqlx_alias)
            {
                return true;
            }
            index += 1;
        }
        false
    })
}

fn cargo_config_override_obscures_sqlx_alias(config: &ShellWord) -> bool {
    if config.active_dollar || config.literal_dollar || config.dynamic {
        return true;
    }
    let text = config.value.trim();
    if !text.contains('=') {
        return true;
    }
    let Ok(config) = text.parse::<toml::Table>() else {
        return true;
    };
    if config.contains_key("include") {
        return true;
    }
    match config.get("alias") {
        None => false,
        Some(toml::Value::Table(aliases)) => aliases.contains_key("sqlx"),
        Some(_) => true,
    }
}

fn effective_cargo_config_obscures_sqlx_alias(
    root: &Path,
    command: &str,
    environment: &DoctorEnvironment,
) -> bool {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(invocation_directories) = cargo_sqlx_invocation_directories(&root, command) else {
        return true;
    };
    for invocation_directory in invocation_directories {
        for directory in invocation_directory.ancestors() {
            if cargo_config_directory_obscures_sqlx_alias(&directory.join(".cargo")) {
                return true;
            }
        }
    }

    let cargo_home = if let Some(cargo_home) = environment.cargo_home.as_deref() {
        let cargo_home = PathBuf::from(cargo_home);
        if !cargo_home.is_absolute() {
            return true;
        }
        Some(cargo_home)
    } else if let Some(home) = environment.home.as_deref() {
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            return true;
        }
        Some(home.join(".cargo"))
    } else {
        None
    };
    cargo_home
        .as_deref()
        .is_some_and(cargo_config_directory_obscures_sqlx_alias)
}

fn cargo_sqlx_invocation_directories(root: &Path, command: &str) -> Option<Vec<PathBuf>> {
    let parsed = parse_shell_commands(command);
    if parsed.ambiguous {
        return None;
    }
    let mut cwd = root.to_path_buf();
    let mut directories = Vec::new();
    for (command_index, words) in parsed.commands.iter().enumerate() {
        let incoming = command_index
            .checked_sub(1)
            .and_then(|index| parsed.separators.get(index))
            .copied();
        let outgoing = parsed.separators.get(command_index).copied();
        if shell_command_changes_directory(words) {
            if !matches!(incoming, None | Some(ShellSeparator::Sequence))
                || outgoing != Some(ShellSeparator::And)
            {
                return None;
            }
            cwd = resolve_literal_cd(root, &cwd, words)?;
            continue;
        }
        let Some(program_index) = command_program_index(words) else {
            continue;
        };
        if executable_is_named(&shell_word_value(&words[program_index]), "cargo")
            && cargo_subcommand(words, program_index + 1).as_deref() == Some("sqlx")
        {
            directories.push(cwd.clone());
        }
    }
    (!directories.is_empty()).then_some(directories)
}

fn cargo_config_directory_obscures_sqlx_alias(directory: &Path) -> bool {
    [directory.join("config.toml"), directory.join("config")]
        .into_iter()
        .any(|path| cargo_config_obscures_sqlx_alias(&path))
}

fn cargo_config_obscures_sqlx_alias(path: &Path) -> bool {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let config = match text.parse::<toml::Value>() {
        Ok(config) => config,
        Err(_) => return true,
    };
    if config.get("include").is_some() {
        return true;
    }
    match config.get("alias") {
        None => false,
        Some(toml::Value::Table(aliases)) => aliases.contains_key("sqlx"),
        Some(_) => true,
    }
}

fn path_has_symlink_or_reparse_component(path: &Path) -> Option<bool> {
    use std::path::Component;

    if !path.is_absolute() {
        return None;
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
            }
            Component::CurDir => {}
            // Parent components make the raw lookup identity harder to
            // reason about. Bare PATH entries do not need them.
            Component::ParentDir => return None,
            Component::Normal(_) => {
                current.push(component.as_os_str());
                let metadata = fs::symlink_metadata(&current).ok()?;
                if metadata_is_symlink_or_reparse_point(&metadata) {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}

#[cfg(windows)]
fn metadata_is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn bare_sqlx_probe_program(program: &str) -> bool {
    if program_has_explicit_path(program) {
        return false;
    }
    #[cfg(windows)]
    {
        executable_basename(program).is_some_and(|basename| {
            basename.eq_ignore_ascii_case("sqlx") || basename.eq_ignore_ascii_case("cargo-sqlx")
        })
    }
    #[cfg(not(windows))]
    {
        matches!(program, "sqlx" | "cargo-sqlx")
    }
}

fn sanitized_probe_search_path(root: &Path, executable: &Path) -> Option<OsString> {
    let root = fs::canonicalize(root).ok()?;
    let executable = fs::canonicalize(executable).ok()?;
    let directory = executable.parent()?;
    if !directory.is_absolute() || directory.starts_with(root) {
        return None;
    }
    env::join_paths([directory]).ok()
}

fn program_presence(root: &Path, program: &str, resolved: Option<&Path>) -> (bool, String) {
    match resolved {
        Some(_) => (true, format!("{program} is available")),
        None if program_has_explicit_path(program) => {
            let path = PathBuf::from(program);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            (
                false,
                format!("{} is missing or not executable", path.display()),
            )
        }
        None => (false, format!("{program} was not found on PATH")),
    }
}

fn executable_exists(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::{CurrentDirGuard, EnvVarGuard, lock_env};
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    use crate::test_process::{
        TestProcessIdentity, assert_test_process_stopped, publish_test_process_identity,
        read_test_process_identity, terminate_and_confirm_test_process,
    };
    use serde_json::json;
    use tempfile::tempdir;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const OWNED_PROCESS_DESCENDANT_MARKER_ENV: &str = "JIG_DOCTOR_OWNED_PROCESS_DESCENDANT_MARKER";

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn shell_quote_test_path(path: &Path) -> String {
        let path = path
            .to_str()
            .expect("test helper paths must be representable in shell fixtures");
        format!("'{}'", path.replace('\'', "'\"'\"'"))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn owned_test_descendant_script(marker: &Path, tail: &str) -> String {
        format!(
            "#!/bin/sh\n{marker_env}={marker} {test_exe} --exact doctor::tests::owned_process_descendant_helper --nocapture &\nwhile [ ! -f {marker} ]; do :; done\n{tail}\n",
            marker_env = OWNED_PROCESS_DESCENDANT_MARKER_ENV,
            marker = shell_quote_test_path(marker),
            test_exe = shell_quote_test_path(&std::env::current_exe().unwrap()),
        )
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn owned_process_descendant_helper() {
        let Some(marker) = std::env::var_os(OWNED_PROCESS_DESCENDANT_MARKER_ENV) else {
            return;
        };
        let identity =
            TestProcessIdentity::capture_current().expect("capture test helper identity");
        publish_test_process_identity(Path::new(&marker), &identity);
        std::thread::sleep(Duration::from_secs(30));
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn owned_process_max_timeout_helper() {
        if std::env::var_os("JIG_DOCTOR_MAX_TIMEOUT_HELPER").is_some() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_list_output_helper() {
        let Some(mode) = std::env::var_os("JIG_DOCTOR_PROXY_LIST_HELPER") else {
            return;
        };
        if mode == "valid" {
            println!(r#"{{"ok":true,"running":false,"routes":[]}}"#);
            return;
        }

        let marker = PathBuf::from(
            std::env::var_os("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER")
                .expect("hanging proxy-list helper has a descendant marker"),
        );
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::owned_process_descendant_helper",
                "--nocapture",
            ])
            .env(OWNED_PROCESS_DESCENDANT_MARKER_ENV, &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        std::mem::forget(child);
        let _ = read_test_process_identity(&marker);
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn program_resolution_distinguishes_unset_and_explicitly_empty_path() {
        let _env = lock_env();
        let repo = tempdir().unwrap();
        let invocation = tempdir().unwrap();
        let _cwd = CurrentDirGuard::set(invocation.path());
        let program = "doctor-empty-path-tool";
        #[cfg(unix)]
        write_test_executable(&repo.path().join(program), "#!/bin/sh\nexit 0\n");
        #[cfg(windows)]
        fs::write(repo.path().join(format!("{program}.CMD")), "@exit /b 0\r\n").unwrap();
        #[cfg(not(any(unix, windows)))]
        fs::write(repo.path().join(program), "executable\n").unwrap();
        #[cfg(windows)]
        let path_extensions = Some(OsStr::new(".CMD"));
        #[cfg(not(windows))]
        let path_extensions = None;

        assert_eq!(
            resolve_program(repo.path(), program, None, path_extensions),
            None
        );
        let resolution =
            resolve_program(repo.path(), program, Some(OsStr::new("")), path_extensions).unwrap();
        assert!(resolution.path.starts_with(repo.path()));
        assert!(!resolution.path.starts_with(invocation.path()));
        assert_eq!(
            resolution.origin,
            ProgramOrigin::SearchPath {
                entry: PathBuf::new()
            }
        );
    }

    #[test]
    fn command_programs_include_external_env_and_skip_shell_assignments() {
        assert_eq!(command_program("cargo test").as_deref(), Some("cargo"));
        assert_eq!(
            command_program("RUSTFLAGS=-Dwarnings cargo test").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            command_programs_for_shell("env RUSTFLAGS=-Dwarnings cargo test"),
            vec!["env", "cargo"]
        );
        assert_eq!(
            command_programs_for_shell("env FOO.BAR=x cargo test"),
            vec!["env", "cargo"]
        );
        assert_eq!(
            command_program("\"scripts/jig\" check contract").as_deref(),
            Some("scripts/jig")
        );
    }

    #[test]
    fn command_programs_only_treat_keywords_as_pre_assignment_prefixes() {
        for command in [
            "DATABASE_URL=sqlite:x ! sqlx prepare",
            "! DATABASE_URL=sqlite:x ! sqlx prepare",
            "DATABASE_URL=sqlite:x then sqlx prepare",
        ] {
            assert!(
                command_programs_for_shell(command).is_empty(),
                "{command:?}",
            );
        }

        assert_eq!(
            command_programs_for_shell("! DATABASE_URL=sqlite:x sqlx prepare"),
            vec!["sqlx"],
        );
        for command in [
            "''! DATABASE_URL=sqlite:x sqlx prepare",
            r"\! DATABASE_URL=sqlite:x sqlx prepare",
        ] {
            assert_eq!(
                command_programs_for_shell(command),
                vec!["!"],
                "{command:?}"
            );
        }
    }

    #[test]
    fn required_programs_treat_env_split_strings_and_unknown_options_as_ambiguous() {
        for command in [
            "env -S 'private-split-tool --flag'",
            "env -Sprivate-split-tool",
            "env '-Sprivate-split-tool --flag'",
            "env --split-string 'private-split-tool --flag'",
            "env --split-string",
            "env '--split-string=private-split-tool --flag'",
            "env -iS 'private-split-tool --flag'",
            "env -S 'private-split-tool --flag' cargo",
            "env --private-option cargo",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::Wrapper),
                "{command:?}",
            );
            assert_eq!(
                discovery
                    .programs
                    .iter()
                    .map(|program| program.program.as_str())
                    .collect::<Vec<_>>(),
                vec!["env"],
                "{command:?}",
            );
            assert_eq!(
                discovery.programs[0].path_lookup,
                ProgramPathLookup::Captured,
                "{command:?}",
            );
        }

        for command in [
            "env -P /private/tools cargo test",
            "env -P/private/tools cargo test",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::Wrapper),
                "{command:?}",
            );
            assert_eq!(
                discovery
                    .programs
                    .iter()
                    .map(|program| program.program.as_str())
                    .collect::<Vec<_>>(),
                vec!["env", "cargo"],
                "{command:?}",
            );
            assert_eq!(
                discovery.programs[0].path_lookup,
                ProgramPathLookup::Captured
            );
            assert_eq!(
                discovery.programs[1].path_lookup,
                ProgramPathLookup::Unverifiable
            );
        }
    }

    #[test]
    fn required_programs_model_builtin_dispatch_and_external_wrapper_boundaries() {
        for command in [
            "builtin export DATABASE_URL=sqlite:ignored.db",
            "builtin declare DATABASE_URL=sqlite:ignored.db",
            "builtin printf -v DATABASE_URL %s sqlite:ignored.db",
            "builtin command export DATABASE_URL=sqlite:ignored.db",
            "command builtin export DATABASE_URL=sqlite:ignored.db",
            "builtin doctor-not-a-builtin cargo test",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert!(discovery.programs.is_empty(), "{command:?}");
            assert!(discovery.ambiguity.is_none(), "{command:?}");
        }

        for (command, expected) in [
            ("builtin command cargo test", vec!["cargo"]),
            ("builtin exec cargo test", vec!["cargo"]),
            ("exec export DATABASE_URL=x", vec!["export"]),
            ("env export DATABASE_URL=x", vec!["env", "export"]),
            ("nohup export DATABASE_URL=x", vec!["nohup", "export"]),
            ("command exec export DATABASE_URL=x", vec!["export"]),
            ("env exec cargo test", vec!["env", "exec"]),
            ("nohup command cargo test", vec!["nohup", "command"]),
            ("exec env cargo test", vec!["env", "cargo"]),
            ("env nohup cargo test", vec!["env", "nohup", "cargo"]),
        ] {
            assert_eq!(command_programs_for_shell(command), expected, "{command:?}");
        }
    }

    #[test]
    fn required_programs_emit_ordered_external_wrapper_chains() {
        for (command, expected) in [
            (
                "env nohup /usr/bin/time cargo test",
                vec!["env", "nohup", "/usr/bin/time", "cargo"],
            ),
            (
                "/opt/tools/env /opt/tools/nohup /opt/tools/time /opt/tools/cargo test",
                vec![
                    "/opt/tools/env",
                    "/opt/tools/nohup",
                    "/opt/tools/time",
                    "/opt/tools/cargo",
                ],
            ),
            ("command time cargo test", vec!["time", "cargo"]),
            ("exec env cargo test", vec!["env", "cargo"]),
            ("command exec cargo test", vec!["cargo"]),
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery
                    .programs
                    .iter()
                    .map(|program| program.program.as_str())
                    .collect::<Vec<_>>(),
                expected,
                "{command:?}",
            );
            assert!(discovery.ambiguity.is_none(), "{command:?}");
        }
    }

    #[test]
    fn required_programs_retain_external_wrappers_without_a_known_target() {
        for command in [
            "env",
            "env --help",
            "env -0",
            "nohup",
            "nohup --help",
            "/usr/bin/time --help",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(discovery.programs.len(), 1, "{command:?}");
            assert!(discovery.ambiguity.is_none(), "{command:?}");
        }
        assert_eq!(
            command_programs_for_shell("nohup -- --help"),
            vec!["nohup", "--help"]
        );

        for (command, expected_wrapper) in [
            ("env \"$TOOL\" test", "env"),
            ("env --private-option cargo test", "env"),
            ("nohup --private-option cargo test", "nohup"),
            ("/usr/bin/time --private-option cargo test", "/usr/bin/time"),
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::Wrapper),
                "{command:?}",
            );
            assert_eq!(discovery.programs.len(), 1, "{command:?}");
            assert_eq!(
                discovery.programs[0].program, expected_wrapper,
                "{command:?}",
            );
        }
    }

    #[test]
    fn required_programs_recognize_complete_standard_bash_builtins_and_keywords() {
        for builtin in [
            ".",
            ":",
            "[",
            "alias",
            "bg",
            "bind",
            "break",
            "builtin",
            "caller",
            "cd",
            "command",
            "compgen",
            "complete",
            "compopt",
            "continue",
            "declare",
            "dirs",
            "disown",
            "echo",
            "enable",
            "eval",
            "exec",
            "exit",
            "export",
            "false",
            "fc",
            "fg",
            "getopts",
            "hash",
            "help",
            "history",
            "jobs",
            "kill",
            "let",
            "local",
            "logout",
            "mapfile",
            "popd",
            "printf",
            "pushd",
            "pwd",
            "read",
            "readarray",
            "readonly",
            "return",
            "set",
            "shift",
            "shopt",
            "source",
            "suspend",
            "test",
            "times",
            "trap",
            "true",
            "type",
            "typeset",
            "ulimit",
            "umask",
            "unalias",
            "unset",
            "wait",
        ] {
            assert!(bash_builtin(builtin), "missing Bash builtin {builtin:?}");
        }
        for keyword in [
            "!", "[[", "]]", "case", "coproc", "do", "done", "elif", "else", "esac", "fi", "for",
            "function", "if", "in", "select", "then", "time", "until", "while", "{", "}",
        ] {
            assert!(bash_keyword(keyword), "missing Bash keyword {keyword:?}");
        }

        for command in [
            "wait",
            "kill -0 1",
            "mapfile -t values </dev/null",
            "readarray values </dev/null",
            "getopts p option",
            "hash -r",
            "help wait",
            "shopt -s nullglob",
            "pushd .",
            "popd",
            "\"wait\"",
        ] {
            assert!(
                command_programs_for_shell(command).is_empty(),
                "{command:?}"
            );
        }

        let quoted_keyword = required_command_programs_for_shell("\"[[\" argument");
        assert_eq!(quoted_keyword.programs[0].program, "[[");
        assert!(quoted_keyword.ambiguity.is_none());
        let command_keyword = required_command_programs_for_shell("command time true");
        assert_eq!(command_keyword.programs[0].program, "time");
    }

    #[test]
    fn required_programs_surface_dynamic_eval_source_time_and_global_parse_ambiguity() {
        for command in [
            "$TOOL test",
            "command \"$TOOL\" test",
            "eval 'cargo test'",
            "source scripts/setup.sh",
            ". scripts/setup.sh",
            "builtin eval 'cargo test'",
            "command source scripts/setup.sh",
            "time cargo test",
            "[[ -n value ]]",
            "cargo \"$(missing-helper)\" test",
            "cargo test >\"$(missing-helper)\"",
            "cargo `missing-helper` test",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert!(discovery.ambiguity.is_some(), "{command:?}");
            assert!(
                discovery
                    .programs
                    .iter()
                    .all(|program| program.path_lookup == ProgramPathLookup::Unverifiable),
                "{command:?}",
            );
        }

        let dynamic_target = required_command_programs_for_shell("nohup \"$TOOL\" test");
        assert_eq!(
            dynamic_target.ambiguity,
            Some(RequiredProgramAmbiguity::Wrapper)
        );
        assert_eq!(dynamic_target.programs[0].program, "nohup");
        assert_eq!(
            dynamic_target.programs[0].path_lookup,
            ProgramPathLookup::Captured
        );

        let literal = required_command_programs_for_shell("'$TOOL' test");
        assert_eq!(literal.programs[0].program, "$TOOL");
        assert_eq!(literal.programs[0].path_lookup, ProgramPathLookup::Captured);

        for command in [
            "doctor_fn() { :; }; doctor_fn",
            "if true; then /definitely/missing; fi",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::ShellSyntax),
                "{command:?}",
            );
            assert!(
                discovery
                    .programs
                    .iter()
                    .all(|program| program.path_lookup == ProgramPathLookup::Unverifiable),
                "{command:?}",
            );
        }
    }

    #[test]
    fn required_programs_taint_dispatch_after_shell_state_mutations() {
        for (command, expected_program) in [
            ("hash -p /tmp/shim cargo; cargo test", "cargo"),
            ("enable -f /tmp/plugin custom; custom", "custom"),
            ("trap 'missing-helper' DEBUG; cargo test", "cargo"),
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::ShellState),
                "{command:?}",
            );
            let program = discovery.programs.last().unwrap();
            assert_eq!(program.program, expected_program, "{command:?}");
            assert_eq!(
                program.path_lookup.clone(),
                ProgramPathLookup::Unverifiable,
                "{command:?}",
            );
        }

        for command in ["hash -t cargo; cargo test", "trap -p; cargo test"] {
            let discovery = required_command_programs_for_shell(command);
            assert!(discovery.ambiguity.is_none(), "{command:?}");
            assert_eq!(
                discovery.programs.last().unwrap().path_lookup.clone(),
                ProgramPathLookup::Captured,
                "{command:?}",
            );
        }
    }

    #[test]
    fn required_programs_do_not_resolve_cwd_sensitive_paths_from_the_repo_root() {
        for command in [
            "env -C sub ./tool",
            "env --chdir sub ./tool",
            "cd sub && ./tool",
        ] {
            let discovery = required_command_programs_for_shell(command);
            let program = discovery.programs.last().unwrap();
            assert_eq!(program.program, "./tool", "{command:?}");
            assert_eq!(
                program.path_lookup.clone(),
                ProgramPathLookup::Unverifiable,
                "{command:?}"
            );
        }
        for command in ["env -C sub tool", "cd sub && tool"] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.programs.last().unwrap().path_lookup.clone(),
                ProgramPathLookup::CapturedAfterCwdChange,
                "{command:?}",
            );
        }
        assert_eq!(
            required_command_programs_for_shell("env -C child cargo test")
                .programs
                .iter()
                .map(|program| (program.program.as_str(), program.path_lookup.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("env", ProgramPathLookup::Captured),
                ("cargo", ProgramPathLookup::CapturedAfterCwdChange),
            ]
        );
        assert_eq!(
            required_command_programs_for_shell("env -C child nohup cargo test")
                .programs
                .iter()
                .map(|program| (program.program.as_str(), program.path_lookup.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("env", ProgramPathLookup::Captured),
                ("nohup", ProgramPathLookup::CapturedAfterCwdChange),
                ("cargo", ProgramPathLookup::CapturedAfterCwdChange),
            ]
        );
        assert_eq!(
            required_command_programs_for_shell("cd sub; PATH=relative tool")
                .programs
                .last()
                .unwrap()
                .path_lookup,
            ProgramPathLookup::Unverifiable,
        );
        #[cfg(not(windows))]
        for command in ["env -C sub /usr/bin/tool", "cd sub && /usr/bin/tool"] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.programs.last().unwrap().path_lookup,
                ProgramPathLookup::Explicit,
                "{command:?}",
            );
        }
        #[cfg(windows)]
        for command in [
            "env -C sub C:/Windows/System32/cmd.exe",
            "cd sub && C:/Windows/System32/cmd.exe",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.programs.last().unwrap().path_lookup,
                ProgramPathLookup::Explicit,
                "{command:?}",
            );
        }
        #[cfg(not(windows))]
        {
            assert!(search_path_is_cwd_independent(Some(OsStr::new(
                "/usr/bin:/bin"
            ))));
            assert!(!search_path_is_cwd_independent(Some(OsStr::new(
                "bin:/usr/bin"
            ))));
            assert!(!search_path_is_cwd_independent(Some(OsStr::new(
                ":/usr/bin"
            ))));
        }
        #[cfg(windows)]
        {
            assert!(search_path_is_cwd_independent(Some(OsStr::new(
                r"C:\Windows;D:\bin"
            ))));
            assert!(!search_path_is_cwd_independent(Some(OsStr::new(
                r"bin;C:\Windows"
            ))));
            assert!(!search_path_is_cwd_independent(Some(OsStr::new(
                r";C:\Windows"
            ))));
        }
    }

    #[test]
    fn required_programs_treat_env_null_with_a_utility_as_ambiguous() {
        for command in [
            "env -0 cargo test",
            "env --null cargo test",
            "env -i0 cargo test",
            "env -0 DATABASE_URL=sqlite:ignored.db cargo test",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(discovery.programs[0].program, "env", "{command:?}");
            assert_eq!(discovery.programs.len(), 1, "{command:?}");
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::Wrapper),
                "{command:?}",
            );
        }
        let print_environment = required_command_programs_for_shell("env -0 DATABASE_URL=value");
        assert_eq!(print_environment.programs[0].program, "env");
        assert_eq!(print_environment.programs.len(), 1);
        assert!(print_environment.ambiguity.is_none());
    }

    #[test]
    fn executable_basename_is_utf8_boundary_safe() {
        assert_eq!(executable_basename("💩a"), Some("💩a"));
        assert_eq!(executable_basename("💩.EXE"), Some("💩"));
        let discovery = required_command_programs_for_shell("💩a --version");
        assert_eq!(discovery.programs[0].program, "💩a");
    }

    #[test]
    fn command_programs_respect_env_option_and_wrapper_assignment_boundaries() {
        assert_eq!(
            command_programs_for_shell("env FOO=one -i cargo test"),
            vec!["env", "-i"]
        );
        assert_eq!(
            command_programs_for_shell("env -i FOO=one cargo test"),
            vec!["env", "cargo"]
        );
        assert_eq!(
            command_programs_for_shell("command env FOO=one cargo test"),
            vec!["env", "cargo"]
        );

        for command in [
            "command DATABASE_URL=sqlite:private.db cargo sqlx prepare",
            "exec DATABASE_URL=sqlite:private.db cargo sqlx prepare",
        ] {
            let discovery = required_command_programs_for_shell(command);
            assert_eq!(
                discovery.ambiguity,
                Some(RequiredProgramAmbiguity::Wrapper),
                "{command:?}",
            );
            assert!(discovery.programs.is_empty(), "{command:?}");
        }
        let nohup = required_command_programs_for_shell(
            "nohup DATABASE_URL=sqlite:private.db cargo sqlx prepare",
        );
        assert_eq!(nohup.ambiguity, Some(RequiredProgramAmbiguity::Wrapper));
        assert_eq!(nohup.programs[0].program, "nohup");
        assert_eq!(nohup.programs.len(), 1);
    }

    #[test]
    fn required_programs_track_path_lookup_scope() {
        let lookups = |command: &str| {
            required_command_programs_for_shell(command)
                .programs
                .into_iter()
                .map(|program| (program.program, program.path_lookup))
                .collect::<Vec<_>>()
        };

        for command in [
            "PATH=repo-bin; sqlx prepare",
            "PATH[0]=repo-bin; sqlx prepare",
            "export PATH=repo-bin; sqlx prepare",
            "declare -x PATH=repo-bin; sqlx prepare",
            "builtin declare -x PATH=repo-bin; sqlx prepare",
            "read PATH <<< repo-bin; sqlx prepare",
            "printf -v PATH %s repo-bin; sqlx prepare",
            "printf -v 'PATH[0]' %s repo-bin; sqlx prepare",
            "mapfile -t PATH </dev/null; sqlx prepare",
            "getopts p PATH; sqlx prepare",
            "let PATH=0; sqlx prepare",
            "declare -n path_ref=PATH; sqlx prepare",
            "unset PATH; sqlx prepare",
            "PATH=repo-bin && sqlx prepare",
            "PATH+=:repo-bin; sqlx prepare",
            "false && export PATH=repo-bin; sqlx prepare",
            "source settings.sh; sqlx prepare",
        ] {
            assert_eq!(
                lookups(command).last(),
                Some(&("sqlx".to_string(), ProgramPathLookup::Unverifiable)),
                "{command:?}"
            );
        }

        assert_eq!(
            lookups("PATH=repo-bin sqlx prepare"),
            vec![(
                "sqlx".to_string(),
                ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
            )]
        );
        assert_eq!(
            lookups("PATH= sqlx prepare"),
            vec![(
                "sqlx".to_string(),
                ProgramPathLookup::CommandLocal(OsString::new()),
            )]
        );
        assert_eq!(
            lookups("command -p sqlx prepare"),
            vec![("sqlx".to_string(), ProgramPathLookup::Unverifiable)]
        );
        assert_eq!(
            lookups("env PATH=/missing sqlx prepare"),
            vec![
                ("env".to_string(), ProgramPathLookup::Captured),
                (
                    "sqlx".to_string(),
                    ProgramPathLookup::CommandLocal(OsString::from("/missing")),
                ),
            ]
        );
        for command in ["env -u PATH sqlx prepare", "env -i sqlx prepare"] {
            assert_eq!(
                lookups(command),
                vec![
                    ("env".to_string(), ProgramPathLookup::Captured),
                    ("sqlx".to_string(), ProgramPathLookup::Unverifiable),
                ],
                "{command:?}"
            );
        }
        assert_eq!(
            lookups("PATH=repo-bin env sqlx prepare"),
            vec![
                (
                    "env".to_string(),
                    ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
                ),
                (
                    "sqlx".to_string(),
                    ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
                ),
            ]
        );
        assert_eq!(
            lookups("exec -c env sqlx prepare"),
            vec![
                ("env".to_string(), ProgramPathLookup::Captured),
                ("sqlx".to_string(), ProgramPathLookup::Unverifiable),
            ]
        );
        assert_eq!(
            lookups("exec -c sqlx prepare"),
            vec![("sqlx".to_string(), ProgramPathLookup::Captured)]
        );
        assert_eq!(
            lookups("command -p env sqlx prepare"),
            vec![
                ("env".to_string(), ProgramPathLookup::Unverifiable),
                ("sqlx".to_string(), ProgramPathLookup::Captured),
            ]
        );

        assert_eq!(
            lookups("PATH=repo-bin sqlx prepare; cargo test"),
            vec![
                (
                    "sqlx".to_string(),
                    ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
                ),
                ("cargo".to_string(), ProgramPathLookup::Captured),
            ]
        );
        for command in [
            "PATH=repo-bin true; sqlx prepare",
            "PATH=repo-bin /bin/true; sqlx prepare",
            "printf '%s' PATH=repo-bin; sqlx prepare",
        ] {
            assert_eq!(
                lookups(command).last(),
                Some(&("sqlx".to_string(), ProgramPathLookup::Captured)),
                "{command:?}"
            );
        }
        assert_eq!(
            lookups("PATH=repo-bin; scripts/sqlx prepare").last(),
            Some(&("scripts/sqlx".to_string(), ProgramPathLookup::Explicit))
        );
    }

    #[test]
    fn command_programs_report_compound_command_executables() {
        assert_eq!(
            command_programs(Path::new("."), "cargo test && npm run build"),
            vec!["cargo", "npm"]
        );
        assert_eq!(
            command_programs(
                Path::new("."),
                "RUSTFLAGS=-Dwarnings cargo test; env NODE_ENV=test pnpm test"
            ),
            vec!["cargo", "env", "pnpm"]
        );
    }

    #[test]
    fn command_programs_skip_builtins_and_redirection_targets() {
        assert_eq!(
            command_programs(
                Path::new("."),
                "printf '%s\\n' skipped > /tmp/out && cargo test 2>&1"
            ),
            vec!["cargo"]
        );
        for command in [
            "cargo test &> /tmp/out",
            "cargo test &>>/tmp/out",
            "cargo test >| /tmp/out",
        ] {
            assert_eq!(command_programs(Path::new("."), command), vec!["cargo"]);
        }
        for command in [
            "'2'>/tmp/out cargo sqlx prepare -D sqlite:ignored.db",
            r"\2>/tmp/out cargo sqlx prepare -D sqlite:ignored.db",
            "''DATABASE_URL=sqlite:ignored.db cargo sqlx prepare",
            "'DATABASE_URL'=sqlite:ignored.db cargo sqlx prepare",
            "''! cargo sqlx prepare -D sqlite:ignored.db",
        ] {
            assert_ne!(
                command_program(command).as_deref(),
                Some("cargo"),
                "{command:?}"
            );
        }
        assert_eq!(
            command_program("DATABASE_URL='sqlite:actual.db' cargo sqlx prepare").as_deref(),
            Some("cargo")
        );
    }

    #[test]
    fn command_programs_skip_shell_block_closers() {
        assert_eq!(
            command_programs_for_shell(
                "for manifest in crates/*/Cargo.toml; do cargo test --manifest-path \"$manifest\"; done; if [ \"$found\" -eq 0 ]; then printf skipped; fi"
            ),
            vec!["cargo"]
        );
    }

    #[test]
    fn command_programs_follow_generated_optional_cargo_branch() {
        let temp = tempdir().unwrap();
        let command = format!(
            "{}cargo fetch{}printf '%s\\n' skipped{}",
            crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
            crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
            crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
        );

        assert!(command_programs(temp.path(), &command).is_empty());

        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        assert_eq!(command_programs(temp.path(), &command), vec!["cargo"]);
    }

    #[test]
    fn command_programs_require_cargo_sqlx_subcommand() {
        assert_eq!(
            command_programs_for_shell(
                "SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx cargo sqlx prepare --check"
            ),
            vec!["cargo"]
        );
        assert_eq!(
            command_programs_for_shell(
                "cargo +nightly --config net.git-fetch-with-cli=true sqlx prepare --check"
            ),
            vec!["cargo"]
        );
        assert_eq!(
            command_programs_for_shell("/opt/jig/bin/cargo sqlx prepare -D sqlite:doctor.db"),
            vec!["/opt/jig/bin/cargo"]
        );
        assert_eq!(
            command_programs_for_shell("/opt/jig/bin/sqlx prepare -D sqlite:doctor.db"),
            vec!["/opt/jig/bin/sqlx"]
        );
        assert_eq!(
            command_programs_for_shell("/opt/jig/bin/cargo-sqlx sqlx prepare -D sqlite:doctor.db"),
            vec!["/opt/jig/bin/cargo-sqlx"]
        );
        assert_eq!(
            command_programs_for_shell("command -- cargo sqlx prepare -D sqlite:doctor.db"),
            vec!["cargo"]
        );
        assert_eq!(
            command_programs_for_shell("exec -- sqlx prepare -D sqlite:doctor.db"),
            vec!["sqlx"]
        );
        assert_eq!(
            command_programs_for_shell("nohup -- cargo-sqlx sqlx prepare -D sqlite:doctor.db"),
            vec!["nohup", "cargo-sqlx"]
        );
        assert_eq!(
            command_programs_for_shell(
                "command -v cargo >/dev/null && cargo sqlx prepare -D sqlite:doctor.db"
            ),
            vec!["cargo"]
        );
        assert_eq!(
            command_programs_for_shell("command -p cargo sqlx prepare -D sqlite:doctor.db"),
            vec!["cargo"]
        );
        assert_eq!(
            command_programs_for_shell("exec -a jig-cargo cargo sqlx prepare -D sqlite:doctor.db"),
            vec!["cargo"]
        );
        assert_eq!(
            command_programs_for_shell("exec -c cargo sqlx prepare -D sqlite:doctor.db"),
            vec!["cargo"]
        );
        assert!(
            command_programs_for_shell("exec -z cargo sqlx prepare -D sqlite:doctor.db").is_empty()
        );
        assert!(!cargo_sqlx_command_has_inline_config(
            "cargo --config net.git-fetch-with-cli=true sqlx prepare -D sqlite:doctor.db"
        ));
        assert!(cargo_sqlx_command_has_inline_config(
            "cargo --config alias.sqlx='run --package fake' sqlx prepare -D sqlite:doctor.db"
        ));
        assert!(cargo_sqlx_command_has_inline_config(
            "cargo --config include='dispatch.toml' sqlx prepare -D sqlite:doctor.db"
        ));
    }

    #[test]
    fn cargo_dispatch_detects_command_local_alias_and_home_environment_changes() {
        for command in [
            "CARGO_ALIAS_SQLX='run --package fake' cargo sqlx prepare",
            "CARGO_HOME=/tmp/cargo-home cargo sqlx prepare",
            "CARGO_HOME[0]=/tmp/cargo-home cargo sqlx prepare",
            "env HOME=/tmp/home cargo sqlx prepare",
            "env -u USERPROFILE cargo sqlx prepare",
            "env --unset=HOMEDRIVE cargo sqlx prepare",
            "env -i cargo sqlx prepare",
            "export CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
            "declare -x CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
            "builtin declare -x CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
            "declare -x CARGO_ALIAS_SQLX='run --package fake'; cargo sqlx prepare",
            "read CARGO_HOME <<< /tmp/cargo-home; cargo sqlx prepare",
            "printf -v CARGO_HOME %s /tmp/cargo-home; cargo sqlx prepare",
            "printf -v 'CARGO_HOME[0]' %s /tmp/cargo-home; cargo sqlx prepare",
            "mapfile -t CARGO_HOME </dev/null; cargo sqlx prepare",
            "getopts p CARGO_HOME; cargo sqlx prepare",
            "let CARGO_HOME=0; cargo sqlx prepare",
            "declare -n cargo_home_ref=CARGO_HOME; cargo sqlx prepare",
            "unset HOMEPATH; cargo sqlx prepare",
            "HOME=/tmp/home; cargo sqlx prepare",
            "exec -c cargo sqlx prepare",
        ] {
            assert!(
                cargo_sqlx_command_changes_dispatch_environment(command),
                "{command:?}",
            );
        }

        for command in [
            "OTHER_VALUE=/tmp cargo sqlx prepare",
            "env -u OTHER_VALUE cargo sqlx prepare",
            "printf '%s' HOME=/tmp/home; cargo sqlx prepare",
            "env export CARGO_HOME=/tmp/home; cargo sqlx prepare",
        ] {
            assert!(
                !cargo_sqlx_command_changes_dispatch_environment(command),
                "{command:?}",
            );
        }
    }

    #[test]
    fn sqlx_driver_discovery_honors_command_environment_and_dotenv_precedence() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join(".env"),
            "DATABASE_URL=postgres://user:private-password@localhost/demo\n",
        )
        .unwrap();
        fs::write(
            temp.path().join(".env.example"),
            "DATABASE_URL=sqlite:example.db\n",
        )
        .unwrap();

        assert_eq!(
            configured_sqlx_driver(temp.path(), "cargo sqlx prepare --check", None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::Dotenv,
            })
        );

        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --check",
                Some(OsStr::new("sqlite:environment.db")),
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::Environment,
            })
        );
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL=sqlite:assignment.db cargo sqlx prepare --check --database-url postgres://flag-user:flag-password@localhost/demo",
                Some(OsStr::new("sqlite:environment.db")),
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::CommandFlag,
            })
        );
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "env DATABASE_URL=postgres://assignment-user:assignment-password@localhost/demo cargo sqlx prepare --check",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::CommandAssignment,
            })
        );
        for command in [
            "! DATABASE_URL=sqlite:negated.db sqlx prepare --check",
            "! env -i DATABASE_URL=sqlite:negated-env.db sqlx prepare --check",
        ] {
            assert_eq!(
                configured_sqlx_driver(
                    temp.path(),
                    command,
                    Some(OsStr::new("postgres://localhost/ambient")),
                ),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Sqlite,
                    source: SqlxDriverSource::CommandAssignment,
                }),
                "{command:?}",
            );
        }
        for command in [
            "command env DATABASE_URL=postgres://localhost/command-env cargo sqlx prepare --check",
            "builtin command env DATABASE_URL=postgres://localhost/builtin-command-env cargo sqlx prepare --check",
            "exec env DATABASE_URL=postgres://localhost/exec-env cargo sqlx prepare --check",
            "env nohup env DATABASE_URL=postgres://localhost/nested-env cargo sqlx prepare --check",
        ] {
            assert_eq!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Postgres,
                    source: SqlxDriverSource::CommandAssignment,
                }),
                "{command:?}",
            );
        }
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --check --database-url=postgres://flag-user:flag-password@localhost/demo",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::CommandFlag,
            })
        );

        fs::remove_file(temp.path().join(".env")).unwrap();
        assert_eq!(
            configured_sqlx_driver(temp.path(), "cargo sqlx prepare --check", None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::DotenvExample,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_does_not_cross_post_assignment_keywords() {
        let temp = tempdir().unwrap();
        for command in [
            "DATABASE_URL=sqlite:x ! sqlx prepare",
            "! DATABASE_URL=sqlite:x ! sqlx prepare",
            "DATABASE_URL=sqlite:x then sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, None),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }

        assert_eq!(
            configured_sqlx_driver(temp.path(), "! DATABASE_URL=sqlite:x sqlx prepare", None,),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandAssignment,
            }),
        );
        for command in [
            "''! DATABASE_URL=sqlite:x sqlx prepare",
            r"\! DATABASE_URL=sqlite:x sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, None),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }
    }

    #[test]
    fn sqlx_driver_discovery_stops_at_the_nearest_existing_dotenv() {
        let temp = tempdir().unwrap();
        let child = temp.path().join("crates/api");
        fs::create_dir_all(&child).unwrap();
        fs::write(
            temp.path().join(".env"),
            "DATABASE_URL=postgres://parent-user:parent-secret@localhost/demo\n",
        )
        .unwrap();
        fs::write(
            temp.path().join(".env.example"),
            "DATABASE_URL=sqlite:example-secret.db\n",
        )
        .unwrap();

        for contents in ["UNRELATED_SECRET=child-secret\n", "\n"] {
            fs::write(child.join(".env"), contents).unwrap();
            assert_eq!(
                configured_sqlx_driver_fallback(temp.path(), &child, None, false),
                SqlxDriverResolution::Absent,
                "nearest dotenv contents were {contents:?}",
            );
        }

        fs::write(child.join(".env"), "DATABASE_URL='unterminated\n").unwrap();
        assert!(matches!(
            configured_sqlx_driver_fallback(temp.path(), &child, None, false),
            SqlxDriverResolution::Indeterminate(reason)
                if reason == "a dotenv file could not be parsed safely"
        ));

        fs::remove_file(child.join(".env")).unwrap();
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), &child, None, false),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::Dotenv,
            })
        );

        fs::remove_file(temp.path().join(".env")).unwrap();
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), &child, None, false),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::DotenvExample,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_fails_open_for_dynamic_and_ambiguous_commands() {
        let temp = tempdir().unwrap();
        let ambient = Some(OsStr::new("sqlite:environment.db"));

        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --database-url '$OTHER_DATABASE_URL'",
                ambient,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --database-url '$DATABASE_URL'",
                ambient,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                r#"cargo sqlx prepare --database-url \$DATABASE_URL"#,
                ambient,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --database-url \"$DATABASE_URL\"",
                ambient,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
        for command in [
            "cargo sqlx prepare --database-url $DATABASE_URL",
            "cargo sqlx prepare --database-url=$DATABASE_URL",
            "cargo sqlx prepare -D$DATABASE_URL",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, ambient),
                    SqlxDriverResolution::Indeterminate(reason)
                        if reason.contains("unquoted DATABASE_URL")
                ),
                "{command:?}",
            );
        }
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL=$DATABASE_URL cargo sqlx prepare",
                ambient,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandAssignment,
            })
        );
        fs::write(
            temp.path().join(".env"),
            "DATABASE_URL=postgres://dotenv-must-not-be-used/doctor\n",
        )
        .unwrap();
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --database-url '$DATABASE_URL'",
                None,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL=sqlite:first.db cargo sqlx prepare && printf ignored && cargo sqlx migrate info --database-url=sqlite:second.db",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandAssignment,
            })
        );
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL=sqlite:first.db cargo sqlx prepare && cargo sqlx migrate info --database-url=postgres://localhost/second",
                None,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "export DATABASE_URL=postgres://localhost/demo && cargo sqlx prepare",
                None,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL=postgres://localhost/demo && cargo sqlx prepare",
                None,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "env -u DATABASE_URL cargo sqlx prepare",
                ambient,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(temp.path(), "env - cargo sqlx prepare", ambient,),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare --database-url='postgres://localhost/demo",
                None,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "printf DATABASE_URL=postgres://ignored && DATABASE_URL=sqlite:actual.db cargo sqlx prepare",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandAssignment,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_supports_cli_variants_short_flags_and_continuations() {
        let temp = tempdir().unwrap();
        for command in [
            "cargo sqlx prepare -D sqlite:first.db",
            "command /opt/bin/cargo sqlx prepare -Dsqlite:second.db",
            "exec sqlx prepare -D=sqlite:third.db",
            "nohup cargo-sqlx sqlx prepare --database-url sqlite:fourth.db",
            "cargo \\\n             sqlx prepare -D \\\r\n             sqlite:fifth.db",
            "command -- cargo sqlx prepare -D sqlite:sixth.db",
            "exec -- sqlx prepare -D sqlite:seventh.db",
            "nohup -- cargo-sqlx sqlx prepare -D sqlite:eighth.db",
        ] {
            assert_eq!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Sqlite,
                    source: SqlxDriverSource::CommandFlag,
                }),
                "{command:?}",
            );
        }
    }

    #[test]
    fn sqlx_driver_discovery_ignores_comments_and_preserves_literal_hashes() {
        let temp = tempdir().unwrap();
        let ambient = Some(OsStr::new("sqlite:environment.db"));

        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare # -D postgres://doctor-user:comment-secret@localhost/demo",
                ambient,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::Environment,
            })
        );
        for command in [
            "cargo sqlx prepare -D sqlite:doctor.db#in-word",
            r"cargo sqlx prepare -D sqlite:doctor.db\#escaped",
            "cargo sqlx prepare -D 'sqlite:doctor.db#quoted'",
        ] {
            assert_eq!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Sqlite,
                    source: SqlxDriverSource::CommandFlag,
                }),
                "{command:?}",
            );
        }
    }

    #[test]
    fn shell_parser_preserves_empty_quote_word_boundaries_before_hashes() {
        assert_eq!(command_programs_for_shell("''#foo"), vec!["#foo"]);
        assert_eq!(
            command_programs_for_shell("# ignored\ncargo test"),
            vec!["cargo"]
        );
        assert!(command_programs_for_shell("'' cargo sqlx prepare -D sqlite:wrong.db").is_empty());

        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "'' cargo sqlx prepare -D sqlite:wrong.db",
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(OsString::new()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert!(check.detail.contains("scripts/jig check sqlx"));
    }

    #[test]
    fn shell_parser_preserves_assignment_name_and_io_number_provenance() {
        let temp = tempdir().unwrap();
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL='sqlite:actual.db' cargo sqlx prepare",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandAssignment,
            })
        );
        for command in [
            "'2'>out cargo sqlx prepare -D sqlite:ignored.db",
            r"\2>out cargo sqlx prepare -D sqlite:ignored.db",
            "''DATABASE_URL=sqlite:ignored.db cargo sqlx prepare",
            "'DATABASE_URL'=sqlite:ignored.db cargo sqlx prepare",
            "''! cargo sqlx prepare -D sqlite:ignored.db",
            "''! DATABASE_URL=sqlite:ignored.db sqlx prepare",
            r"\! DATABASE_URL=sqlite:ignored.db sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, None),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }
    }

    #[test]
    fn shell_parser_ignores_heredoc_bodies() {
        let temp = tempdir().unwrap();
        let unquoted_substitution = "cat <<EOF\n$(missing-helper)\nEOF";
        let unquoted = required_command_programs_for_shell(unquoted_substitution);
        assert_eq!(
            unquoted.ambiguity,
            Some(RequiredProgramAmbiguity::ShellSyntax)
        );
        assert_eq!(unquoted.programs[0].program, "cat");
        assert_eq!(
            unquoted.programs[0].path_lookup,
            ProgramPathLookup::Unverifiable
        );
        assert!(matches!(
            configured_sqlx_driver(temp.path(), unquoted_substitution, None),
            SqlxDriverResolution::Indeterminate(_)
        ));

        for inert in [
            "cat <<'EOF'\n$(missing-helper)\nEOF",
            "cat <<\\EOF\n$(missing-helper)\nEOF",
            "cat <<EOF\n\\$(missing-helper)\nEOF",
        ] {
            let discovery = required_command_programs_for_shell(inert);
            assert!(discovery.ambiguity.is_none(), "{inert:?}");
            assert_eq!(discovery.programs[0].program, "cat", "{inert:?}");
            assert_eq!(
                discovery.programs[0].path_lookup,
                ProgramPathLookup::Captured,
                "{inert:?}",
            );
        }

        let command = "cat <<'PAYLOAD'\nDATABASE_URL=postgres://body-secret cargo sqlx prepare -D postgres://body-secret\nPAYLOAD\ncargo sqlx prepare -D sqlite:actual.db";

        assert_eq!(command_programs_for_shell(command), vec!["cat", "cargo"]);
        assert_eq!(
            configured_sqlx_driver(temp.path(), command, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );

        let tab_stripped = "cat <<-EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db";
        assert_eq!(
            command_programs_for_shell(tab_stripped),
            vec!["cat", "sqlx"]
        );
        assert_eq!(
            configured_sqlx_driver(temp.path(), tab_stripped, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );

        for tab_stripped in [
            "cat <<- EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db",
            "cat <<-EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db",
        ] {
            assert_eq!(
                command_programs_for_shell(tab_stripped),
                vec!["cat", "sqlx"]
            );
            assert_eq!(
                configured_sqlx_driver(temp.path(), tab_stripped, None),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Sqlite,
                    source: SqlxDriverSource::CommandFlag,
                })
            );
        }

        let multiple_crlf = "cat <<ONE <<-'TWO'\r\ncargo sqlx prepare -D postgres://first\r\nONE\r\n\tcargo sqlx prepare -D postgres://second\r\n\tTWO\r\nsqlx prepare -D sqlite:actual.db";
        assert_eq!(
            command_programs_for_shell(multiple_crlf),
            vec!["cat", "sqlx"]
        );
        assert_eq!(
            configured_sqlx_driver(temp.path(), multiple_crlf, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );

        let unterminated = "cat <<EOF\ncargo sqlx prepare -D postgres://body-secret";
        assert_eq!(command_programs_for_shell(unterminated), vec!["cat"]);
        assert!(matches!(
            configured_sqlx_driver(temp.path(), unterminated, None),
            SqlxDriverResolution::Indeterminate(_)
        ));
    }

    #[test]
    fn shell_words_preserve_literal_edge_quotes_and_semicolons() {
        assert_eq!(
            command_programs_for_shell(r#"'"sqlx"' prepare -D sqlite:ignored.db"#),
            vec!["\"sqlx\""]
        );
        assert_eq!(
            command_programs_for_shell("'sqlx;' prepare -D sqlite:ignored.db"),
            vec!["sqlx;"]
        );
        assert_eq!(
            command_programs_for_shell("'sqlx' prepare -D sqlite:actual.db"),
            vec!["sqlx"]
        );
    }

    #[test]
    fn sqlx_driver_discovery_detects_later_database_url_mutations() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:root.db\n").unwrap();

        for command in [
            "FOO=one DATABASE_URL=postgres://localhost/assignment && cargo sqlx prepare",
            "DATABASE_URL[0]=postgres://localhost/array-assignment && cargo sqlx prepare",
            "FOO=one export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
            "command export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
            "builtin export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
            "builtin command export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
            "declare -x DATABASE_URL=postgres://localhost/declared && cargo sqlx prepare",
            "builtin declare -x DATABASE_URL=postgres://localhost/declared && cargo sqlx prepare",
            "read DATABASE_URL <<< postgres://localhost/read && cargo sqlx prepare",
            "builtin read DATABASE_URL </dev/null && cargo sqlx prepare",
            "printf -v DATABASE_URL %s postgres://localhost/printf && cargo sqlx prepare",
            "builtin printf -v DATABASE_URL %s postgres://localhost/printf && cargo sqlx prepare",
            "declare -n database_ref=DATABASE_URL && cargo sqlx prepare",
            "printf -v 'DATABASE_URL[0]' %s postgres://localhost/array-printf && cargo sqlx prepare",
            "printf '-vDATABASE_URL[0]' %s postgres://localhost/array-printf && cargo sqlx prepare",
            "read 'DATABASE_URL[0]' </dev/null && cargo sqlx prepare",
            "read '-aDATABASE_URL[0]' </dev/null && cargo sqlx prepare",
            "declare 'DATABASE_URL[0]=postgres://localhost/array-declare' && cargo sqlx prepare",
            "unset 'DATABASE_URL[0]' && cargo sqlx prepare",
            "mapfile -t DATABASE_URL </dev/null && cargo sqlx prepare",
            "readarray DATABASE_URL </dev/null && cargo sqlx prepare",
            "set -- -p; getopts p DATABASE_URL; cargo sqlx prepare",
            "let DATABASE_URL=0; cargo sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, None),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "printf '%s' DATABASE_URL=postgres://ignored && cargo sqlx prepare -D sqlite:actual.db",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "PATH=./repo-tools cargo sqlx prepare -D sqlite:actual.db",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_respects_database_url_scrub_order() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:dotenv.db\n").unwrap();
        let ambient = Some(OsStr::new("sqlite:ambient.db"));

        for command in [
            "DATABASE_URL=postgres://localhost/before env -i cargo sqlx prepare",
            "DATABASE_URL=postgres://localhost/before env --ignore-environment cargo sqlx prepare",
            "DATABASE_URL=postgres://localhost/before env -u DATABASE_URL cargo sqlx prepare",
            "DATABASE_URL=postgres://localhost/before env -uDATABASE_URL cargo sqlx prepare",
            "DATABASE_URL=postgres://localhost/before env --unset DATABASE_URL cargo sqlx prepare",
            "DATABASE_URL=postgres://localhost/before env --unset=DATABASE_URL cargo sqlx prepare",
            "DATABASE_URL=postgres://localhost/before exec -c cargo sqlx prepare",
            "DATABASE_URL+=postgres://localhost/appended cargo sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, ambient),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }

        for command in [
            "env -i DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
            "DATABASE_URL=sqlite:before.db env -i DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
            "DATABASE_URL=sqlite:before.db exec -c env DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
            "DATABASE_URL+=ignored DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
        ] {
            assert_eq!(
                configured_sqlx_driver(temp.path(), command, ambient),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Postgres,
                    source: SqlxDriverSource::CommandAssignment,
                }),
                "{command:?}",
            );
        }

        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "DATABASE_URL=postgres://localhost/before env -i cargo sqlx prepare -D sqlite:explicit.db",
                ambient,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_does_not_treat_wrapper_operands_as_assignments() {
        let temp = tempdir().unwrap();
        let ambient = Some(OsStr::new("postgres://localhost/ambient"));

        for command in [
            "env -C DATABASE_URL=sqlite:option.db cargo sqlx prepare",
            "env --chdir DATABASE_URL=sqlite:option.db cargo sqlx prepare",
            "command DATABASE_URL=sqlite:target.db cargo sqlx prepare",
            "exec DATABASE_URL=sqlite:target.db cargo sqlx prepare",
            "nohup DATABASE_URL=sqlite:target.db cargo sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, ambient),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }
    }

    #[test]
    fn sqlx_driver_discovery_rejects_mixed_known_and_absent_requirements() {
        let temp = tempdir().unwrap();
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "cargo sqlx prepare -D sqlite:known.db && cargo sqlx prepare --no-dotenv",
                None,
            ),
            SqlxDriverResolution::Indeterminate(reason)
                if reason.contains("some SQLx invocations")
        ));
    }

    #[test]
    fn sqlx_driver_discovery_models_wrapper_options_conservatively() {
        let temp = tempdir().unwrap();
        let ambient = Some(OsStr::new("sqlite:environment.db"));

        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "command -v cargo >/dev/null && cargo sqlx prepare -D sqlite:checked.db",
                ambient,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
        for command in [
            "command -p cargo sqlx prepare -D sqlite:default-path.db",
            "exec -a jig-cargo cargo sqlx prepare -D sqlite:custom-argv-zero.db",
            "exec -z cargo sqlx prepare -D sqlite:unsupported-option.db",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, ambient),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }
        assert!(matches!(
            configured_sqlx_driver(temp.path(), "exec -c cargo sqlx prepare", ambient,),
            SqlxDriverResolution::Indeterminate(_)
        ));
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "exec -c cargo sqlx prepare -D sqlite:explicit.db",
                ambient,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_uses_literal_cd_dotenv_and_rejects_unsafe_cwd() {
        let temp = tempdir().unwrap();
        let child = temp.path().join("crates/api");
        fs::create_dir_all(&child).unwrap();
        fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:root.db\n").unwrap();
        fs::write(
            child.join(".env"),
            "DATABASE_URL=postgres://localhost/child\n",
        )
        .unwrap();
        let parsed = parse_shell_commands("cd crates/api && cargo sqlx prepare");
        assert_eq!(parsed.separators.first(), Some(&ShellSeparator::And));
        let resolved = resolve_literal_cd(temp.path(), temp.path(), &parsed.commands[0]).unwrap();
        assert_eq!(resolved, fs::canonicalize(&child).unwrap());
        assert!(
            database_url_from_dotenv(&resolved.join(".env"))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), &resolved, None, false),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::Dotenv,
            })
        );

        assert_eq!(
            configured_sqlx_driver(temp.path(), "cd crates/api && cargo sqlx prepare", None,),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::Dotenv,
            })
        );
        for command in [
            "cd $APP_DIR && cargo sqlx prepare",
            "cd crates/api; cargo sqlx prepare",
            "CDPATH= cd crates/api && cargo sqlx prepare",
            "command cd crates/api && cargo sqlx prepare",
            "env -C crates/api cargo sqlx prepare",
            "env -Ccrates/api cargo sqlx prepare",
            "cargo -Ccrates/api sqlx prepare",
        ] {
            assert!(matches!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Indeterminate(_)
            ));
        }
        assert!(matches!(
            configured_sqlx_driver(
                temp.path(),
                "cd crates/api && cargo sqlx prepare --no-dotenv",
                None,
            ),
            SqlxDriverResolution::Indeterminate(_)
        ));
    }

    #[test]
    fn sqlx_driver_discovery_sees_parent_dotenv_before_local_example() {
        let outer = tempdir().unwrap();
        let root = outer.path().join("repo");
        fs::create_dir(&root).unwrap();
        fs::write(
            outer.path().join(".env"),
            "DATABASE_URL=postgres://localhost/parent\n",
        )
        .unwrap();
        fs::write(
            root.join(".env.example"),
            "DATABASE_URL=sqlite:local-example.db\n",
        )
        .unwrap();

        assert!(matches!(
            configured_sqlx_driver_fallback(&root, &root, None, false),
            SqlxDriverResolution::Indeterminate(reason)
                if reason.contains("above the Jig repository")
        ));
    }

    #[test]
    fn sqlx_driver_discovery_accepts_bom_prefixed_dotenv_hints() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join(".env"),
            b"\xef\xbb\xbfDATABASE_URL=postgres://localhost/bom\n",
        )
        .unwrap();
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::Dotenv,
            })
        );

        fs::remove_file(temp.path().join(".env")).unwrap();
        fs::write(
            temp.path().join(".env.example"),
            b"\xef\xbb\xbfDATABASE_URL=sqlite:bom.db\n",
        )
        .unwrap();
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::DotenvExample,
            })
        );
    }

    #[test]
    fn sqlx_driver_discovery_rejects_dotenv_substitution_without_using_ambient_helpers() {
        let _env = lock_env();
        let helper = "JIG_DOCTOR_DOTENV_HELPER_DO_NOT_LEAK";
        let secret = "ambient-substitution-secret";
        let _helper = EnvVarGuard::set(helper, secret);
        let temp = tempdir().unwrap();

        fs::write(
            temp.path().join(".env"),
            format!("DATABASE_URL=${helper}:private.db\n"),
        )
        .unwrap();
        let dotenv = configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false);
        assert!(matches!(
            dotenv,
            SqlxDriverResolution::Indeterminate(reason)
                if reason.contains("variable substitution")
        ));

        fs::remove_file(temp.path().join(".env")).unwrap();
        fs::write(
            temp.path().join(".env.example"),
            format!("DATABASE_URL=${{{helper}}}:private.db\n"),
        )
        .unwrap();
        let example = configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false);
        assert!(matches!(
            example,
            SqlxDriverResolution::Indeterminate(reason)
                if reason.contains("variable substitution")
        ));

        for resolution in [dotenv, example] {
            assert!(!format!("{resolution:?}").contains(secret));
        }
    }

    #[test]
    fn sqlx_driver_discovery_preserves_literal_dotenv_dollars() {
        let temp = tempdir().unwrap();
        for value in ["'sqlite:literal-$HELPER.db'", "sqlite:escaped-\\$HELPER.db"] {
            fs::write(temp.path().join(".env"), format!("DATABASE_URL={value}\n")).unwrap();
            assert_eq!(
                configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false),
                SqlxDriverResolution::Known(SqlxDriverRequirement {
                    driver: SqlxDriver::Sqlite,
                    source: SqlxDriverSource::Dotenv,
                }),
                "{value:?}",
            );
        }
    }

    #[test]
    fn dotenv_database_url_uses_the_platform_key_matching_rule() {
        assert!(dotenv_database_url_key("DATABASE_URL"));
        assert_eq!(dotenv_database_url_key("database_url"), cfg!(windows));

        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join(".env"),
            "database_url=sqlite:first.db\nDATABASE_URL=postgres://localhost/second\n",
        )
        .unwrap();
        let database_url = database_url_from_dotenv(&temp.path().join(".env")).unwrap();
        #[cfg(windows)]
        assert_eq!(
            database_url,
            Some(DotenvDatabaseUrl::Literal("sqlite:first.db".into()))
        );
        #[cfg(not(windows))]
        assert_eq!(
            database_url,
            Some(DotenvDatabaseUrl::Literal(
                "postgres://localhost/second".into()
            ))
        );
    }

    #[test]
    fn sqlx_driver_discovery_models_guarded_and_pipelined_cd_safely() {
        let temp = tempdir().unwrap();
        let child = temp.path().join("crates/api");
        fs::create_dir_all(&child).unwrap();
        fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:root.db\n").unwrap();
        fs::write(
            child.join(".env"),
            "DATABASE_URL=postgres://localhost/child\n",
        )
        .unwrap();

        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                "cd crates/api || exit 1; cargo sqlx prepare",
                None,
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::Dotenv,
            })
        );
        for command in [
            "printf x | cd crates/api && cargo sqlx prepare",
            "true || cd crates/api && cargo sqlx prepare",
            "false && cd crates/api; cargo sqlx prepare",
        ] {
            assert!(
                matches!(
                    configured_sqlx_driver(temp.path(), command, None),
                    SqlxDriverResolution::Indeterminate(_)
                ),
                "{command:?}",
            );
        }
    }

    #[test]
    fn sqlx_driver_discovery_normalizes_postgresql_alias() {
        assert_eq!(
            SqlxDriver::from_database_url("postgresql://localhost/demo"),
            Some(SqlxDriver::Postgres)
        );
        assert_eq!(
            SqlxDriver::from_database_url("SQLITE:demo.db"),
            Some(SqlxDriver::Sqlite)
        );
        assert_eq!(
            SqlxDriver::from_database_url("mysql://localhost/demo"),
            None
        );
    }

    #[test]
    fn windows_executable_candidates_honor_validated_pathext_order() {
        let candidates = windows_executable_candidates(
            Path::new("/tools/cargo"),
            Some(OsStr::new(".exe;.CMD;../BAD;.Exe;.PS1")),
        );
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/tools/cargo"),
                PathBuf::from("/tools/cargo.EXE"),
                PathBuf::from("/tools/cargo.CMD"),
                PathBuf::from("/tools/cargo.PS1"),
            ]
        );
        assert_eq!(
            windows_search_path_executable_candidates(
                Path::new("/tools/cargo"),
                Some(OsStr::new(".exe;.CMD;../BAD;.Exe;.PS1")),
            ),
            vec![
                PathBuf::from("/tools/cargo.EXE"),
                PathBuf::from("/tools/cargo.CMD"),
                PathBuf::from("/tools/cargo.PS1"),
                PathBuf::from("/tools/cargo"),
            ]
        );
        assert_eq!(
            validated_windows_path_extensions(None),
            vec![".COM", ".EXE", ".BAT", ".CMD"]
        );
        assert!(executable_is_named("SQLX.ExE", "sqlx"));
        assert!(executable_is_named("cargo-sqlx.EXE", "cargo-sqlx"));
    }

    #[test]
    fn sqlx_driver_probe_classifies_supported_and_missing_drivers() {
        assert_eq!(
            classify_sqlx_driver_probe(SqlxDriver::Sqlite, true, "", ""),
            SqlxDriverProbe::Compatible
        );
        assert_eq!(
            classify_sqlx_driver_probe(
                SqlxDriver::Postgres,
                false,
                "error: unknown value \"jig-doctor-invalid\" for `ssl_mode`",
                ""
            ),
            SqlxDriverProbe::Compatible
        );
        assert_eq!(
            classify_sqlx_driver_probe(
                SqlxDriver::Postgres,
                false,
                "error: invalid value 'jig-doctor-invalid' for sslmode",
                ""
            ),
            SqlxDriverProbe::Compatible
        );
        assert_eq!(
            classify_sqlx_driver_probe(
                SqlxDriver::Sqlite,
                false,
                "",
                "error: error with configuration: no driver found for URL scheme \"sqlite\""
            ),
            SqlxDriverProbe::Incompatible
        );
        assert!(matches!(
            classify_sqlx_driver_probe(SqlxDriver::Sqlite, false, "error: unexpected argument", ""),
            SqlxDriverProbe::Indeterminate(_)
        ));
        for (stdout, stderr) in [
            ("error: invalid value for ssl_mode", ""),
            ("error: invalid value 'jig-doctor-invalid'", ""),
            ("jig-doctor-invalid", "error: invalid value for ssl_mode"),
        ] {
            assert!(matches!(
                classify_sqlx_driver_probe(SqlxDriver::Postgres, false, stdout, stderr,),
                SqlxDriverProbe::Indeterminate(_)
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_driver_probe_invokes_shim_safely_and_times_out() {
        let _env = lock_env();
        let _secret = EnvVarGuard::set("JIG_DOCTOR_TEST_SECRET", "must-not-be-inherited");
        let _database_url = EnvVarGuard::set("DATABASE_URL", "postgres://must-not-be-inherited");
        let temp = tempdir().unwrap();
        let supported = temp.path().join("cargo-sqlx-supported");
        write_test_executable(
            &supported,
            "#!/bin/sh\n[ -z \"${JIG_DOCTOR_TEST_SECRET+x}\" ] || exit 8\n[ -z \"${DATABASE_URL+x}\" ] || exit 8\n[ \"$HOME\" = \"$USERPROFILE\" ] || exit 8\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 8\n[ \"$HOME\" = \"$TMP\" ] || exit 8\n[ \"$HOME\" = \"$TEMP\" ] || exit 8\n[ \"$LC_ALL\" = C ] || exit 8\n[ \"$NO_COLOR\" = 1 ] || exit 8\n[ \"$1\" = sqlx ] || exit 9\nprintf '%s\\n' 'error: unknown value \"jig-doctor-invalid\" for ssl_mode'\nexit 1\n",
        );
        assert_eq!(
            probe_sqlx_driver_with_timeout(
                &supported,
                SqlxProbeStyle::CargoSubcommand,
                SqlxDriver::Postgres,
                Duration::from_secs(1)
            ),
            SqlxDriverProbe::Compatible
        );

        let direct = temp.path().join("sqlx-supported");
        write_test_executable(
            &direct,
            "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nexit 0\n",
        );
        assert_eq!(
            probe_sqlx_driver_with_timeout(
                &direct,
                SqlxProbeStyle::Direct,
                SqlxDriver::Sqlite,
                Duration::from_secs(1),
            ),
            SqlxDriverProbe::Compatible
        );

        let repo = tempdir().unwrap();
        let tools = tempdir().unwrap();
        let unrelated = tempdir().unwrap();
        let tools = fs::canonicalize(tools.path()).unwrap();
        let path_limited = tools.join("sqlx-path-limited");
        write_test_executable(
            &path_limited,
            &format!(
                "#!/bin/sh\n[ \"$PATH\" = '{}' ] || exit 8\nexit 0\n",
                tools.display()
            ),
        );
        let broad_path = env::join_paths([tools.as_path(), unrelated.path()]).unwrap();
        assert_eq!(
            probe_sqlx_driver_with_timeout_and_environment(
                &path_limited,
                SqlxProbeStyle::Direct,
                SqlxDriver::Sqlite,
                Duration::from_secs(1),
                repo.path(),
                &DoctorEnvironment {
                    search_path: Some(broad_path),
                    ..DoctorEnvironment::default()
                },
            ),
            SqlxDriverProbe::Compatible
        );

        let hanging = temp.path().join("cargo-sqlx-hanging");
        write_test_executable(&hanging, "#!/bin/sh\nwhile :; do :; done\n");
        assert!(matches!(
            probe_sqlx_driver_with_timeout(
                &hanging,
                SqlxProbeStyle::CargoSubcommand,
                SqlxDriver::Sqlite,
                Duration::from_millis(20)
            ),
            SqlxDriverProbe::Indeterminate(reason) if reason.contains("timed out")
        ));

        let noisy = temp.path().join("cargo-sqlx-noisy");
        write_test_executable(
            &noisy,
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nexit 0\n",
        );
        assert!(matches!(
            probe_sqlx_driver_with_timeout(
                &noisy,
                SqlxProbeStyle::CargoSubcommand,
                SqlxDriver::Sqlite,
                Duration::from_secs(2)
            ),
            SqlxDriverProbe::Indeterminate(reason) if reason.contains("capture limit")
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sqlx_driver_probe_reaps_descendants_on_completion_timeout_and_wait_failure() {
        let temp = tempdir().unwrap();

        let completed_marker = temp.path().join("completed-descendant");
        let completed = temp.path().join("sqlx-completed");
        write_test_executable(
            &completed,
            &owned_test_descendant_script(&completed_marker, "exit 0"),
        );
        assert_eq!(
            probe_sqlx_driver_with_timeout(
                &completed,
                SqlxProbeStyle::Direct,
                SqlxDriver::Sqlite,
                Duration::from_secs(2),
            ),
            SqlxDriverProbe::Compatible
        );
        let completed_descendant = read_test_process_identity(&completed_marker);

        let timeout_marker = temp.path().join("timeout-descendant");
        let hanging = temp.path().join("sqlx-timeout-tree");
        write_test_executable(
            &hanging,
            &owned_test_descendant_script(&timeout_marker, "while :; do :; done"),
        );
        assert!(matches!(
            probe_sqlx_driver_with_timeout(
                &hanging,
                SqlxProbeStyle::Direct,
                SqlxDriver::Sqlite,
                Duration::from_millis(300),
            ),
            SqlxDriverProbe::Indeterminate(reason) if reason == "the driver probe timed out"
        ));
        let timeout_descendant = read_test_process_identity(&timeout_marker);

        let failure_marker = temp.path().join("failure-descendant");
        let await_failure = temp.path().join("sqlx-await-failure-tree");
        write_test_executable(
            &await_failure,
            &owned_test_descendant_script(&failure_marker, "while :; do :; done"),
        );
        let mut command = Command::new(&await_failure);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_probe_process(&mut command).unwrap();
        let failure_descendant = read_test_process_identity(&failure_marker);
        assert!(matches!(
            finish_probe_wait(
                &mut process,
                Err(std::io::Error::other("injected wait failure")),
            ),
            Err(OwnedProcessTreeError::Await)
        ));

        for descendant in [completed_descendant, timeout_descendant, failure_descendant] {
            assert_test_process_stopped(&descendant);
        }
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_driver_probe_sigint_helper() {
        let Some(executable) = std::env::var_os("JIG_SQLX_PROBE_SIGINT_HELPER") else {
            return;
        };
        let signal_session = DoctorSignalSession::start().unwrap();
        let cancelled = || signal_session.cancelled();
        let result = probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
            Path::new(&executable),
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(30),
            Path::new("/"),
            &DoctorEnvironment::default(),
            Some(&cancelled),
        );
        let _ = finish_doctor_signal_session(signal_session);
        panic!("SIGINT was not re-delivered after probe cleanup: {result:?}");
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_probe_signal_finish_fails_closed_when_restoration_fails() {
        let signals = DoctorSignals {
            first: Some(libc::SIGINT),
            mask: doctor_signal_bit(libc::SIGINT),
        };
        assert_eq!(
            doctor_signal_finish_action(signals, true),
            DoctorSignalFinishAction::Redeliver(signals)
        );
        assert_eq!(
            doctor_signal_finish_action(signals, false),
            DoctorSignalFinishAction::Exit(128 + libc::SIGINT)
        );
        assert_eq!(
            doctor_signal_finish_action(DoctorSignals::default(), false),
            DoctorSignalFinishAction::Continue
        );
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration() {
        const HELPER: &str = "JIG_SQLX_PROBE_MIXED_SIGNAL_HELPER";
        if std::env::var_os(HELPER).is_none() {
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
            assert!(status.success(), "mixed-signal helper exited with {status}");
            return;
        }

        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.store(0, Ordering::SeqCst);
        for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
            // SAFETY: zero initializes the sigaction storage before its fields
            // and mask are populated below.
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
            action.sa_flags = 0;
            // SAFETY: the mask is writable storage owned by this test.
            assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
            // SAFETY: action is initialized and the helper subprocess owns its
            // process-wide dispositions for the remainder of this test.
            assert_eq!(
                unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) },
                0
            );
        }

        let session = DoctorSignalSession::start().unwrap();
        for signal in [
            libc::SIGINT,
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGHUP,
            libc::SIGTERM,
        ] {
            // SAFETY: each supported signal is handled synchronously by the
            // active scoped recorder in this isolated helper subprocess.
            assert_eq!(unsafe { libc::raise(signal) }, 0);
        }
        assert_eq!(
            SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
            0,
            "a signal reached its prior disposition before session retirement",
        );

        finish_doctor_signal_session(session).unwrap();
        assert_eq!(
            SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
            3,
        );
        assert_eq!(
            SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.load(Ordering::SeqCst),
            1 | (2 << 2) | (3 << 4),
        );
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_probe_signal_session_does_not_swallow_later_default_termination() {
        use std::os::unix::process::ExitStatusExt;

        const HELPER: &str = "JIG_SQLX_PROBE_LATER_DEFAULT_SIGNAL_HELPER";
        if std::env::var_os(HELPER).is_none() {
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_does_not_swallow_later_default_termination",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
            assert_eq!(
                status.signal(),
                Some(libc::SIGTERM),
                "later-default-signal helper returned unexpected status {status}",
            );
            return;
        }

        // SAFETY: zero initializes the sigaction storage before its fields and
        // mask are populated below.
        let mut ignored = unsafe { std::mem::zeroed::<libc::sigaction>() };
        ignored.sa_sigaction = libc::SIG_IGN;
        ignored.sa_flags = 0;
        // SAFETY: the mask is writable storage owned by this helper process.
        assert_eq!(unsafe { libc::sigemptyset(&mut ignored.sa_mask) }, 0);
        // SAFETY: ignored is fully initialized and this isolated helper owns
        // its process-wide SIGINT disposition.
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGINT, &ignored, std::ptr::null_mut()) },
            0,
        );
        install_default_doctor_signal_handler(libc::SIGTERM).unwrap();

        let session = DoctorSignalSession::start().unwrap();
        for signal in [libc::SIGINT, libc::SIGTERM] {
            // SAFETY: the active scoped session has installed a handler for
            // each supported signal in this isolated helper process.
            assert_eq!(unsafe { libc::raise(signal) }, 0);
        }
        finish_doctor_signal_session(session).unwrap();
        panic!("the later default SIGTERM disposition was swallowed");
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation() {
        use std::sync::mpsc;

        const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BARRIER_HELPER";
        if std::env::var_os(HELPER).is_none() {
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
            assert!(
                status.success(),
                "reusable barrier helper exited with {status}"
            );
            return;
        }

        SQLX_PROBE_TEST_PAUSE_HANDLER.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_HANDLER_PAUSED.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_RELEASE_HANDLER.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);

        // SAFETY: zero initializes the sigaction storage before its fields and
        // mask are populated below.
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
        action.sa_flags = 0;
        // SAFETY: the mask is writable storage owned by this isolated helper.
        assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
        // SAFETY: this subprocess owns its SIGTERM disposition for the test.
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) },
            0
        );

        let (ready_tx, ready_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let owner = std::thread::spawn(move || {
            let session = DoctorSignalSession::start().unwrap();
            ready_tx.send(session.generation).unwrap();
            finish_rx.recv().unwrap();
            finished_tx
                .send(finish_doctor_signal_session(session).is_ok())
                .unwrap();
        });
        let first_generation = ready_rx.recv().unwrap();

        SQLX_PROBE_TEST_PAUSE_HANDLER.store(true, Ordering::SeqCst);
        let handler = std::thread::spawn(|| record_doctor_signal(libc::SIGTERM));
        let pause_deadline = Instant::now() + Duration::from_secs(1);
        while !SQLX_PROBE_TEST_HANDLER_PAUSED.load(Ordering::SeqCst) {
            assert!(Instant::now() < pause_deadline, "handler did not pause");
            std::thread::yield_now();
        }
        finish_tx.send(()).unwrap();

        let (next_tx, next_rx) = mpsc::channel();
        let next = std::thread::spawn(move || {
            let session = DoctorSignalSession::start().unwrap();
            let generation = session.generation;
            let redelivered = SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst);
            let finished = finish_doctor_signal_session(session).is_ok();
            next_tx.send((generation, redelivered, finished)).unwrap();
        });
        assert!(
            next_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "a second signal-session attempt bypassed the active owner"
        );

        SQLX_PROBE_TEST_RELEASE_HANDLER.store(true, Ordering::SeqCst);
        handler.join().unwrap();
        assert!(finished_rx.recv().unwrap());
        owner.join().unwrap();

        let (next_generation, redelivered, next_finished) =
            next_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        next.join().unwrap();
        assert!(next_generation > first_generation);
        assert_eq!(redelivered, 1, "the next owner entered before redelivery");
        assert!(next_finished);

        SQLX_PROBE_TEST_PAUSE_HANDLER.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_HANDLER_PAUSED.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_RELEASE_HANDLER.store(false, Ordering::SeqCst);
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation() {
        const HELPER: &str = "JIG_SQLX_PROBE_DELAYED_ENTRY_HELPER";
        if std::env::var_os(HELPER).is_none() {
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
            assert!(
                status.success(),
                "delayed-entry helper exited with {status}"
            );
            return;
        }

        SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
        SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);

        // SAFETY: zero initializes the sigaction storage before its fields and
        // mask are populated below.
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
        action.sa_flags = 0;
        // SAFETY: the mask is writable storage owned by this isolated helper.
        assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
        // SAFETY: this subprocess owns its SIGTERM disposition for the test.
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) },
            0
        );

        let first = DoctorSignalSession::start().unwrap();
        let first_generation = first.generation;
        let delayed = std::thread::spawn(|| record_doctor_signal(libc::SIGTERM));
        let pause_deadline = Instant::now() + Duration::from_secs(1);
        while !SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < pause_deadline,
                "handler did not pause before claiming a generation"
            );
            std::thread::yield_now();
        }

        finish_doctor_signal_session(first).unwrap();
        let second = DoctorSignalSession::start().unwrap();
        let second_generation = second.generation;
        assert!(second_generation > first_generation);

        SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
        delayed.join().unwrap();
        assert!(
            second.cancelled(),
            "delayed callback did not join the active generation"
        );
        SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
        finish_doctor_signal_session(second).unwrap();
        assert_eq!(
            SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
            1
        );

        let third = DoctorSignalSession::start().unwrap();
        assert!(third.generation > second_generation);
        finish_doctor_signal_session(third).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal() {
        use std::sync::mpsc;

        const HELPER: &str = "JIG_SQLX_PROBE_RECORDED_TIMEOUT_HELPER";
        if std::env::var_os(HELPER).is_none() {
            let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
            assert_eq!(
                status.code(),
                Some(128 + libc::SIGTERM),
                "recorded-timeout helper returned unexpected status {status}"
            );
            return;
        }

        SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD.store(true, Ordering::SeqCst);
        SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT.store(true, Ordering::SeqCst);
        SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.store(false, Ordering::SeqCst);
        SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.store(false, Ordering::SeqCst);

        let session = DoctorSignalSession::start().unwrap();
        let (handler_done_tx, handler_done_rx) = mpsc::channel();
        let handler = std::thread::spawn(move || {
            record_doctor_signal(libc::SIGTERM);
            handler_done_tx.send(()).unwrap();
        });
        let pause_deadline = Instant::now() + Duration::from_secs(1);
        while !SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < pause_deadline,
                "handler did not pause after recording"
            );
            std::thread::yield_now();
        }

        let coordinator = std::thread::spawn(move || {
            let timeout_deadline = Instant::now() + Duration::from_secs(2);
            while !SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.load(Ordering::SeqCst) {
                assert!(
                    Instant::now() < timeout_deadline,
                    "signal retirement did not reach its quiescence timeout"
                );
                std::thread::yield_now();
            }
            SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.store(true, Ordering::SeqCst);
            handler_done_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("recorded handler did not complete before poison publication");
            SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.store(true, Ordering::SeqCst);
        });

        let result = finish_doctor_signal_session(session);
        coordinator.join().unwrap();
        handler.join().unwrap();
        panic!("recorded signal was not claimed by fail-closed retirement: {result:?}");
    }

    #[cfg(unix)]
    #[test]
    fn inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal() {
        const HELPER: &str = "JIG_SQLX_PROBE_INACTIVE_HANDLER_HELPER";
        if std::env::var_os(HELPER).is_some() {
            DOCTOR_ACTIVE_GENERATION.store(0, Ordering::SeqCst);
            DOCTOR_SIGNAL_GENERATION.store(0, Ordering::SeqCst);
            record_doctor_signal(libc::SIGTERM);
            panic!("an inactive SQLx probe handler swallowed SIGTERM");
        }

        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal",
                "--nocapture",
            ])
            .env(HELPER, "1")
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(128 + libc::SIGTERM));
    }

    #[cfg(unix)]
    #[test]
    fn poisoned_sqlx_probe_session_lock_blocks_future_sessions() {
        const HELPER: &str = "JIG_SQLX_PROBE_POISONED_LOCK_HELPER";
        if std::env::var_os(HELPER).is_some() {
            let poisoner = std::thread::spawn(|| {
                let _guard = DOCTOR_SIGNAL_SESSION.lock().unwrap();
                panic!("poison the signal-session mutex");
            });
            assert!(poisoner.join().is_err());
            let error = DoctorSignalSession::start()
                .err()
                .expect("poisoned mutex must reject a new signal session");
            assert!(error.to_string().contains("mutex is poisoned"));
            return;
        }

        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::poisoned_sqlx_probe_session_lock_blocks_future_sessions",
                "--nocapture",
            ])
            .env(HELPER, "1")
            .status()
            .unwrap();
        assert!(status.success(), "poison helper exited with {status}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sqlx_driver_probe_sigint_reaps_descendants_before_redelivery() {
        use std::os::unix::process::ExitStatusExt;

        let temp = tempdir().unwrap();
        let descendant_marker = temp.path().join("probe-descendant");
        let probe = temp.path().join("sqlx-sigint-tree");
        write_test_executable(
            &probe,
            &owned_test_descendant_script(&descendant_marker, "while :; do :; done"),
        );
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::sqlx_driver_probe_sigint_helper",
                "--nocapture",
            ])
            .env("JIG_SQLX_PROBE_SIGINT_HELPER", &probe)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let descendant = read_test_process_identity(&descendant_marker);
        // SAFETY: the test owns this live helper PID and sends a standard
        // termination signal solely to that subprocess.
        assert_eq!(
            unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
            0
        );
        let status = helper
            .wait_timeout(Duration::from_secs(3))
            .unwrap()
            .expect("SIGINT helper did not terminate after probe cleanup");
        assert_eq!(status.signal(), Some(libc::SIGINT));
        assert_test_process_stopped(&descendant);
    }

    #[cfg(unix)]
    #[test]
    fn owned_process_output_capture_is_bounded_and_lossy_safe() {
        let temp = tempdir().unwrap();
        let noisy = temp.path().join("noisy-output");
        write_test_executable(
            &noisy,
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nprintf '\\377diagnostic\\n'\n",
        );
        let mut command = Command::new(&noisy);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output =
            run_owned_process_tree_with_output(&mut command, Duration::from_secs(2), || false)
                .unwrap();
        let stdout = output.stdout.unwrap();
        let stderr = output.stderr.unwrap();

        assert!(output.status.success());
        assert!(stdout.complete);
        assert!(!stdout.truncated);
        assert!(stdout.to_string_lossy().contains("diagnostic"));
        assert!(stderr.complete);
        assert!(stderr.truncated);
        assert_eq!(stderr.bytes.len(), OWNED_PROCESS_OUTPUT_LIMIT);
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn owned_process_timeout_overflow_remains_unbounded() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "doctor::tests::owned_process_max_timeout_helper",
                "--nocapture",
            ])
            .env("JIG_DOCTOR_MAX_TIMEOUT_HELPER", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = run_owned_process_tree_with_output(&mut command, Duration::MAX, || false)
            .expect("an overflowing timeout remains an unbounded wait");

        assert!(output.status.success());
        assert!(output.stdout.unwrap().complete);
        assert!(output.stderr.unwrap().complete);
    }

    #[test]
    fn proxy_list_command_preserves_the_portable_launcher_plan() {
        let temp = tempdir().unwrap();
        let (launcher, command) = proxy_list_command(temp.path()).unwrap();
        let args = command
            .get_args()
            .map(OsStr::to_os_string)
            .collect::<Vec<_>>();

        assert!(launcher.is_absolute());
        assert_eq!(command.get_current_dir(), Some(temp.path()));
        for key in crate::shell::BASH_CONTROL_ENVIRONMENT_KEYS {
            assert!(
                command
                    .get_envs()
                    .any(|(candidate, value)| candidate == OsStr::new(key) && value.is_none()),
                "{key} was not removed from the launcher-backed proxy diagnostic"
            );
        }
        #[cfg(windows)]
        {
            assert_eq!(command.get_program(), OsStr::new("bash"));
            assert_eq!(
                args,
                vec![
                    launcher.into_os_string(),
                    OsString::from("proxy"),
                    OsString::from("list"),
                    OsString::from("--json"),
                ]
            );
        }
        #[cfg(not(windows))]
        {
            assert_eq!(command.get_program(), launcher.as_os_str());
            assert_eq!(
                args,
                vec![
                    OsString::from("proxy"),
                    OsString::from("list"),
                    OsString::from("--json"),
                ]
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn proxy_list_command_converts_verbatim_roots_for_bash_and_its_working_directory() {
        let (launcher, command) = proxy_list_command(Path::new(r"\\?\C:\repo")).unwrap();
        let args = command
            .get_args()
            .map(OsStr::to_os_string)
            .collect::<Vec<_>>();

        assert_eq!(launcher, PathBuf::from(r"C:\repo\scripts\jig"));
        assert_eq!(command.get_program(), OsStr::new("bash"));
        assert_eq!(command.get_current_dir(), Some(Path::new(r"C:\repo")));
        assert_eq!(args[0], launcher.as_os_str());

        let (unc_launcher, unc_command) =
            proxy_list_command(Path::new(r"\\?\UNC\server\share\repo")).unwrap();
        assert_eq!(
            unc_launcher,
            PathBuf::from(r"\\server\share\repo\scripts\jig")
        );
        assert_eq!(
            unc_command.get_current_dir(),
            Some(Path::new(r"\\server\share\repo"))
        );
        assert_eq!(
            unc_command.get_args().next(),
            Some(unc_launcher.as_os_str())
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_list_output_executes_the_launcher_through_a_clean_bash_environment() {
        let temp = tempdir().unwrap();
        write_doctor_fixture(temp.path());
        let poison_marker = temp.path().join("proxy-poison-ran");
        let trace_marker = temp.path().join("proxy-trace-poison-ran");
        fs::write(
            temp.path().join("scripts/proxy-startup-poison.sh"),
            "printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"\nexit 91\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("scripts/jig"),
            r#"#!/bin/bash
if [ "$#" -ne 3 ] || [ "$1" != proxy ] || [ "$2" != list ] || [ "$3" != --json ]; then
  exit 19
fi
if [ ! -f .jig.toml ]; then
  exit 20
fi
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ]; then
  exit 21
fi
if declare -F jig_doctor_proxy_poison >/dev/null; then
  exit 22
fi
case "$-" in *x*|*v*) exit 23 ;; esac
shopt -q extglob && exit 24
case "$PS4" in *JIG_DOCTOR_PROXY_PS4_POISON*) exit 25 ;; esac
[ "$JIG_DOCTOR_PROXY_ORDINARY" = preserved ] || exit 26
printf '%s\n' '{"ok":true,"running":false,"routes":[]}'
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                temp.path().join("scripts/jig"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let (_, mut command) = proxy_list_command(temp.path()).unwrap();
        command
            .env(
                "BASH_ENV",
                temp.path().join("scripts/proxy-startup-poison.sh"),
            )
            .env("ENV", temp.path().join("scripts/proxy-startup-poison.sh"))
            .env("CDPATH", ".")
            .env(
                "BASH_FUNC_jig_doctor_proxy_poison%%",
                "() { printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"; }",
            )
            .env("SHELLOPTS", "xtrace:verbose")
            .env("BASHOPTS", "extglob")
            .env(
                "PS4",
                "JIG_DOCTOR_PROXY_PS4_POISON$(printf poison > \"$JIG_DOCTOR_PROXY_TRACE_MARKER\")",
            )
            .env("BASH_XTRACEFD", "2")
            .env("JIG_DOCTOR_PROXY_POISON_MARKER", &poison_marker)
            .env("JIG_DOCTOR_PROXY_TRACE_MARKER", &trace_marker)
            .env("JIG_DOCTOR_PROXY_ORDINARY", "preserved");
        crate::shell::sanitize_bash_environment(&mut command);

        let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

        assert_eq!(output["ok"], true);
        assert_eq!(output["running"], false);
        assert_eq!(output["routes"], json!([]));
        assert!(
            !poison_marker.exists(),
            "Bash startup control environment executed during proxy diagnostics"
        );
        assert!(
            !trace_marker.exists(),
            "Bash trace environment executed during proxy diagnostics"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_list_output_accepts_valid_json_larger_than_the_diagnostic_default() {
        let temp = tempdir().unwrap();
        let launcher = temp.path().join("proxy-list-valid");
        write_test_executable(
            &launcher,
            "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 1100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
        );
        let mut command = Command::new(launcher);

        let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

        assert_eq!(output["ok"], true);
        assert_eq!(output["running"], false);
        assert_eq!(output["routes"], json!([]));
        assert!(
            output["padding"].as_str().unwrap().len() > OWNED_PROCESS_OUTPUT_LIMIT,
            "proxy functional JSON must not share the 16 KiB diagnostic cap"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_list_output_reports_injected_stdout_truncation() {
        let temp = tempdir().unwrap();
        let launcher = temp.path().join("proxy-list-truncated");
        write_test_executable(
            &launcher,
            "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
        );
        let mut command = Command::new(launcher);

        let error = proxy_list_output_with_timeout_and_limits_and_cancellation(
            &mut command,
            Duration::from_secs(2),
            ProcessOutputLimits {
                stdout: 128,
                stderr: OWNED_PROCESS_OUTPUT_LIMIT,
            },
            || false,
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("exceeded the diagnostic capture limit"),
            "{error}"
        );
    }

    #[cfg(feature = "dev-proxy")]
    #[test]
    fn proxy_list_capture_limit_exceeds_the_unchanged_routes_file_limit() {
        assert_eq!(jig_dev_proxy::MAX_ROUTES_FILE_BYTES, 4 * 1024 * 1024);
        assert!(
            ProcessOutputLimits::proxy_list().stdout
                > jig_dev_proxy::MAX_ROUTES_FILE_BYTES as usize
        );
        assert_eq!(
            ProcessOutputLimits::proxy_list().stderr,
            OWNED_PROCESS_OUTPUT_LIMIT
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_list_output_timeout_reaps_its_exact_descendant() {
        let temp = tempdir().unwrap();
        let marker = temp.path().join("proxy-list-descendant");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "doctor::tests::proxy_list_output_helper",
                "--nocapture",
            ])
            .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
            .env("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER", &marker);

        let error = proxy_list_output_with_timeout(&mut command, Duration::from_millis(100))
            .unwrap_err()
            .to_string();
        let descendant = read_test_process_identity(&marker);

        assert!(error.contains("timed out"), "{error}");
        assert_test_process_stopped(&descendant);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_check_sigint_helper() {
        let Some(root) = std::env::var_os("JIG_DOCTOR_PROXY_SIGINT_ROOT") else {
            return;
        };
        let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
        let result = doctor_context_checks(&ctx);
        panic!("SIGINT was not re-delivered after proxy cleanup: {result:?}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn proxy_check_sigint_reaps_its_exact_descendant_before_redelivery() {
        use std::os::unix::process::ExitStatusExt;

        let temp = tempdir().unwrap();
        write_doctor_fixture(temp.path());
        fs::write(
            temp.path().join(".jig.toml"),
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
            ),
        )
        .unwrap();
        fs::create_dir(temp.path().join("web")).unwrap();
        write_test_executable(
            &temp.path().join("scripts/jig"),
            &format!(
                "#!/bin/sh\nexec {} --exact doctor::tests::proxy_list_output_helper --nocapture\n",
                shell_quote_test_path(&std::env::current_exe().unwrap())
            ),
        );
        let descendant_marker = temp.path().join("proxy-sigint-descendant");
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::proxy_check_sigint_helper",
                "--nocapture",
            ])
            .env("JIG_DOCTOR_PROXY_SIGINT_ROOT", temp.path())
            .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
            .env(
                "JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER",
                &descendant_marker,
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let descendant = read_test_process_identity(&descendant_marker);
        // SAFETY: this test owns the live isolated helper subprocess.
        assert_eq!(
            unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
            0
        );
        let status = helper
            .wait_timeout(Duration::from_secs(3))
            .unwrap()
            .expect("SIGINT helper did not terminate after proxy cleanup");
        assert_eq!(status.signal(), Some(libc::SIGINT));
        assert_test_process_stopped(&descendant);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn production_sqlx_probe_sigint_helper() {
        let Some(marker) = std::env::var_os("JIG_DOCTOR_SQLX_PRODUCTION_MARKER") else {
            return;
        };
        let identity = TestProcessIdentity::capture_current().unwrap();
        publish_test_process_identity(Path::new(&marker), &identity);
        std::thread::sleep(Duration::from_secs(30));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn doctor_sqlx_sigint_sequence_helper() {
        let Some(root) = std::env::var_os("JIG_DOCTOR_SQLX_SEQUENCE_ROOT") else {
            return;
        };
        let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
        let result = doctor_context_checks(&ctx);
        panic!("SIGINT was not re-delivered after SQLx cleanup: {result:?}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn cancellation_during_production_sqlx_prevents_codex_and_proxy_spawns() {
        use std::os::unix::process::ExitStatusExt;

        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "sqlx prepare -D sqlite:production-signal.db",
        );
        let config_path = temp.path().join(".jig.toml");
        fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
        fs::create_dir(temp.path().join("web")).unwrap();

        let probe_marker = temp.path().join("sqlx-production-probe");
        let tools = tempdir().unwrap();
        write_test_executable(
            &tools.path().join("sqlx"),
            &format!(
                "#!/bin/sh\nJIG_DOCTOR_SQLX_PRODUCTION_MARKER={} exec {} --exact doctor::tests::production_sqlx_probe_sigint_helper --nocapture\n",
                shell_quote_test_path(&probe_marker),
                shell_quote_test_path(&std::env::current_exe().unwrap())
            ),
        );
        let codex_marker = temp.path().join("codex-started");
        let codex = temp.path().join("codex");
        write_test_executable(
            &codex,
            &format!(
                "#!/bin/sh\nprintf c > '{}'\nexit 0\n",
                codex_marker.display()
            ),
        );
        let proxy_marker = temp.path().join("proxy-started");
        write_test_executable(
            &temp.path().join("scripts/jig"),
            &format!(
                "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
                proxy_marker.display()
            ),
        );

        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::doctor_sqlx_sigint_sequence_helper",
                "--nocapture",
            ])
            .env("JIG_DOCTOR_SQLX_SEQUENCE_ROOT", temp.path())
            .env("JIG_CODEX_BIN", &codex)
            .env("CODEX_HOME", temp.path().join("codex-home"))
            .env("PATH", fs::canonicalize(tools.path()).unwrap())
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .env_remove("CDPATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let probe = read_test_process_identity(&probe_marker);
        // SAFETY: this test owns the isolated doctor helper subprocess.
        assert_eq!(
            unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
            0
        );
        let status = helper
            .wait_timeout(Duration::from_secs(3))
            .unwrap()
            .expect("doctor helper did not terminate after SQLx cleanup");
        assert_eq!(status.signal(), Some(libc::SIGINT));
        assert_test_process_stopped(&probe);
        assert!(
            !codex_marker.exists(),
            "Codex started after SQLx cancellation"
        );
        assert!(
            !proxy_marker.exists(),
            "proxy started after SQLx cancellation"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn production_codex_probe_helper() {
        let Some(marker) = std::env::var_os("JIG_DOCTOR_CODEX_DESCENDANT_MARKER") else {
            return;
        };
        for _ in 0..2_000 {
            println!("codex-probe-secret-that-must-not-leak");
            eprintln!("codex-probe-secret-that-must-not-leak");
        }
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::owned_process_descendant_helper",
                "--nocapture",
            ])
            .env(OWNED_PROCESS_DESCENDANT_MARKER_ENV, &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        std::mem::forget(child);
        let _ = read_test_process_identity(Path::new(&marker));
        std::thread::sleep(Duration::from_secs(30));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn doctor_codex_sigint_sequence_helper() {
        let Some(root) = std::env::var_os("JIG_DOCTOR_CODEX_SEQUENCE_ROOT") else {
            return;
        };
        let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
        let result = doctor_context_checks(&ctx);
        panic!("SIGINT was not re-delivered after Codex cleanup: {result:?}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn cancellation_during_noisy_codex_reaps_descendant_and_prevents_proxy_spawn() {
        use std::os::unix::process::ExitStatusExt;

        let temp = tempdir().unwrap();
        write_doctor_fixture(temp.path());
        let config_path = temp.path().join(".jig.toml");
        fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
        fs::create_dir(temp.path().join("web")).unwrap();
        let codex = temp.path().join("codex");
        write_test_executable(
            &codex,
            &format!(
                "#!/bin/sh\nexec {} --exact doctor::tests::production_codex_probe_helper --nocapture\n",
                shell_quote_test_path(&std::env::current_exe().unwrap())
            ),
        );
        let proxy_marker = temp.path().join("proxy-started");
        write_test_executable(
            &temp.path().join("scripts/jig"),
            &format!(
                "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
                proxy_marker.display()
            ),
        );
        let descendant_marker = temp.path().join("codex-descendant");
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::doctor_codex_sigint_sequence_helper",
                "--nocapture",
            ])
            .env("JIG_DOCTOR_CODEX_SEQUENCE_ROOT", temp.path())
            .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
            .env("JIG_CODEX_BIN", &codex)
            .env("CODEX_HOME", temp.path().join("codex-home"))
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .env_remove("CDPATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let descendant = read_test_process_identity(&descendant_marker);
        // SAFETY: this test owns the isolated doctor helper subprocess.
        assert_eq!(
            unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
            0
        );
        let status = helper
            .wait_timeout(Duration::from_secs(3))
            .unwrap()
            .expect("doctor helper did not terminate after Codex cleanup");
        assert_eq!(status.signal(), Some(libc::SIGINT));
        assert_test_process_stopped(&descendant);
        assert!(
            !proxy_marker.exists(),
            "proxy started after Codex cancellation"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn standalone_codex_sigint_sequence_helper() {
        let Some(codex) = std::env::var_os("JIG_STANDALONE_CODEX_SIGINT_BIN") else {
            return;
        };
        let result = standalone_codex_support_probe_with_signal_session(
            codex.to_str().unwrap(),
            Duration::from_secs(30),
        );
        panic!("SIGINT was not re-delivered after standalone Codex cleanup: {result:?}");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn standalone_codex_sigint_reaps_its_exact_descendant_before_redelivery() {
        use std::os::unix::process::ExitStatusExt;

        let temp = tempdir().unwrap();
        let codex = temp.path().join("codex");
        write_test_executable(
            &codex,
            &format!(
                "#!/bin/sh\nexec {} --exact doctor::tests::production_codex_probe_helper --nocapture\n",
                shell_quote_test_path(&std::env::current_exe().unwrap())
            ),
        );
        let descendant_marker = temp.path().join("standalone-codex-descendant");
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::standalone_codex_sigint_sequence_helper",
                "--nocapture",
            ])
            .env("JIG_STANDALONE_CODEX_SIGINT_BIN", &codex)
            .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let descendant = read_test_process_identity(&descendant_marker);
        // SAFETY: this test owns the isolated standalone doctor helper.
        assert_eq!(
            unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
            0
        );
        let status = helper
            .wait_timeout(Duration::from_secs(3))
            .unwrap()
            .expect("standalone doctor helper did not terminate after Codex cleanup");
        assert_eq!(status.signal(), Some(libc::SIGINT));
        assert_test_process_stopped(&descendant);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn owned_process_output_escape_helper() {
        let _env = lock_env();
        let Some(mode) = std::env::var_os("JIG_OWNED_OUTPUT_ESCAPE_HELPER") else {
            return;
        };
        let marker = PathBuf::from(
            std::env::var_os("JIG_OWNED_OUTPUT_ESCAPE_MARKER").expect("escape marker path"),
        );
        if mode == "escaped" {
            assert_ne!(
                unsafe { libc::setsid() },
                -1,
                "escape the owned process group"
            );
            let identity =
                TestProcessIdentity::capture_current().expect("capture escaped helper identity");
            publish_test_process_identity(&marker, &identity);
            std::thread::sleep(Duration::from_secs(30));
            return;
        }

        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::owned_process_output_escape_helper",
                "--nocapture",
            ])
            .env("JIG_OWNED_OUTPUT_ESCAPE_HELPER", "escaped")
            .env("JIG_OWNED_OUTPUT_ESCAPE_MARKER", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        std::mem::forget(child);
        let _ = read_test_process_identity(&marker);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn owned_process_output_capture_does_not_wait_for_an_escaped_pipe_owner() {
        let temp = tempdir().unwrap();
        let started = Instant::now();
        for iteration in 0..4 {
            let marker = temp
                .path()
                .join(format!("escaped-output-owner-{iteration}"));
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "doctor::tests::owned_process_output_escape_helper",
                    "--nocapture",
                ])
                .env("JIG_OWNED_OUTPUT_ESCAPE_HELPER", "spawn")
                .env("JIG_OWNED_OUTPUT_ESCAPE_MARKER", &marker)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let output =
                run_owned_process_tree_with_output(&mut command, Duration::from_secs(5), || false)
                    .unwrap();
            let escaped = read_test_process_identity(&marker);
            terminate_and_confirm_test_process(&escaped);

            assert!(output.status.success());
            assert!(
                !output.stdout.unwrap().complete || !output.stderr.unwrap().complete,
                "an escaped pipe owner must make capture explicitly incomplete"
            );
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "repeated silent pipe escapes must remain bounded without accumulating readers"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_waitid_observer_accepts_only_the_exact_terminal_child() {
        for code in [libc::CLD_EXITED, libc::CLD_KILLED, libc::CLD_DUMPED] {
            assert_eq!(
                classify_probe_waitid_observation(73, 73, code).unwrap(),
                ProbeLeaderObservation::Exited
            );
        }
        for code in [libc::CLD_STOPPED, libc::CLD_TRAPPED, libc::CLD_CONTINUED] {
            assert_eq!(
                classify_probe_waitid_observation(73, 73, code).unwrap(),
                ProbeLeaderObservation::Running
            );
        }
        assert_eq!(
            classify_probe_waitid_observation(73, 0, i32::MAX).unwrap(),
            ProbeLeaderObservation::Running
        );
        assert!(classify_probe_waitid_observation(73, 74, libc::CLD_EXITED).is_err());
        assert!(classify_probe_waitid_observation(73, 73, i32::MAX).is_err());
    }

    #[test]
    fn macos_probe_snapshot_requires_the_exact_sole_pinned_leader() {
        assert!(classify_macos_probe_group_snapshot(73, 1, [73, 0]).unwrap());
        assert!(!classify_macos_probe_group_snapshot(73, 2, [73, 74]).unwrap());
        assert!(!classify_macos_probe_group_snapshot(73, 2, [74, 75]).unwrap());
        assert!(classify_macos_probe_group_snapshot(73, 1, [74, 0]).is_err());
        assert!(classify_macos_probe_group_snapshot(73, 1, [0, 0]).is_err());
        assert!(classify_macos_probe_group_snapshot(0, 1, [73, 0]).is_err());
        assert!(classify_macos_probe_group_snapshot(73, 0, [0, 0]).is_err());
        assert!(classify_macos_probe_group_snapshot(73, 3, [73, 74]).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_group_confirmation_resignals_before_retrying_an_inconclusive_proof() {
        #[derive(Default)]
        struct InjectedCleanup {
            signals: usize,
            proofs: VecDeque<bool>,
        }

        let now = Instant::now();
        let sleeps = std::cell::Cell::new(0_usize);
        let mut cleanup = InjectedCleanup {
            proofs: VecDeque::from([false, true]),
            ..InjectedCleanup::default()
        };
        confirm_probe_process_group_quiescent_with(
            &mut cleanup,
            73,
            now + Duration::from_secs(1),
            1,
            "injected confirmation",
            |cleanup, _, _| {
                cleanup.signals += 1;
                Ok(if cleanup.signals == 1 {
                    // Models ESRCH or eligible macOS EPERM: neither is proof
                    // that a late group member cannot become visible.
                    ProbeProcessGroupSignalResult::Inconclusive
                } else {
                    ProbeProcessGroupSignalResult::Delivered
                })
            },
            |cleanup, _, _| {
                Ok(cleanup
                    .proofs
                    .pop_front()
                    .expect("injected proof sequence was exhausted"))
            },
            || now,
            |_| sleeps.set(sleeps.get() + 1),
        )
        .unwrap();

        assert_eq!(cleanup.signals, 2);
        assert!(cleanup.proofs.is_empty());
        assert_eq!(sleeps.get(), 1);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_group_confirmation_signals_between_two_empty_linux_proofs() {
        #[derive(Default)]
        struct InjectedCleanup {
            signals: usize,
            proofs: VecDeque<bool>,
        }

        let now = Instant::now();
        let mut cleanup = InjectedCleanup {
            // A live scan resets the proof count; two following empty scans
            // are each preceded by a group SIGKILL attempt.
            proofs: VecDeque::from([false, true, true]),
            ..InjectedCleanup::default()
        };
        confirm_probe_process_group_quiescent_with(
            &mut cleanup,
            73,
            now + Duration::from_secs(1),
            2,
            "injected Linux confirmation",
            |cleanup, _, _| {
                cleanup.signals += 1;
                Ok(ProbeProcessGroupSignalResult::Delivered)
            },
            |cleanup, _, _| {
                Ok(cleanup
                    .proofs
                    .pop_front()
                    .expect("injected proof sequence was exhausted"))
            },
            || now,
            |_| {},
        )
        .unwrap();

        assert_eq!(cleanup.signals, 3);
        assert!(cleanup.proofs.is_empty());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_group_confirmation_never_proves_or_resignals_after_a_terminal_error() {
        #[derive(Default)]
        struct InjectedCleanup {
            signals: usize,
            proofs: usize,
        }

        let now = Instant::now();
        let mut cleanup = InjectedCleanup::default();
        let error = confirm_probe_process_group_quiescent_with(
            &mut cleanup,
            73,
            now + Duration::from_secs(1),
            1,
            "injected terminal error",
            |cleanup, _, _| {
                cleanup.signals += 1;
                Err(std::io::Error::from_raw_os_error(libc::ECHILD))
            },
            |cleanup, _, _| {
                cleanup.proofs += 1;
                Ok(true)
            },
            || now,
            |_| {},
        )
        .unwrap_err();

        assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
        assert_eq!(cleanup.signals, 1);
        assert_eq!(cleanup.proofs, 0);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_group_signal_observes_the_pinned_leader_before_any_signal_attempt() {
        #[derive(Default)]
        struct InjectedSignal {
            observations: usize,
            signals: usize,
        }

        let mut injected = InjectedSignal::default();
        let error = observe_probe_leader_before_group_signal_with(
            &mut injected,
            |injected| {
                injected.observations += 1;
                Err(std::io::Error::from_raw_os_error(libc::ECHILD))
            },
            |injected, _| {
                injected.signals += 1;
                Ok(ProbeProcessGroupSignalResult::Delivered)
            },
        )
        .unwrap_err();

        assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
        assert_eq!(injected.observations, 1);
        assert_eq!(injected.signals, 0);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_group_confirmation_checks_the_absolute_deadline_after_every_signal() {
        #[derive(Default)]
        struct InjectedCleanup {
            signals: usize,
            proofs: usize,
        }

        let start = Instant::now();
        let deadline = start + Duration::from_millis(20);
        let observed_now = std::cell::Cell::new(start);
        let sleeps = std::cell::Cell::new(0_usize);
        let mut cleanup = InjectedCleanup::default();
        let error = confirm_probe_process_group_quiescent_with(
            &mut cleanup,
            73,
            deadline,
            1,
            "injected deadline",
            |cleanup, _, _| {
                cleanup.signals += 1;
                observed_now.set(deadline);
                Ok(ProbeProcessGroupSignalResult::Delivered)
            },
            |cleanup, _, _| {
                cleanup.proofs += 1;
                Ok(true)
            },
            || observed_now.get(),
            |_| sleeps.set(sleeps.get() + 1),
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(cleanup.signals, 1);
        assert_eq!(cleanup.proofs, 0);
        assert_eq!(sleeps.get(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_probe_group_eperm_is_inconclusive_only_for_an_exited_pinned_leader() {
        let eligible = resolve_macos_probe_group_signal_eperm(
            std::io::Error::from_raw_os_error(libc::EPERM),
            Ok(ProbeLeaderObservation::Exited),
        )
        .unwrap();
        assert_eq!(eligible, ProbeProcessGroupSignalResult::Inconclusive);

        let running_error = resolve_macos_probe_group_signal_eperm(
            std::io::Error::from_raw_os_error(libc::EPERM),
            Ok(ProbeLeaderObservation::Running),
        )
        .unwrap_err();
        assert_eq!(running_error.raw_os_error(), Some(libc::EPERM));

        let observation_error = resolve_macos_probe_group_signal_eperm(
            std::io::Error::from_raw_os_error(libc::EPERM),
            Err(std::io::Error::from_raw_os_error(libc::ECHILD)),
        )
        .unwrap_err();
        assert_eq!(
            observation_error.kind(),
            std::io::Error::from_raw_os_error(libc::ECHILD).kind()
        );
        assert!(observation_error.to_string().contains("after EPERM"));
    }

    #[test]
    fn linux_probe_scan_distinguishes_live_zombie_and_unverifiable_members() {
        let stats = std::collections::HashMap::from([
            (73, "73 (leader) Z 1 73 0 0 0".to_string()),
            (74, "74 (worker) S 73 73 0 0 0".to_string()),
            (75, "75 (other) S 1 75 0 0 0".to_string()),
        ]);
        assert!(
            linux_probe_group_has_live_members_with(
                73,
                [73, 74, 75],
                |pid| Ok(stats[&pid].clone()),
                |_| unreachable!(),
                || true,
            )
            .unwrap()
        );
        assert!(
            !linux_probe_group_has_live_members_with(
                73,
                [73, 75],
                |pid| Ok(stats[&pid].clone()),
                |_| unreachable!(),
                || true,
            )
            .unwrap()
        );
        assert!(
            !linux_probe_group_has_live_members_with(
                73,
                [76],
                |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
                |_| Ok(None),
                || true,
            )
            .unwrap()
        );
        assert!(
            linux_probe_group_has_live_members_with(
                73,
                [76],
                |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
                |_| Ok(Some(73)),
                || true,
            )
            .is_err()
        );
        assert!(
            linux_probe_group_has_live_members_with(
                73,
                [73],
                |pid| Ok(stats[&pid].clone()),
                |_| unreachable!(),
                || false,
            )
            .is_err()
        );
    }

    #[test]
    fn linux_probe_enumeration_fails_closed_when_advancing_exhausts_budget() {
        let within_budget = std::cell::Cell::new(true);
        let entries = std::iter::from_fn(|| {
            within_budget.set(false);
            None::<std::io::Result<i32>>
        });

        let error = collect_linux_probe_process_ids_with(73, entries, Some, || within_budget.get())
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("while scanning Linux processes"));
    }

    #[test]
    fn linux_probe_scan_fails_closed_when_fallback_lookup_exhausts_budget() {
        let within_budget = std::cell::Cell::new(true);

        let error = linux_probe_group_has_live_members_with(
            73,
            [74],
            |_| Err(std::io::Error::other("injected stat failure")),
            |_| {
                within_budget.set(false);
                Ok(None)
            },
            || within_budget.get(),
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("while scanning Linux processes"));
    }

    #[test]
    fn linux_probe_scan_checks_budget_before_live_and_empty_results() {
        let live_checks = std::cell::Cell::new(0usize);
        let live_error = linux_probe_group_has_live_members_with(
            73,
            [74],
            |_| Ok("74 (worker) S 1 73 0 0 0".to_string()),
            |_| unreachable!(),
            || {
                let check = live_checks.get() + 1;
                live_checks.set(check);
                check <= 3
            },
        )
        .unwrap_err();
        assert_eq!(live_checks.get(), 4);
        assert_eq!(live_error.kind(), std::io::ErrorKind::TimedOut);

        let empty_checks = std::cell::Cell::new(0usize);
        let empty_error = linux_probe_group_has_live_members_with(
            73,
            std::iter::empty(),
            |_| unreachable!(),
            |_| unreachable!(),
            || {
                let check = empty_checks.get() + 1;
                empty_checks.set(check);
                check == 1
            },
        )
        .unwrap_err();
        assert_eq!(empty_checks.get(), 2);
        assert_eq!(empty_error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_cleanup_uses_one_absolute_deadline() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_probe_process(&mut command).unwrap();
        let first = process.cleanup_deadline();
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(process.cleanup_deadline(), first);
        process.cleanup_deadline = None;
        process.terminate_and_reap().unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn probe_tree_failure_uses_one_direct_fallback_and_retains_the_primary_error() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_probe_process(&mut command).unwrap();
        process.process_group = Some(PinnedProbeProcessGroup { id: -1 });

        let first = process.terminate_and_reap().unwrap_err().to_string();
        assert!(first.contains("identity is not positive"), "{first}");
        assert!(process.cleanup_finalized);
        assert!(!process.cleanup_complete);
        assert!(process.reaped_status.is_some());
        assert!(process.process_group.is_none());
        assert_eq!(process.terminate_and_reap().unwrap_err().to_string(), first);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_probe_cleanup_stress_never_releases_a_live_group_member() {
        for iteration in 0..100 {
            let mut command = Command::new("/bin/bash");
            command
                .args(["-c", "(/bin/sleep 30) & printf '%s' \"$!\"; exit 0"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let output =
                run_owned_process_tree_with_output(&mut command, Duration::from_secs(2), || false)
                    .unwrap_or_else(|error| panic!("iteration {iteration}: {error}"));
            assert!(output.status.success(), "iteration {iteration}");
            let descendant = output
                .stdout
                .unwrap()
                .to_string_lossy()
                .parse::<libc::pid_t>()
                .unwrap();
            // SAFETY: a zero signal only probes the PID written by this owned
            // helper. Cleanup must not return while that group member exists.
            let result = unsafe { libc::kill(descendant, 0) };
            assert_eq!(result, -1, "iteration {iteration}, PID {descendant}");
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH),
                "iteration {iteration}, PID {descendant}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_driver_probe_cleanup_is_bounded_and_requires_pinned_identity() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_probe_process(&mut command).unwrap();
        let process_group = process.process_group.take().unwrap();

        let started = std::time::Instant::now();
        assert!(matches!(
            finish_probe_wait(
                &mut process,
                Err(std::io::Error::other("injected observation failure")),
            ),
            Err(OwnedProcessTreeError::Cleanup)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(process.reaped_status.is_none());
        assert!(!process.cleanup_complete);
        assert!(process.cleanup_finalized);
        let retained_error = process.cleanup_error.as_ref().unwrap().message.clone();
        assert!(retained_error.contains("direct-child fallback also failed"));
        assert!(retained_error.contains("direct-child reap also failed"));

        // Restoring an identity after finalization must not cause a second
        // cleanup attempt or overwrite the first failure.
        process.process_group = Some(process_group);
        assert_eq!(
            process.terminate_and_reap().unwrap_err().to_string(),
            retained_error
        );
        // SAFETY: the test deliberately hid this still-pinned group from the
        // supervisor and retains its exact original identifier for teardown.
        assert_eq!(unsafe { libc::kill(-process_group.id, libc::SIGKILL) }, 0);
        process.child.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sqlx_driver_probe_wait_errors_only_release_identity_after_echild() {
        for errno in [libc::EINVAL, libc::ENOSYS] {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", "while :; do :; done"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut process = spawn_probe_process(&mut command).unwrap();
            let error = std::io::Error::from_raw_os_error(errno);

            update_probe_identity_after_wait_error(&mut process, &error);

            assert!(process.process_group.is_some(), "errno {errno}");
            assert!(
                matches!(
                    finish_probe_wait(&mut process, Err(error)),
                    Err(OwnedProcessTreeError::Await)
                ),
                "errno {errno}"
            );
            assert!(process.cleanup_complete, "errno {errno}");
            assert!(process.process_group.is_none(), "errno {errno}");
        }

        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_probe_process(&mut command).unwrap();
        let process_group = process.process_group.unwrap();
        let error = std::io::Error::from_raw_os_error(libc::ECHILD);
        update_probe_identity_after_wait_error(&mut process, &error);

        assert!(process.process_group.is_none());
        assert!(matches!(
            finish_probe_wait(&mut process, Err(error)),
            Err(OwnedProcessTreeError::Cleanup)
        ));
        assert!(!process.cleanup_complete);
        assert!(process.cleanup_finalized);
        let retained_error = process.cleanup_error.as_ref().unwrap().message.clone();

        process.process_group = Some(process_group);
        assert_eq!(
            process.terminate_and_reap().unwrap_err().to_string(),
            retained_error
        );
        // SAFETY: this is the exact original process group whose identity the
        // test removed artificially; terminate it solely for test teardown.
        assert_eq!(unsafe { libc::kill(-process_group.id, libc::SIGKILL) }, 0);
        process.child.wait().unwrap();
    }

    #[test]
    fn windows_job_active_process_poll_is_bounded_and_propagates_query_errors() {
        let mut active = VecDeque::from([2, 1, 0]);
        wait_for_no_active_processes(Duration::from_secs(1), || {
            Ok(active.pop_front().unwrap_or(0))
        })
        .unwrap();
        assert!(active.is_empty());

        let error = wait_for_no_active_processes(Duration::from_secs(1), || {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected query failure",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);

        let started = std::time::Instant::now();
        let error = wait_for_no_active_processes(Duration::from_millis(20), || Ok(1)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(windows)]
    #[test]
    fn windows_probe_process_starts_suspended_in_a_new_process_group() {
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

        let flags = windows_probe_creation_flags();
        assert_eq!(flags, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
        assert_ne!(flags & CREATE_SUSPENDED, 0);
        assert_ne!(flags & CREATE_NEW_PROCESS_GROUP, 0);
    }

    #[cfg(windows)]
    #[test]
    fn sqlx_driver_probe_windows_job_reaps_descendants() {
        let temp = tempdir().unwrap();
        let marker = temp.path().join("windows-descendant");
        let ready = temp.path().join("windows-ready");
        let script = temp.path().join("probe-tree.cmd");
        fs::write(
            &script,
            format!(
                "@echo off\r\nif \"%~1\"==\"child\" goto child\r\nstart \"\" /b cmd.exe /d /c call \"%~f0\" child\r\n>\"{}\" echo ready\r\nping.exe -n 30 127.0.0.1 >nul\r\nexit /b\r\n:child\r\nping.exe -n 3 127.0.0.1 >nul\r\n>\"{}\" echo leaked\r\nexit /b\r\n",
                ready.display(),
                marker.display(),
            ),
        )
        .unwrap();
        let mut command = Command::new("cmd.exe");
        command
            .args(["/d", "/c"])
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_probe_process(&mut command).unwrap();
        wait_for_test_file(&ready);
        assert!(matches!(
            finish_probe_wait(
                &mut process,
                Err(std::io::Error::other("injected wait failure")),
            ),
            Err(OwnedProcessTreeError::Await)
        ));

        std::thread::sleep(Duration::from_millis(2_500));
        assert!(
            !marker.exists(),
            "Windows Job Object left a descendant running"
        );
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_distinguishes_missing_and_incompatible_sqlx_cli() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), "cargo-sqlx sqlx prepare");
        fs::write(
            temp.path().join(".env"),
            "DATABASE_URL=sqlite:private-database-name.db\n",
        )
        .unwrap();

        let tools = tempdir().unwrap();
        let bin = tools.path().to_path_buf();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let missing = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
        assert!(!missing.ok);
        assert_eq!(missing.status, "missing");
        assert!(missing.detail.contains("cargo-sqlx"));

        write_test_executable(
            &bin.join("cargo-sqlx"),
            "#!/bin/sh\nprintf '%s\\n' 'error: error with configuration: no driver found for URL scheme \"sqlite\"'\nexit 1\n",
        );
        let incompatible =
            required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
        assert!(!incompatible.ok);
        assert_eq!(incompatible.status, "incompatible");
        assert!(incompatible.detail.contains("lacks the SQLite driver"));
        assert!(
            incompatible
                .fix
                .as_deref()
                .unwrap()
                .contains("--features sqlite")
        );
        assert_eq!(
            cargo_sqlx_program(&incompatible)["driver_probe"]["status"],
            "missing_driver"
        );
        assert_eq!(
            cargo_sqlx_program(&incompatible)["driver_probe"]["compatible"],
            false
        );

        let serialized = serde_json::to_string(&incompatible).unwrap();
        assert!(!serialized.contains("private-database-name"));
        assert!(!serialized.contains("sqlite:private"));
        let summary = format_summary(&output(None, vec![incompatible]));
        assert!(summary.contains("Required tools: needs setup (incompatible, required)"));
        assert!(summary.contains("--features sqlite"));
        assert!(!summary.contains("private-database-name"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_require_external_wrappers_and_their_targets() {
        let run = |command: &str, executables: &[&str]| {
            let repo = tempdir().unwrap();
            write_doctor_fixture_with_bootstrap_command(repo.path(), command);
            let tools = tempdir().unwrap();
            for executable in executables {
                write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
            }
            let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None))
        };
        let bootstrap_programs = |check: &DoctorCheck| {
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "bootstrap_command")
                .unwrap()["programs"]
                .as_array()
                .unwrap()
                .clone()
        };

        for wrapper in ["env", "nohup"] {
            let command = format!("{wrapper} cargo test");
            let missing_wrapper = run(&command, &["cargo"]);
            assert_eq!(missing_wrapper.status, "missing", "{wrapper}");
            assert!(!missing_wrapper.ok, "{wrapper}");
            let programs = bootstrap_programs(&missing_wrapper);
            assert_eq!(programs[0]["program"], wrapper, "{wrapper}");
            assert_eq!(programs[0]["present"], false, "{wrapper}");
            assert_eq!(programs[1]["program"], "cargo", "{wrapper}");
            assert_eq!(programs[1]["present"], true, "{wrapper}");

            let missing_target = run(&command, &[wrapper]);
            assert_eq!(missing_target.status, "missing", "{wrapper}");
            let programs = bootstrap_programs(&missing_target);
            assert_eq!(programs[0]["present"], true, "{wrapper}");
            assert_eq!(programs[1]["present"], false, "{wrapper}");

            let all_present = run(&command, &[wrapper, "cargo"]);
            assert_eq!(all_present.status, "present", "{wrapper}");
            assert!(all_present.ok, "{wrapper}");
        }

        for command in ["env --help", "env -0"] {
            let missing_wrapper = run(command, &[]);
            assert_eq!(missing_wrapper.status, "missing", "{command:?}");
            let programs = bootstrap_programs(&missing_wrapper);
            assert_eq!(programs.len(), 1, "{command:?}");
            assert_eq!(programs[0]["program"], "env", "{command:?}");
            assert_eq!(programs[0]["present"], false, "{command:?}");
        }

        let dynamic_target = run("env \"$TOOL\" test", &[]);
        assert_eq!(dynamic_target.status, "missing");
        let programs = bootstrap_programs(&dynamic_target);
        assert_eq!(programs[0]["program"], "env");
        assert_eq!(programs[0]["present"], false);
        assert_eq!(programs[1]["program"], Value::Null);
        assert_eq!(programs[1]["present"], Value::Null);
        assert!(
            !serde_json::to_string(&dynamic_target)
                .unwrap()
                .contains("TOOL")
        );
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_check_nested_external_time_chain_in_order() {
        let repo = tempdir().unwrap();
        let tools = tempdir().unwrap();
        for executable in ["env", "nohup", "time", "cargo"] {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let time = tools.path().join("time");
        write_doctor_fixture_with_bootstrap_command(
            repo.path(),
            &format!("env nohup {} cargo test", time.display()),
        );
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present");
        let programs = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap()["programs"]
            .as_array()
            .unwrap();
        assert_eq!(programs.len(), 4);
        assert_eq!(programs[0]["program"], "env");
        assert_eq!(programs[1]["program"], "nohup");
        assert_eq!(programs[2]["program"], time.display().to_string());
        assert_eq!(programs[3]["program"], "cargo");
        assert!(programs.iter().all(|program| program["present"] == true));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_marks_ambiguous_wrappers_unverified_without_leaking() {
        for (command, secret) in [
            (
                "env -S 'doctor-split-secret missing-tool --flag'",
                "doctor-split-secret",
            ),
            (
                "env '--split-string=doctor-long-split-secret missing-tool --flag' cargo",
                "doctor-long-split-secret",
            ),
            (
                "exec -z doctor-wrapper-secret cargo test",
                "doctor-wrapper-secret",
            ),
        ] {
            let temp = tempdir().unwrap();
            write_doctor_fixture_with_bootstrap_command(temp.path(), command);
            let tools = tempdir().unwrap();
            write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

            let check = required_tools_check_with_environment(
                &ctx,
                &DoctorEnvironment {
                    search_path: Some(tools.path().as_os_str().to_os_string()),
                    ..DoctorEnvironment::default()
                },
            );

            assert!(check.ok, "{command:?}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{command:?}");
            assert!(check.fix.is_none(), "{command:?}");
            let tool = check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "bootstrap_command")
                .unwrap();
            assert!(tool["present"].is_null(), "{command:?}");
            assert!(
                tool["programs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|program| program["present"].is_null()),
                "{command:?}",
            );
            let serialized = serde_json::to_string(&check).unwrap();
            assert!(!serialized.contains(secret), "{command:?}");
            assert!(!serialized.contains("No external executable required"));
        }
    }

    #[test]
    fn required_tools_downgrades_dynamic_and_complex_shell_commands() {
        for command in [
            "$DOCTOR_DYNAMIC_TOOL test",
            "eval 'doctor-eval-missing-tool --version'",
            "doctor_fn() { :; }; doctor_fn",
            "cargo \"$(missing-helper)\" test",
            "cargo test >\"$(missing-helper)\"",
            "cat <<EOF\n$(missing-helper)\nEOF",
        ] {
            let temp = tempdir().unwrap();
            write_doctor_fixture_with_bootstrap_command(temp.path(), command);
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
            let check = required_tools_check_with_environment(
                &ctx,
                &DoctorEnvironment {
                    search_path: Some(OsString::new()),
                    ..DoctorEnvironment::default()
                },
            );

            assert!(check.ok, "{command:?}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{command:?}");
            assert!(
                check.detail.contains("must be run to verify"),
                "{command:?}"
            );
            assert!(!check.detail.contains("Missing command"), "{command:?}");
            let serialized = serde_json::to_string(&check).unwrap();
            assert!(!serialized.contains("missing-helper"), "{command:?}");
            assert!(!serialized.contains("DOCTOR_DYNAMIC_TOOL"), "{command:?}");
            assert!(
                !serialized.contains("doctor-eval-missing-tool"),
                "{command:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_preserve_known_presence_but_downgrade_inherited_shell_state() {
        for issue in [
            ShellEnvironmentIssue::BashEnv,
            ShellEnvironmentIssue::PosixEnv,
            ShellEnvironmentIssue::CdPath,
            ShellEnvironmentIssue::ImportedFunction,
        ] {
            let repo = tempdir().unwrap();
            write_doctor_fixture_with_bootstrap_command(repo.path(), "env cargo test");
            let tools = tempdir().unwrap();
            for executable in ["env", "cargo"] {
                write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
            }
            let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
            let mut environment = doctor_environment(tools.path(), None);
            environment.shell_environment_issue = Some(issue);

            let check = required_tools_check_with_environment(&ctx, &environment);

            assert!(check.ok, "{issue:?}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{issue:?}");
            let tool = check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "bootstrap_command")
                .unwrap();
            assert!(tool["present"].is_null(), "{issue:?}");
            let programs = tool["programs"].as_array().unwrap();
            assert_eq!(programs[0]["program"], "env", "{issue:?}");
            assert_eq!(programs[0]["present"], true, "{issue:?}");
            assert_eq!(programs[1]["program"], "cargo", "{issue:?}");
            assert_eq!(programs[1]["present"], true, "{issue:?}");
            assert!(programs.last().unwrap()["present"].is_null(), "{issue:?}");
            assert!(
                !serde_json::to_string(&check)
                    .unwrap()
                    .contains("No external executable required")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_downgrade_prior_dispatch_mutations() {
        for (command, target) in [
            ("hash -p /tmp/shim cargo; cargo test", "cargo"),
            ("enable -f /tmp/plugin custom; custom", "custom"),
            ("trap 'missing-helper' DEBUG; cargo test", "cargo"),
        ] {
            let repo = tempdir().unwrap();
            write_doctor_fixture_with_bootstrap_command(repo.path(), command);
            let tools = tempdir().unwrap();
            write_test_executable(&tools.path().join(target), "#!/bin/sh\nexit 0\n");
            let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

            let check = required_tools_check_with_environment(
                &ctx,
                &doctor_environment(tools.path(), None),
            );

            assert!(check.ok, "{command:?}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{command:?}");
            let tool = check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "bootstrap_command")
                .unwrap();
            assert!(tool["present"].is_null(), "{command:?}");
            let target = tool["programs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|program| program["program"] == target)
                .unwrap();
            assert!(target["present"].is_null(), "{command:?}");
            let serialized = serde_json::to_string(&check).unwrap();
            assert!(!serialized.contains("/tmp/shim"), "{command:?}");
            assert!(!serialized.contains("/tmp/plugin"), "{command:?}");
            assert!(!serialized.contains("missing-helper"), "{command:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_resolve_literal_relative_and_empty_path_from_repo_root() {
        let _env = lock_env();
        for (command, relative_executable) in [
            ("PATH=bin cargo test", "bin/cargo"),
            ("PATH= cargo test", "cargo"),
        ] {
            let repo = tempdir().unwrap();
            write_doctor_fixture_with_bootstrap_command(repo.path(), command);
            let invocation = repo.path().join("invocation/subdir");
            fs::create_dir_all(&invocation).unwrap();
            let executable = repo.path().join(relative_executable);
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            write_test_executable(&executable, "#!/bin/sh\nexit 0\n");
            let _cwd = CurrentDirGuard::set(&invocation);
            let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

            let check = required_tools_check_with_environment(
                &ctx,
                &DoctorEnvironment {
                    search_path: Some(invocation.as_os_str().to_os_string()),
                    ..DoctorEnvironment::default()
                },
            );

            assert!(check.ok, "{command:?}: {}", check.detail);
            assert_eq!(check.status, "present", "{command:?}");
            let tool = check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "bootstrap_command")
                .unwrap();
            assert_eq!(tool["present"], true, "{command:?}");
            assert_eq!(tool["programs"][0]["present"], true, "{command:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_accept_external_env_non_bash_assignment_names() {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), "env FOO.BAR=x cargo test");
        let tools = tempdir().unwrap();
        for executable in ["env", "cargo"] {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present");
        let programs = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap()["programs"]
            .as_array()
            .unwrap();
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0]["program"], "env");
        assert_eq!(programs[1]["program"], "cargo");
        assert!(programs.iter().all(|program| program["present"] == true));

        fs::remove_file(tools.path().join("cargo")).unwrap();
        let missing =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));
        assert!(!missing.ok);
        assert_eq!(missing.status, "missing");
        let programs = missing.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap()["programs"]
            .as_array()
            .unwrap();
        assert_eq!(programs[0]["program"], "env");
        assert_eq!(programs[0]["present"], true);
        assert_eq!(programs[1]["program"], "cargo");
        assert_eq!(programs[1]["present"], false);
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_avoid_cwd_false_present_and_false_missing_results() {
        for tool_location in ["root", "sub"] {
            let temp = tempdir().unwrap();
            let sub = temp.path().join("sub");
            fs::create_dir(&sub).unwrap();
            write_doctor_fixture_with_bootstrap_command(
                temp.path(),
                "env -C sub ./doctor-cwd-tool",
            );
            let tool = if tool_location == "root" {
                temp.path().join("doctor-cwd-tool")
            } else {
                sub.join("doctor-cwd-tool")
            };
            write_test_executable(&tool, "#!/bin/sh\nexit 0\n");
            let tools = tempdir().unwrap();
            write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

            let check = required_tools_check_with_environment(
                &ctx,
                &DoctorEnvironment {
                    search_path: Some(tools.path().as_os_str().to_os_string()),
                    ..DoctorEnvironment::default()
                },
            );

            assert!(check.ok, "{tool_location}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{tool_location}");
            let bootstrap = check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "bootstrap_command")
                .unwrap();
            assert!(bootstrap["present"].is_null(), "{tool_location}");
            assert_eq!(bootstrap["programs"][0]["program"], "env");
            assert_eq!(bootstrap["programs"][0]["present"], true);
            assert!(bootstrap["programs"][1]["present"].is_null());
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_does_not_probe_driver_from_assignment_removed_by_wrapper() {
        let temp = tempdir().unwrap();
        let secret = "doctor-removed-assignment-secret";
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            &format!(
                "DATABASE_URL=postgres://doctor:{secret}@localhost/demo env -u DATABASE_URL cargo-sqlx sqlx prepare"
            ),
        );
        let tools = tempdir().unwrap();
        let marker = tools.path().join("probe-marker");
        write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
        write_test_executable(
            &tools.path().join("cargo-sqlx"),
            &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &doctor_environment(tools.path(), Some("sqlite:ambient.db")),
        );

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert!(!marker.exists());
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["driver"],
            json!(null)
        );
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
            "unverified"
        );
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(secret));
        assert!(!serialized.contains("postgres://doctor"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_preserve_sqlx_probe_through_external_wrapper_chain() {
        let repo = tempdir().unwrap();
        let tools = tempdir().unwrap();
        for executable in ["env", "nohup", "time"] {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let marker = tools.path().join("sqlx-probe-marker");
        write_test_executable(
            &tools.path().join("sqlx"),
            &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
        );
        let time = tools.path().join("time");
        write_sqlx_doctor_fixture_with_command(
            repo.path(),
            &format!(
                "env nohup {} sqlx prepare -D sqlite:wrapper-chain.db",
                time.display()
            ),
        );
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present");
        assert!(marker.exists());
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
            "compatible"
        );
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(&time.display().to_string()));
        assert!(!serialized.contains("wrapper-chain.db"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_redacts_sqlx_commands_even_when_resolution_is_ambiguous() {
        let temp = tempdir().unwrap();
        let secret = "doctor-inline-password";
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            &format!(
                "cargo sqlx prepare --database-url='postgres://doctor-user:{secret}@localhost/demo"
            ),
        );

        let tools = tempdir().unwrap();
        let bin = tools.path().to_path_buf();
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
        assert!(check.ok);
        assert_eq!(check.status, "present_unverified");
        assert!(check.fix.is_none());
        assert_eq!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "sqlx_check_command")
                .unwrap()["command"],
            "<redacted: sqlx_check_command>"
        );

        let output = output(None, vec![check]);
        let serialized = serde_json::to_string(&output).unwrap();
        let summary = format_summary(&output);
        assert!(!serialized.contains(secret));
        assert!(!serialized.contains("postgres://doctor-user"));
        assert!(!summary.contains(secret));
        assert!(!summary.contains("postgres://doctor-user"));
        assert!(summary.contains("present_unverified"));
        assert!(summary.contains("scripts/jig check sqlx"));
        assert!(summary.contains("Next required step: none"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_redact_unquoted_database_url_expansion_values() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "cargo sqlx prepare --database-url=$DATABASE_URL",
        );
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
        let secret = "doctor-unquoted-expansion-secret";
        let database_url =
            format!("sqlite:first.db -D postgres://doctor:{secret}@localhost/injected");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &doctor_environment(tools.path(), Some(&database_url)),
        );

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(secret));
        assert!(!serialized.contains("postgres://doctor"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_nearest_dotenv_diagnostics_do_not_leak_values_or_home() {
        let temp = tempdir().unwrap();
        let child = temp.path().join("crates/api");
        fs::create_dir_all(&child).unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), "cd crates/api && cargo sqlx prepare");
        let parent_secret = "parent-database-secret";
        let child_secret = "nearest-unrelated-secret";
        fs::write(
            temp.path().join(".env"),
            format!("DATABASE_URL=postgres://doctor:{parent_secret}@localhost/demo\n"),
        )
        .unwrap();
        fs::write(child.join(".env"), format!("OTHER_VALUE={child_secret}\n")).unwrap();
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&tools.path().join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        let output = output(None, vec![check]);
        let serialized = serde_json::to_string(&output).unwrap();
        let summary = format_summary(&output);
        for rendered in [&serialized, &summary] {
            assert!(!rendered.contains(parent_secret));
            assert!(!rendered.contains(child_secret));
            if let Some(home) = env::var_os("HOME").and_then(|home| home.into_string().ok()) {
                assert!(!rendered.contains(&home));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_redacts_url_tokens_misparsed_as_sqlx_executables() {
        let temp = tempdir().unwrap();
        let secret = "misparsed-inline-password";
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            &format!(
                "postgres://doctor-user:{secret}@localhost/demo; cargo sqlx prepare --database-url='$DYNAMIC_DATABASE_URL'"
            ),
        );

        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
        assert!(!check.ok);
        assert_eq!(check.status, "missing");
        let serialized = serde_json::to_string(&check).unwrap();
        let summary = format_summary(&output(None, vec![check]));
        assert!(!serialized.contains(secret));
        assert!(!serialized.contains("postgres://doctor-user"));
        assert!(!summary.contains(secret));
        assert!(!summary.contains("postgres://doctor-user"));
        assert!(serialized.contains("<redacted: command executable>"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_treats_indeterminate_sqlx_probe_as_present_unverified() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "cargo-sqlx sqlx prepare -D sqlite:doctor.db",
        );

        let tools = tempdir().unwrap();
        let bin = tools.path().to_path_buf();
        write_test_executable(
            &bin.join("cargo-sqlx"),
            "#!/bin/sh\nprintf '%s\\n' 'unexpected doctor probe response'\nexit 2\n",
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &doctor_environment(&bin, Some("sqlite:doctor.db")),
        );
        assert!(check.ok);
        assert_eq!(check.status, "present_unverified");
        assert!(check.fix.is_none());
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
            "unverified"
        );
        assert!(cargo_sqlx_program(&check)["driver_probe"]["compatible"].is_null());
        assert!(check.detail.contains("scripts/jig check sqlx"));
        assert!(check.detail.contains("in the SQLx CLI"));
        assert!(!check.detail.contains("in cargo-sqlx"));
        assert!(!check.detail.contains("reinstall"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_does_not_execute_sqlx_probe_with_shell_environment_poisoning() {
        use std::os::unix::ffi::OsStringExt;

        let secret = "shell-environment-poison-secret";
        let issue = |controls: [Option<&OsStr>; 7], variables: Vec<(OsString, OsString)>| {
            inherited_shell_environment_issue(
                [
                    (ShellEnvironmentIssue::BashEnv, controls[0]),
                    (ShellEnvironmentIssue::PosixEnv, controls[1]),
                    (ShellEnvironmentIssue::CdPath, controls[2]),
                    (ShellEnvironmentIssue::ShellOptions, controls[3]),
                    (ShellEnvironmentIssue::BashOptions, controls[4]),
                    (ShellEnvironmentIssue::TracePrompt, controls[5]),
                    (ShellEnvironmentIssue::TraceFileDescriptor, controls[6]),
                ],
                variables,
            )
        };
        let scenarios = [
            issue(
                [Some(OsStr::new(secret)), None, None, None, None, None, None],
                Vec::new(),
            ),
            issue(
                [None, Some(OsStr::new(secret)), None, None, None, None, None],
                Vec::new(),
            ),
            issue(
                [None, None, Some(OsStr::new(secret)), None, None, None, None],
                Vec::new(),
            ),
            issue(
                [None, None, None, Some(OsStr::new(secret)), None, None, None],
                Vec::new(),
            ),
            issue(
                [None, None, None, None, Some(OsStr::new(secret)), None, None],
                Vec::new(),
            ),
            issue(
                [None, None, None, None, None, Some(OsStr::new(secret)), None],
                Vec::new(),
            ),
            issue(
                [None, None, None, None, None, None, Some(OsStr::new(secret))],
                Vec::new(),
            ),
            issue(
                [None; 7],
                vec![(
                    OsString::from("BASH_FUNC_sqlx%%"),
                    OsString::from(format!("() {{ printf {secret}; }}")),
                )],
            ),
            issue(
                [None; 7],
                vec![(
                    OsString::from_vec(b"BASH_FUNC_sqlx_\xff%%".to_vec()),
                    OsString::from(format!("() {{ printf {secret}; }}")),
                )],
            ),
        ];
        assert_eq!(
            scenarios,
            [
                Some(ShellEnvironmentIssue::BashEnv),
                Some(ShellEnvironmentIssue::PosixEnv),
                Some(ShellEnvironmentIssue::CdPath),
                Some(ShellEnvironmentIssue::ShellOptions),
                Some(ShellEnvironmentIssue::BashOptions),
                Some(ShellEnvironmentIssue::TracePrompt),
                Some(ShellEnvironmentIssue::TraceFileDescriptor),
                Some(ShellEnvironmentIssue::ImportedFunction),
                Some(ShellEnvironmentIssue::ImportedFunction),
            ]
        );

        for (index, issue) in scenarios.into_iter().enumerate() {
            let temp = tempdir().unwrap();
            write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:doctor.db");
            let bin = temp.path().join("bin");
            fs::create_dir(&bin).unwrap();
            let marker = temp.path().join(format!("probe-ran-{index}"));
            write_test_executable(
                &bin.join("sqlx"),
                &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
            );
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
            let mut environment = doctor_environment(&bin, Some("sqlite:doctor.db"));
            environment.shell_environment_issue = issue;

            let check = required_tools_check_with_environment(&ctx, &environment);

            assert!(check.ok, "{}", check.detail);
            assert_eq!(check.status, "present_unverified");
            assert!(
                check
                    .detail
                    .contains("external executable reference(s) inspected")
            );
            assert!(
                !marker.exists(),
                "ambient shell state allowed probe execution"
            );
            let probe = &cargo_sqlx_program(&check)["driver_probe"];
            assert!(probe["driver"].is_null());
            assert!(probe["source"].is_null());
            assert_eq!(probe["status"], "unverified");
            let serialized = serde_json::to_string(&check).unwrap();
            assert!(!serialized.contains(secret));
            assert!(serialized.contains("inherited shell state"));
        }

        assert_eq!(issue([None; 7], Vec::new()), None);
        assert_eq!(issue([Some(OsStr::new("")); 7], Vec::new()), None);
    }

    #[test]
    fn doctor_environment_capture_audits_bash_startup_state_without_retaining_values() {
        let _env = lock_env();
        let _posix_env = EnvVarGuard::remove("ENV");
        let _cdpath = EnvVarGuard::remove("CDPATH");
        let secret = "doctor-bash-env-secret";
        let _bash_env = EnvVarGuard::set("BASH_ENV", secret);

        let environment = DoctorEnvironment::capture();

        assert_eq!(
            environment.shell_environment_issue,
            Some(ShellEnvironmentIssue::BashEnv)
        );
        assert!(!format!("{environment:?}").contains(secret));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_does_not_trust_ambiguous_cargo_sqlx_dispatch() {
        for case in [
            "environment",
            "command_environment",
            "inline",
            "inline_include",
            "config",
            "config_include",
            "nested_config",
            "relative_cargo_home",
        ] {
            let temp = tempdir().unwrap();
            let command = match case {
                "command_environment" => {
                    "CARGO_ALIAS_SQLX='run --package fake' cargo sqlx prepare -D sqlite:doctor.db"
                }
                "inline" => {
                    "cargo --config alias.sqlx='run --package fake' sqlx prepare -D sqlite:doctor.db"
                }
                "inline_include" => {
                    "cargo --config include='dispatch.toml' sqlx prepare -D sqlite:doctor.db"
                }
                "nested_config" => "cd crates/api && cargo sqlx prepare -D sqlite:doctor.db",
                _ => "cargo sqlx prepare -D sqlite:doctor.db",
            };
            write_sqlx_doctor_fixture_with_command(temp.path(), command);
            if matches!(case, "config" | "config_include" | "nested_config") {
                let config_dir = if case == "nested_config" {
                    temp.path().join("crates/api/.cargo")
                } else {
                    temp.path().join(".cargo")
                };
                fs::create_dir_all(&config_dir).unwrap();
                fs::write(
                    config_dir.join("config.toml"),
                    if case == "config_include" {
                        "include = 'dispatch.toml'\n"
                    } else {
                        "[alias]\nsqlx = 'run --package fake'\n"
                    },
                )
                .unwrap();
            }
            let tools = tempdir().unwrap();
            write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
            let mut environment = doctor_environment(tools.path(), None);
            if case == "environment" {
                environment.cargo_alias_sqlx = Some("run --package fake".into());
            } else if case == "relative_cargo_home" {
                environment.cargo_home = Some("relative-cargo-home".into());
            }

            let check = required_tools_check_with_environment(&ctx, &environment);

            assert!(check.ok, "{case}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{case}");
            assert!(check.detail.contains("cargo sqlx dispatch"), "{case}");
            if matches!(
                case,
                "inline" | "inline_include" | "config" | "config_include" | "nested_config"
            ) {
                assert!(check.detail.contains("config"), "{case}: {}", check.detail);
            }
            if matches!(case, "environment" | "command_environment") {
                let detail = check.detail.to_ascii_lowercase();
                assert!(
                    detail.contains("alias") || detail.contains("home"),
                    "{case}: {}",
                    check.detail,
                );
            }
            if case == "relative_cargo_home" {
                assert!(check.detail.contains("config"), "{case}: {}", check.detail);
            }
            assert_eq!(cargo_sqlx_program(&check)["present"], true, "{case}");
            assert_eq!(
                cargo_sqlx_program(&check)["driver_probe"]["status"],
                "unverified",
                "{case}",
            );
            assert!(
                check.data["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|tool| tool["programs"].as_array().unwrap())
                    .all(|program| program["program"] != "cargo-sqlx"),
                "{case}",
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn unresolved_cargo_does_not_probe_an_external_subcommand() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "cargo sqlx prepare -D sqlite:doctor.db",
        );
        let tools = tempdir().unwrap();
        let probe_marker = temp.path().join("probe-marker");
        write_test_executable(
            &tools.path().join("cargo-sqlx"),
            &format!(
                "#!/bin/sh\nprintf probed > '{}'\nexit 0\n",
                probe_marker.display()
            ),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(!check.ok);
        assert_eq!(check.status, "missing");
        assert!(!probe_marker.exists());
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
            "unverified"
        );
        assert!(check.detail.contains("external cargo path does not prove"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_reports_explicit_cargo_wrapper_without_subcommand_probe() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join("scripts")).unwrap();
        let cargo = temp.path().join("scripts/cargo");
        write_test_executable(&cargo, "#!/bin/sh\nexit 0\n");
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "scripts/cargo sqlx prepare -D sqlite:doctor.db",
        );
        let tools = tempdir().unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert!(check.detail.contains("external cargo path does not prove"));
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .all(|program| program["program"] != "cargo-sqlx")
        );
    }

    #[cfg(unix)]
    #[test]
    fn cargo_alias_leaves_cargo_unverified_while_direct_clis_probe() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "cargo sqlx prepare -D sqlite:alias.db && sqlx prepare -D sqlite:direct.db && cargo-sqlx sqlx prepare -D sqlite:shim.db",
        );
        let tools = tempdir().unwrap();
        let probe_log = temp.path().join("probe-log");
        write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(
            &tools.path().join("sqlx"),
            &format!("#!/bin/sh\nprintf d >> '{}'\nexit 0\n", probe_log.display()),
        );
        write_test_executable(
            &tools.path().join("cargo-sqlx"),
            &format!("#!/bin/sh\nprintf c >> '{}'\nexit 0\n", probe_log.display()),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(tools.path(), None);
        environment.cargo_alias_sqlx = Some("run --package fake".into());

        let check = required_tools_check_with_environment(&ctx, &environment);

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert_eq!(fs::read_to_string(probe_log).unwrap(), "dc");
        let probes = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .filter_map(|program| program.get("driver_probe"))
            .collect::<Vec<_>>();
        assert_eq!(
            probes
                .iter()
                .filter(|probe| probe["status"] == "unverified")
                .count(),
            1
        );
        assert_eq!(
            probes
                .iter()
                .filter(|probe| probe["status"] == "compatible")
                .count(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_never_probes_cargo_subcommand_dispatch() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "DATABASE_URL=sqlite:first.db cargo sqlx prepare && cargo sqlx migrate info --database-url=sqlite:second.db",
        );

        let tools = tempdir().unwrap();
        let bin = tools.path().to_path_buf();
        let probe_count = temp.path().join("probe-count");
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(
            &bin.join("cargo-sqlx"),
            &format!(
                "#!/bin/sh\nprintf x >> '{}'\nexit 0\n",
                probe_count.display()
            ),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
        assert!(check.ok);
        assert_eq!(check.status, "present_unverified");
        assert!(!probe_count.exists());
        assert_eq!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .filter(|program| program.get("driver_probe").is_some())
                .count(),
            2
        );
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .filter_map(|program| program.get("driver_probe"))
                .all(|probe| probe["status"] == "unverified")
        );
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_neither_resolves_nor_probes_changed_path_invocations() {
        for repo_tool_is_present in [false, true] {
            let temp = tempdir().unwrap();
            let repo_tools = temp.path().join("repo-secret-bin");
            fs::create_dir(&repo_tools).unwrap();
            write_sqlx_doctor_fixture_with_command(
                temp.path(),
                "PATH=repo-secret-bin; sqlx prepare -D sqlite:path-secret.db",
            );

            let ambient = tempdir().unwrap();
            let marker = temp.path().join("path-probe-must-not-run");
            let body = format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display());
            if repo_tool_is_present {
                write_test_executable(&repo_tools.join("sqlx"), &body);
            } else {
                write_test_executable(&ambient.path().join("sqlx"), &body);
            }
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

            let check = required_tools_check_with_environment(
                &ctx,
                &doctor_environment(ambient.path(), None),
            );

            assert!(check.ok, "{}", check.detail);
            assert_eq!(check.status, "present_unverified");
            let sqlx_tool = check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["command_key"] == "sqlx_check_command")
                .unwrap();
            assert!(sqlx_tool["present"].is_null());
            assert!(sqlx_tool["programs"][0]["present"].is_null());
            assert_eq!(
                sqlx_tool["programs"][0]["driver_probe"]["status"],
                "unverified"
            );
            assert!(!marker.exists());

            let serialized = serde_json::to_string(&check).unwrap();
            assert!(!serialized.contains("repo-secret-bin"));
            assert!(!serialized.contains("path-secret"));
            assert!(serialized.contains("may change the executable lookup context"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_localizes_changed_path_and_only_probes_captured_path() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "PATH=repo-tools sqlx prepare -D sqlite:first.db && sqlx prepare -D sqlite:second.db",
        );
        let tools = tempdir().unwrap();
        let marker = temp.path().join("ambient-probe-count");
        let repo_tools = temp.path().join("repo-tools");
        fs::create_dir(&repo_tools).unwrap();
        write_test_executable(
            &repo_tools.join("sqlx"),
            &format!("#!/bin/sh\nprintf r >> '{}'\nexit 0\n", marker.display()),
        );
        write_test_executable(
            &tools.path().join("sqlx"),
            &format!("#!/bin/sh\nprintf x >> '{}'\nexit 0\n", marker.display()),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        let programs = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "sqlx_check_command")
            .unwrap()["programs"]
            .as_array()
            .unwrap();
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0]["present"], true);
        assert_eq!(programs[0]["driver_probe"]["status"], "unverified");
        assert_eq!(programs[1]["present"], true);
        assert_eq!(programs[1]["driver_probe"]["status"], "compatible");
        assert_eq!(fs::read_to_string(marker).unwrap(), "x");
    }

    #[cfg(unix)]
    #[test]
    fn doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches() {
        const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BATCH_HELPER";
        if let Some(root) = std::env::var_os(HELPER) {
            let root = PathBuf::from(root);
            let ctx = RepoContext::load_from_root(root.clone()).unwrap();
            let first = doctor_context_checks(&ctx);
            assert!(first.required_tools.ok, "{}", first.required_tools.detail);
            assert_eq!(
                cargo_sqlx_program(&first.required_tools)["driver_probe"]["status"],
                "compatible"
            );
            assert_eq!(first.agent.status, "missing", "{}", first.agent.detail);
            assert_eq!(first.agent.data["codex"]["available"], true);
            assert_eq!(first.proxy.status, "not running", "{}", first.proxy.detail);
            assert_eq!(
                fs::read_to_string(root.join("probe-count")).unwrap(),
                "dckp"
            );

            let second = doctor_context_checks(&ctx);
            assert!(second.required_tools.ok, "{}", second.required_tools.detail);
            assert_eq!(second.required_tools.status, "present");
            assert_eq!(
                cargo_sqlx_program(&second.required_tools)["driver_probe"]["status"],
                "compatible"
            );
            assert_eq!(second.agent.status, "missing", "{}", second.agent.detail);
            assert_eq!(second.agent.data["codex"]["available"], true);
            assert_eq!(
                second.proxy.status, "not running",
                "{}",
                second.proxy.detail
            );
            assert_eq!(
                fs::read_to_string(root.join("probe-count")).unwrap(),
                "dckpdckp"
            );
            return;
        }

        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "sqlx prepare -D sqlite:reusable.db && cargo-sqlx sqlx prepare -D sqlite:reusable.db",
        );
        let tools = tempdir().unwrap();
        write_test_executable(
            &tools.path().join("sqlx"),
            &format!(
                "#!/bin/sh\nprintf d >> '{}'\nexit 0\n",
                temp.path().join("probe-count").display()
            ),
        );
        write_test_executable(
            &tools.path().join("cargo-sqlx"),
            &format!(
                "#!/bin/sh\nprintf c >> '{}'\nexit 0\n",
                temp.path().join("probe-count").display()
            ),
        );
        fs::write(
            temp.path().join(".jig.toml"),
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
            ),
        )
        .unwrap();
        fs::create_dir(temp.path().join("web")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            fs::read_to_string(temp.path().join(".jig.toml"))
                .unwrap()
                .replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                ),
        )
        .unwrap();
        let codex = temp.path().join("codex");
        write_test_executable(
            &codex,
            &format!(
                "#!/bin/sh\nprintf k >> '{}'\n[ \"$*\" = \"plugin marketplace add --help\" ]\n",
                temp.path().join("probe-count").display()
            ),
        );
        write_test_executable(
            &temp.path().join("scripts/jig"),
            &format!(
                "#!/bin/sh\nprintf p >> '{}'\nprintf '%s\\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
                temp.path().join("probe-count").display()
            ),
        );
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches",
                "--nocapture",
            ])
            .env(HELPER, temp.path())
            .env("PATH", fs::canonicalize(tools.path()).unwrap())
            .env("JIG_CODEX_BIN", codex)
            .env("CODEX_HOME", temp.path().join("codex-home"))
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .env_remove("CDPATH")
            .status()
            .unwrap();
        assert!(
            status.success(),
            "reusable batch helper exited with {status}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn signal_retirement_failure_invalidates_every_configured_process_check() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:retirement.db");
        let config_path = temp.path().join(".jig.toml");
        fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
        fs::create_dir(temp.path().join("web")).unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let mut checks = DoctorContextChecks {
            required_tools: check(
                "required_tools",
                "Required tools",
                true,
                true,
                "present",
                "present",
            ),
            agent: check(
                "agent_skills",
                "Agent skills",
                false,
                true,
                "installed",
                "installed",
            ),
            proxy: check("proxy", "Proxy", false, true, "running", "running"),
        };

        mark_doctor_signal_retirement_failure(&ctx, &mut checks);

        assert_eq!(checks.required_tools.status, "present_unverified");
        assert!(
            checks
                .required_tools
                .detail
                .contains("could not retire safely")
        );
        for process_check in [&checks.agent, &checks.proxy] {
            assert!(!process_check.ok);
            assert_eq!(process_check.status, "error");
            assert!(process_check.detail.contains("could not retire safely"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_probes_bare_path_forms_but_not_explicit_sqlx_paths() {
        let temp = tempdir().unwrap();
        let tools = tempdir().unwrap();
        let bin = tools.path().to_path_buf();
        let probe_log = temp.path().join("probe-log");
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(
            &bin.join("sqlx"),
            &format!(
                "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nprintf d >> '{}'\nexit 0\n",
                probe_log.display()
            ),
        );
        write_test_executable(
            &bin.join("cargo-sqlx"),
            &format!(
                "#!/bin/sh\n[ \"$1\" = sqlx ] || exit 9\n[ \"$2\" = migrate ] || exit 9\nprintf c >> '{}'\nexit 0\n",
                probe_log.display()
            ),
        );
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            &format!(
                "CARGO=cargo sqlx prepare -D sqlite:direct.db && {} sqlx prepare -Dsqlite:shim.db && {} sqlx prepare -D=sqlite:cargo.db",
                bin.join("cargo-sqlx").display(),
                bin.join("cargo").display(),
            ),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert_eq!(fs::read_to_string(probe_log).unwrap(), "d");
        let probes = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .filter_map(|program| program.get("driver_probe"))
            .collect::<Vec<_>>();
        assert_eq!(probes.len(), 3);
        assert_eq!(
            probes
                .iter()
                .filter(|probe| probe["status"] == "compatible")
                .count(),
            1
        );
        assert_eq!(
            probes
                .iter()
                .filter(|probe| probe["status"] == "unverified")
                .count(),
            2
        );
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(&bin.display().to_string()));
        assert!(!serialized.contains("sqlite:direct.db"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_never_executes_repo_local_or_explicit_sqlx_tools() {
        let temp = tempdir().unwrap();
        let repo_bin = temp.path().join("bin");
        let repo_scripts = temp.path().join("scripts");
        fs::create_dir(&repo_bin).unwrap();
        fs::create_dir(&repo_scripts).unwrap();
        let external = tempdir().unwrap();
        let marker = temp.path().join("probe-must-not-run");
        let body = format!("#!/bin/sh\nprintf ran >> '{}'\nexit 0\n", marker.display());
        write_test_executable(&repo_bin.join("sqlx"), &body);
        write_test_executable(&repo_scripts.join("sqlx"), &body);
        write_test_executable(&external.path().join("sqlx"), &body);
        let relative_external = Path::new("..").join(
            external
                .path()
                .file_name()
                .expect("temporary tool directory has a basename"),
        );
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            &format!(
                "sqlx prepare -D sqlite:repo-path.db && scripts/sqlx prepare -D sqlite:repo-explicit.db && {}/sqlx prepare -D sqlite:custom-relative.db && {} prepare -D sqlite:custom-absolute.db",
                relative_external.display(),
                external.path().join("sqlx").display(),
            ),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(&repo_bin, None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert!(!marker.exists());
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(&temp.path().display().to_string()));
        assert!(!serialized.contains(&external.path().display().to_string()));

        let symlink_repo = tempdir().unwrap();
        let symlink_scripts = symlink_repo.path().join("scripts");
        let symlink_marker = symlink_repo.path().join("probe-must-not-run");
        write_sqlx_doctor_fixture_with_command(
            symlink_repo.path(),
            "sqlx prepare -D sqlite:symlink.db",
        );
        let symlink_body = format!(
            "#!/bin/sh\nprintf ran >> '{}'\nexit 0\n",
            symlink_marker.display()
        );
        write_test_executable(&symlink_scripts.join("sqlx"), &symlink_body);
        let symlink_path = tempdir().unwrap();
        std::os::unix::fs::symlink(
            symlink_scripts.join("sqlx"),
            symlink_path.path().join("sqlx"),
        )
        .unwrap();
        let symlink_ctx = RepoContext::load_from_root(symlink_repo.path().to_path_buf()).unwrap();
        let symlink_check = required_tools_check_with_environment(
            &symlink_ctx,
            &doctor_environment(symlink_path.path(), None),
        );
        assert!(symlink_check.ok, "{}", symlink_check.detail);
        assert_eq!(symlink_check.status, "present_unverified");
        assert!(!symlink_marker.exists());

        let linked_directory_repo = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            linked_directory_repo.path(),
            "sqlx prepare -D sqlite:linked-directory.db",
        );
        let real_tools = tempdir().unwrap();
        let linked_directory_marker = linked_directory_repo.path().join("probe-must-not-run");
        write_test_executable(
            &real_tools.path().join("sqlx"),
            &format!(
                "#!/bin/sh\nprintf ran > '{}'\nexit 0\n",
                linked_directory_marker.display()
            ),
        );
        let path_container = tempdir().unwrap();
        let linked_tools = path_container.path().join("linked-tools");
        std::os::unix::fs::symlink(real_tools.path(), &linked_tools).unwrap();
        let linked_directory_ctx =
            RepoContext::load_from_root(linked_directory_repo.path().to_path_buf()).unwrap();
        let linked_directory_check = required_tools_check_with_environment(
            &linked_directory_ctx,
            &DoctorEnvironment {
                search_path: Some(linked_tools.into_os_string()),
                ..DoctorEnvironment::default()
            },
        );
        assert!(
            linked_directory_check.ok,
            "{}",
            linked_directory_check.detail
        );
        assert_eq!(linked_directory_check.status, "present_unverified");
        assert!(!linked_directory_marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_ignores_commented_sqlx_urls_and_wrapper_separator() {
        let temp = tempdir().unwrap();
        let secret = "commented-database-secret";
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            &format!(
                "command -v cargo >/dev/null && command -- cargo sqlx prepare # -D postgres://doctor-user:{secret}@localhost/demo"
            ),
        );
        let tools = tempdir().unwrap();
        let bin = tools.path().to_path_buf();
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &doctor_environment(&bin, Some("sqlite:doctor.db")),
        );

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["driver"],
            "sqlite"
        );
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
            "unverified"
        );
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .all(|program| program["program"] != "cargo-sqlx")
        );
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .all(|program| !matches!(program["program"].as_str(), Some("--" | "-v")))
        );
        let output = output(None, vec![check]);
        let serialized = serde_json::to_string(&output).unwrap();
        let summary = format_summary(&output);
        for rendered in [&serialized, &summary] {
            assert!(!rendered.contains(secret));
            assert!(!rendered.contains("postgres://doctor-user"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_fails_open_for_nontransparent_wrapper_options() {
        for command in [
            "command -p cargo sqlx prepare -D sqlite:command-p-wrapper-secret.db",
            "exec -a private-argv-zero cargo sqlx prepare -D sqlite:exec-a-wrapper-secret.db",
            "exec -c cargo sqlx prepare",
        ] {
            let temp = tempdir().unwrap();
            write_sqlx_doctor_fixture_with_command(temp.path(), command);
            let bin = temp.path().join("bin");
            fs::create_dir(&bin).unwrap();
            write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
            write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
            let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

            let check = required_tools_check_with_environment(
                &ctx,
                &doctor_environment(&bin, Some("sqlite:ambient-wrapper-secret.db")),
            );

            assert!(check.ok, "{command:?}: {}", check.detail);
            assert_eq!(check.status, "present_unverified", "{command:?}");
            assert!(check.fix.is_none());
            assert!(
                check.data["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|tool| tool["programs"].as_array().unwrap())
                    .filter_map(|program| program["program"].as_str())
                    .all(|program| matches!(program, "cargo" | "cargo-sqlx")),
                "{command:?}",
            );
            let serialized = serde_json::to_string(&check).unwrap();
            for secret in [
                "command-p-wrapper-secret",
                "exec-a-wrapper-secret",
                "ambient-wrapper-secret",
                "private-argv-zero",
            ] {
                assert!(!serialized.contains(secret), "{command:?}: leaked {secret}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_marks_no_url_and_custom_sqlx_wrappers_unverified() {
        let no_url = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(no_url.path(), "cargo sqlx prepare --no-dotenv");
        let no_url_bin = no_url.path().join("bin");
        fs::create_dir(&no_url_bin).unwrap();
        write_test_executable(&no_url_bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&no_url_bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let no_url_ctx = RepoContext::load_from_root(no_url.path().to_path_buf()).unwrap();

        let no_url_check = required_tools_check_with_environment(
            &no_url_ctx,
            &doctor_environment(&no_url_bin, None),
        );
        assert!(no_url_check.ok, "{}", no_url_check.detail);
        assert_eq!(no_url_check.status, "present_unverified");
        assert!(no_url_check.fix.is_none());
        assert!(no_url_check.detail.contains("scripts/jig check sqlx"));
        assert!(
            !no_url_check
                .detail
                .to_ascii_lowercase()
                .contains("reinstall")
        );

        let wrapper = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(
            wrapper.path(),
            "scripts/private-sqlx-wrapper --check",
        );
        write_test_executable(
            &wrapper.path().join("scripts/private-sqlx-wrapper"),
            "#!/bin/sh\nexit 99\n",
        );
        let wrapper_bin = wrapper.path().join("bin");
        fs::create_dir(&wrapper_bin).unwrap();
        let wrapper_ctx = RepoContext::load_from_root(wrapper.path().to_path_buf()).unwrap();

        let wrapper_check = required_tools_check_with_environment(
            &wrapper_ctx,
            &doctor_environment(&wrapper_bin, None),
        );
        assert!(wrapper_check.ok, "{}", wrapper_check.detail);
        assert_eq!(wrapper_check.status, "present_unverified");
        assert!(wrapper_check.fix.is_none());
        let serialized = serde_json::to_string(&wrapper_check).unwrap();
        assert!(!serialized.contains("private-sqlx-wrapper"));
        assert!(serialized.contains("<redacted: command executable>"));
    }

    #[cfg(unix)]
    #[test]
    fn required_tools_redacts_every_command_body_and_generic_credential_token() {
        let temp = tempdir().unwrap();
        write_doctor_fixture(temp.path());
        let secret = "generic-required-command-secret";
        let config_path = temp.path().join(".jig.toml");
        let config = fs::read_to_string(&config_path).unwrap().replace(
            "bootstrap_command = \"printf bootstrap\"",
            &format!(
                "bootstrap_command = {:?}",
                format!("postgres://doctor-user:{secret}@localhost/demo --check")
            ),
        );
        fs::write(config_path, config).unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
        assert!(!check.ok);
        assert_eq!(check.status, "missing");
        assert_eq!(
            check.data["tools"][0]["command"],
            "<redacted: bootstrap_command>"
        );
        assert_eq!(check.data["tools"][0]["command_redacted"], true);
        let output = output(None, vec![check]);
        let serialized = serde_json::to_string(&output).unwrap();
        let summary = format_summary(&output);
        for rendered in [&serialized, &summary] {
            assert!(!rendered.contains(secret));
            assert!(!rendered.contains("postgres://doctor-user"));
        }
        assert!(serialized.contains("<redacted: command executable>"));
    }

    #[test]
    fn runtime_check_accepts_launcher_without_readable_pin_when_config_matches() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("scripts")).unwrap();
        fs::write(temp.path().join("scripts/jig"), "#!/usr/bin/env bash\n").unwrap();

        let output = runtime_check(temp.path(), Some(env!("CARGO_PKG_VERSION")));

        assert!(output.ok);
        assert_eq!(output.status, "unverified launcher");
        assert!(output.fix.is_none());
    }

    #[test]
    fn runtime_check_reports_unreadable_launcher() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("scripts/jig")).unwrap();

        let output = runtime_check(temp.path(), Some(env!("CARGO_PKG_VERSION")));

        assert!(!output.ok);
        assert_eq!(output.status, "mismatch");
        assert!(output.detail.contains("unreadable"));
        assert!(output.data["launcher_error"].as_str().is_some());
        assert!(output.fix.as_deref().unwrap().contains("jig update"));
    }

    #[test]
    fn agent_next_step_prefers_command_shaped_steps() {
        let steps = vec![
            json!("Codex CLI is not available on PATH."),
            json!("Run `scripts/jig agent bootstrap` to register skills."),
        ];

        assert_eq!(
            agent_next_step(&steps),
            Some("Run `scripts/jig agent bootstrap` to register skills.")
        );
    }

    #[test]
    fn summary_surfaces_optional_missing_agent_skills() {
        let output = json!({
            "ok": true,
            "repo": {
                "root": "/tmp/demo",
            },
            "checks": [
                {
                    "label": "Agent skills",
                    "status": "missing",
                    "required": false,
                    "ok": false,
                },
            ],
            "next_step": "Run `scripts/jig agent bootstrap`.",
        });

        let summary = format_summary(&output);

        assert!(summary.contains("Jig doctor: ready"));
        assert!(summary.contains("Agent skills: optional setup (missing, optional)"));
        assert!(summary.contains("Next required step: none"));
        assert!(summary.contains("Optional setup: scripts/jig agent bootstrap"));
    }

    #[test]
    fn summary_surfaces_required_tool_missing_detail() {
        let output = json!({
            "ok": false,
            "repo": {
                "root": "/tmp/demo",
            },
            "checks": [
                {
                    "label": "Required tools",
                    "status": "missing",
                    "required": true,
                    "ok": false,
                    "detail": "Missing command executable(s): schema_dump_command: scripts/dump-schema.sh",
                },
            ],
            "next_step": "Install the missing executable.",
        });

        let summary = format_summary(&output);

        assert!(summary.contains("Required tools: needs setup (missing, required)"));
        assert!(summary.contains(
            "Detail: Missing command executable(s): schema_dump_command: scripts/dump-schema.sh"
        ));
        assert!(summary.contains("Next required step: Install the missing executable."));
        assert!(summary.contains("Optional setup: none"));
    }

    #[test]
    fn doctor_reports_unified_readiness_checks() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        write_doctor_fixture(temp.path());
        let _cwd = CurrentDirGuard::set(temp.path());

        let output = run().unwrap();

        assert_eq!(output["command"], "doctor");
        assert_eq!(output["repo"]["name"], "demo");
        assert_eq!(output["checks"].as_array().unwrap().len(), 8);
        assert!(check_by_id(&output, "runtime")["ok"].as_bool().unwrap());
        assert!(check_by_id(&output, "config")["ok"].as_bool().unwrap());
        assert!(check_by_id(&output, "contract")["ok"].as_bool().unwrap());
        assert!(
            check_by_id(&output, "required_tools")["ok"]
                .as_bool()
                .unwrap()
        );
        assert!(
            check_by_id(&output, "agent_skills")["ok"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(check_by_id(&output, "agent_skills")["required"], false);
        assert_eq!(check_by_id(&output, "proxy")["status"], "not configured");
        assert!(check_by_id(&output, "proxy")["ok"].as_bool().unwrap());
        assert_eq!(check_by_id(&output, "vault")["required"], false);
    }

    #[test]
    fn doctor_reports_all_checks_when_config_is_invalid() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        fs::write(temp.path().join(".jig.toml"), "repo_name = \n").unwrap();
        fs::create_dir_all(temp.path().join("scripts")).unwrap();
        fs::write(
            temp.path().join("scripts/jig"),
            format!(
                "#!/usr/bin/env bash\nJIG_VERSION=\"{}\"\n",
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
        let _cwd = CurrentDirGuard::set(temp.path());

        let output = run().unwrap();

        assert_eq!(output["command"], "doctor");
        assert_eq!(output["checks"].as_array().unwrap().len(), 8);
        assert_eq!(check_by_id(&output, "config")["status"], "invalid");
        assert_eq!(check_by_id(&output, "contract")["status"], "blocked");
        assert_eq!(check_by_id(&output, "required_tools")["status"], "blocked");
        assert_eq!(check_by_id(&output, "agent_skills")["status"], "blocked");
        assert_eq!(check_by_id(&output, "proxy")["status"], "blocked");
        assert_eq!(check_by_id(&output, "vault")["status"], "blocked");
        for id in ["contract", "required_tools", "agent_skills", "proxy"] {
            assert!(
                check_by_id(&output, id)["detail"]
                    .as_str()
                    .unwrap()
                    .contains(".jig.toml")
            );
        }
        assert!(
            check_by_id(&output, "vault")["detail"]
                .as_str()
                .unwrap()
                .contains("repo context")
        );
        assert!(output["next_step"].as_str().unwrap().contains(".jig.toml"));
        assert!(
            output["next_required_step"]
                .as_str()
                .unwrap()
                .contains(".jig.toml")
        );
        assert!(output["optional_setup"].is_null());
        let summary = format_summary(&output);
        assert!(summary.contains("Next required step: Fix `.jig.toml`"));
        assert!(summary.contains("Optional setup: none"));
    }

    #[test]
    fn doctor_uses_configured_repo_root_before_current_directory() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let other = temp.path().join("other");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&other).unwrap();
        write_doctor_fixture(&repo);
        let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &repo);
        let _cwd = CurrentDirGuard::set(&other);

        let output = run().unwrap();

        assert_eq!(
            output["repo"]["root"],
            fs::canonicalize(&repo).unwrap().display().to_string()
        );
        assert_eq!(output["repo"]["name"], "demo");
    }

    #[test]
    fn doctor_reports_invalid_configured_repo_root() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let missing_config = temp.path().join("missing-config");
        fs::create_dir_all(&missing_config).unwrap();
        let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing_config);

        let output = run().unwrap();

        assert_eq!(output["ok"], false);
        assert_eq!(check_by_id(&output, "repo")["status"], "missing");
        assert!(
            check_by_id(&output, "repo")["detail"]
                .as_str()
                .unwrap()
                .contains("JIG_REPO_ROOT does not contain .jig.toml")
        );
        assert!(
            check_by_id(&output, "repo")["fix"]
                .as_str()
                .unwrap()
                .contains("init <path> --preset harness-only --repo-name <name> --sqlx-enabled false --no-input --no-vault")
        );
    }

    #[cfg(unix)]
    fn cargo_sqlx_program(check: &DoctorCheck) -> &Value {
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .find(|program| program.get("driver_probe").is_some())
            .unwrap()
    }

    #[cfg(unix)]
    fn doctor_environment(bin: &Path, database_url: Option<&str>) -> DoctorEnvironment {
        let bin = fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
        DoctorEnvironment {
            search_path: Some(bin.into_os_string()),
            path_extensions: None,
            database_url: database_url.map(OsString::from),
            cargo_alias_sqlx: None,
            cargo_home: None,
            home: None,
            probe_environment: Vec::new(),
            shell_environment_issue: None,
        }
    }

    #[cfg(unix)]
    fn write_test_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, body).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(windows)]
    fn wait_for_test_file(path: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(path.exists(), "timed out waiting for {}", path.display());
    }

    fn write_sqlx_doctor_fixture_with_command(root: &Path, command: &str) {
        write_doctor_fixture(root);
        let config_path = root.join(".jig.toml");
        let sqlx_config = format!(
            "sqlx_enabled = true\nrust_crate_roots = [\"crates\"]\nrust_migration_dir = \"migrations\"\nrust_sqlx_metadata_dir = \".sqlx\"\nschema_dump_enabled = false\nsqlx_check_command = {command:?}\n\n[agent_tooling.codex]"
        );
        let config = fs::read_to_string(&config_path)
            .unwrap()
            .replace("[agent_tooling.codex]", &sqlx_config);
        fs::write(config_path, config).unwrap();
        fs::create_dir(root.join("migrations")).unwrap();

        let contract_path = root.join(".agent/jig-contract.json");
        let mut contract: Value =
            serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
        contract["required_commands"]
            .as_array_mut()
            .unwrap()
            .push(json!("sqlx_check_command"));
        let tools = contract["tools"].as_array_mut().unwrap();
        tools.push(json!({
            "name": tool::SQLX_CHECK,
            "kind": "command",
            "description": "Run the configured SQLx check command.",
            "command": "sqlx_check_command",
        }));
        tools.push(json!({
            "name": tool::MIGRATION_ADD,
            "kind": "native",
            "description": "Add timestamped SQL migration stubs.",
        }));
        fs::write(
            contract_path,
            serde_json::to_string_pretty(&contract).unwrap(),
        )
        .unwrap();
    }

    fn write_doctor_fixture_with_bootstrap_command(root: &Path, command: &str) {
        write_doctor_fixture(root);
        let config_path = root.join(".jig.toml");
        let config = fs::read_to_string(&config_path).unwrap().replace(
            "bootstrap_command = \"printf bootstrap\"",
            &format!("bootstrap_command = {command:?}"),
        );
        fs::write(config_path, config).unwrap();
    }

    fn check_by_id<'a>(output: &'a Value, id: &str) -> &'a Value {
        output["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == id)
            .unwrap()
    }

    fn write_doctor_fixture(root: &Path) {
        fs::create_dir_all(root.join(".agent")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join(".jig.toml"),
            format!(
                r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 3,
                "tool_namespace": "jig",
                "jig_version": env!("CARGO_PKG_VERSION"),
                "required_commands": ["bootstrap_command"],
                "tools": [
                    {
                        "name": tool::CONTRACT_CHECK,
                        "kind": "native",
                        "description": "Contract check."
                    },
                    {
                        "name": tool::BOOTSTRAP,
                        "kind": "command",
                        "description": "Bootstrap.",
                        "command": "bootstrap_command"
                    }
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(root.join(".mcp.json"), "{}").unwrap();
        fs::write(root.join("scripts/install-jig.sh"), "#!/usr/bin/env bash\n").unwrap();
        fs::write(
            root.join("scripts/jig"),
            format!(
                "#!/usr/bin/env bash\nJIG_VERSION=\"{}\"\n",
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("scripts/jig"), fs::Permissions::from_mode(0o755))
                .unwrap();
        }
    }
}
