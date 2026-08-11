use std::collections::{HashSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
#[cfg(unix)]
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use anyhow::anyhow;
use anyhow::{Context, Result};
use jig_owned_process::{
    OwnedProcessTreeError, ProcessOutputLimits, run_owned_process_tree_with_output,
    run_owned_process_tree_with_output_limits,
};
use serde::Serialize;
use serde_json::{Value, json};

#[cfg(test)]
use crate::cli::format_doctor_summary_for_test as format_summary;
use crate::command::{VaultCommand, VaultStatusRequest};
#[cfg(test)]
use crate::context::{
    FALLBACK_RUNTIME_CACHE_BASE, GIT_RUNTIME_CACHE_BASE, RUNTIME_CACHE_PROFILE_SUFFIX,
};
use crate::context::{
    JIG_REPO_ROOT_ENV, RepoContext, find_repo_root_from, find_repo_root_from_or_env,
};
#[cfg(test)]
use crate::tool_defs::tool;

mod runtime;

#[cfg(test)]
use runtime::launcher_repair_staging_check_at;
use runtime::{
    contract_migration_check, launcher_repair_cache_check, launcher_repair_seed_stamp_is_present,
    launcher_repair_staging_check, legacy_version_cache_check, runtime_check,
};

const COMMAND: &str = "doctor";
const LAUNCHER_REPAIR_STAGING_DOCTOR_MIN_AGE: Duration = Duration::from_secs(5 * 60);
const SQLX_DRIVER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const CODEX_SUPPORT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PROXY_LIST_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(120);
const PROXY_LIST_STDOUT_LIMIT: usize = 8 * 1024 * 1024;
#[cfg(unix)]
const DOCTOR_SIGNAL_QUIESCENCE_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn run() -> Result<Value> {
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    // Doctor is capability-only so it can diagnose an invalid generated
    // launcher contract. Its explicit JIG_REPO_ROOT target therefore remains
    // authoritative instead of inheriting repository-scoped launcher state.
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
    if let Some(notice) = doctor_root_override_notice(&cwd, &root) {
        eprintln!("{notice}");
    }

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
    let manifest_contract_version = RepoContext::declared_contract_version_from_root(&root).ok();
    let (config_ok, repo_name, config_jig_version) = match &config_probe {
        Ok(probe) => (
            true,
            Some(probe.repo_name.clone()),
            probe.jig_version.clone(),
        ),
        Err(_) => (false, None, None),
    };
    let config_valid_for_launcher_repair =
        config_ok && crate::bootstrap::launcher_only_repair_answers_are_valid(&root);
    checks.push(config_check(&root, &config_probe));
    checks.push(runtime_check(
        &root,
        manifest_contract_version,
        config_jig_version.as_deref(),
        config_valid_for_launcher_repair,
    ));
    if let Some(staging_check) = launcher_repair_staging_check(&root) {
        checks.push(staging_check);
    }
    if let Some(legacy_cache_check) = legacy_version_cache_check(&root) {
        checks.push(legacy_cache_check);
    }
    if let Some(contract_version) = manifest_contract_version
        .filter(|version| launcher_repair_seed_stamp_is_present(&root, *version))
    {
        checks.push(launcher_repair_cache_check(&root, contract_version));
    }
    if let Some(contract_version) = manifest_contract_version.filter(|version| {
        crate::context::is_supported_contract_version(*version)
            && *version < crate::context::CURRENT_CONTRACT_VERSION
    }) {
        checks.push(contract_migration_check(&root, contract_version));
    }

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
                    "Dev proxy",
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
        ctx_result
            .as_ref()
            .map_err(std::string::ToString::to_string),
    ));

    Ok(output(
        Some(json!({
            "root": root.display().to_string(),
            "name": repo_name,
            "jig_version": config_jig_version,
            "runtime_version": env!("CARGO_PKG_VERSION"),
            "contract_version": manifest_contract_version,
        })),
        checks,
    ))
}

fn doctor_root_override_notice(cwd: &Path, selected_root: &Path) -> Option<String> {
    if !RepoContext::repo_root_override_is_set() {
        return None;
    }
    let local_root = find_repo_root_from(cwd).ok()?;
    let local_root = fs::canonicalize(local_root).ok()?;
    (local_root != selected_root).then(|| {
        format!(
            "jig doctor is using {JIG_REPO_ROOT_ENV}={} instead of the repository containing its invocation directory {}; unset {JIG_REPO_ROOT_ENV} to diagnose {}",
            selected_root.display(),
            local_root.display(),
            local_root.display(),
        )
    })
}

pub(crate) fn program_available_on_path(program: &str) -> bool {
    let Ok(command_cwd) = env::current_dir() else {
        return false;
    };
    let Some(search_path) = env::var_os("PATH") else {
        return false;
    };
    let path_extensions = env::var_os("PATHEXT");
    resolve_program(
        &command_cwd,
        program,
        Some(&search_path),
        path_extensions.as_deref(),
    )
    .is_some()
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

fn config_check(root: &Path, result: &Result<crate::context::RepoConfigProbe>) -> DoctorCheck {
    match result {
        Ok(probe) => check(
            "config",
            ".jig.toml",
            true,
            true,
            "valid",
            match &probe.jig_version {
                Some(version) => format!(
                    "repo_name={}, legacy jig_version={version}",
                    probe.repo_name
                ),
                None => format!("repo_name={}", probe.repo_name),
            },
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

fn contract_check(ctx: &RepoContext) -> DoctorCheck {
    let output = crate::policy::contract_check(ctx);
    if output.exit_status == 0 {
        check(
            "contract",
            "Contract",
            true,
            true,
            "valid",
            output.stdout.trim().to_string(),
        )
        .with_data(json!({ "exit_status": output.exit_status }))
    } else {
        check(
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
        }))
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

    const fn key(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::Sqlite => "SQLite",
        }
    }

    const fn probe_url(self) -> &'static str {
        match self {
            // The generic URL parser accepts this URL, then the PostgreSQL
            // driver rejects the invalid sslmode before opening a socket.
            Self::Postgres => "postgres://127.0.0.1/jig_doctor_probe?sslmode=jig-doctor-invalid",
            // Any migration bookkeeping is confined to this process-local DB.
            Self::Sqlite => "sqlite::memory:",
        }
    }

    const fn install_command(self) -> &'static str {
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
    const fn key(self) -> &'static str {
        match self {
            Self::CommandFlag => "command_flag",
            Self::CommandAssignment => "command_assignment",
            Self::Environment => "environment",
            Self::Dotenv => ".env",
            Self::DotenvExample => ".env.example",
        }
    }

    const fn description(self) -> &'static str {
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
    const fn description(self) -> &'static str {
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
        let Ok(signal_session) = DoctorSignalSession::start() else {
            return doctor_context_checks_with_process_control(
                ctx,
                &environment,
                DoctorProcessControl::unavailable(
                    "the process-wide doctor signal session is unavailable",
                ),
            );
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
                            (Some(_), SqlxDriverResolution::Indeterminate(reason))
                                if !sqlx_resolution_recorded =>
                            {
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
            '$' if quote != Some('\'')
                && chars.peek().is_some_and(|next| {
                    *next == '{' || *next == '_' || next.is_ascii_alphabetic()
                }) =>
            {
                return true;
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
pub(crate) struct DoctorSignalSession {
    _guard: MutexGuard<'static, ()>,
    generation: usize,
    previous_actions: Vec<(libc::c_int, libc::sigaction)>,
    retired: bool,
}

#[cfg(unix)]
#[derive(Clone, Copy)]
pub(crate) struct DoctorSignalCancellation {
    generation: usize,
}

#[cfg(unix)]
impl DoctorSignalCancellation {
    pub(crate) fn cancelled(self) -> bool {
        DOCTOR_SIGNAL_GENERATION.load(Ordering::SeqCst) == self.generation
            && DOCTOR_SIGNAL.load(Ordering::SeqCst) != 0
    }
}

#[cfg(unix)]
#[derive(Default)]
struct DoctorSignalRestoration {
    error: Option<std::io::Error>,
    handlers_may_remain: bool,
}

#[cfg(unix)]
impl DoctorSignalSession {
    pub(crate) fn start() -> std::io::Result<Self> {
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

    pub(crate) fn cancelled(&self) -> bool {
        self.cancellation().cancelled()
    }

    pub(crate) fn cancellation(&self) -> DoctorSignalCancellation {
        DoctorSignalCancellation {
            generation: self.generation,
        }
    }

    pub(crate) fn finish(mut self) -> std::io::Result<()> {
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
    codex_bin: &std::ffi::OsStr,
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
    codex_bin: &std::ffi::OsStr,
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
            return SqlxDriverProbe::Indeterminate(driver_probe_reason(&error).into());
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

const fn driver_probe_reason(error: &OwnedProcessTreeError) -> &'static str {
    match error {
        OwnedProcessTreeError::Start(_) => "the driver probe could not start",
        OwnedProcessTreeError::TimedOut => "the driver probe timed out",
        OwnedProcessTreeError::Cancelled => "the driver probe was cancelled",
        OwnedProcessTreeError::Await => "the driver probe could not be awaited",
        OwnedProcessTreeError::Cleanup => {
            "the driver probe process tree could not be cleaned up safely"
        }
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

fn agent_next_step(steps: &[Value]) -> Option<&str> {
    steps
        .iter()
        .filter_map(Value::as_str)
        .find(|step| step.contains("`scripts/jig "))
        .or_else(|| steps.iter().filter_map(Value::as_str).next())
}

pub(crate) fn proxy_configured(ctx: &RepoContext) -> bool {
    !ctx.frontend_apps().is_empty()
        || !ctx.dev_config().apps.is_empty()
        || ctx.dev_config().workspace_discovery
}

fn proxy_check_with_process_control(
    ctx: &RepoContext,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let label = "Dev proxy";
    let configured = proxy_configured(ctx);
    if !configured {
        return check(
            "proxy",
            label,
            false,
            true,
            "not configured",
            "no dev apps configured",
        )
        .with_data(json!({ "configured": false }));
    }
    match proxy_list_output(ctx, process_control) {
        Ok(output) => proxy_check_from_output(configured, output),
        Err(error) => check("proxy", label, false, false, "error", error.to_string())
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
        proxy_list_output_limits(),
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
        proxy_list_output_limits(),
        || false,
    )
}

fn proxy_list_output_limits() -> ProcessOutputLimits {
    ProcessOutputLimits {
        stdout: PROXY_LIST_STDOUT_LIMIT,
        ..ProcessOutputLimits::default()
    }
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
        "Dev proxy",
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
        let _ = write!(detail, " scope={scope}");
    }
    if let Some(scope_id) = output["vault_scope_id"].as_str() {
        let _ = write!(detail, " scope_id={scope_id}");
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
    const fn description(self) -> &'static str {
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
        }
        | ShellCommandName::NoExternalExecutable { .. } => return false,
        ShellCommandName::AmbiguousWrapper { .. } => return true,
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

const fn is_shell_separator_char(ch: char) -> bool {
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
    let Ok(config) = text.parse::<toml::Value>() else {
        return true;
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
mod tests;
