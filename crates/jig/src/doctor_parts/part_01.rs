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
            if let Some(rust_runtime) = context_checks.rust_runtime {
                checks.push(rust_runtime);
            }
            if let Some(node_runtime) = context_checks.node_runtime {
                checks.push(node_runtime);
            }
            if let Some(sqlx_cli) = context_checks.sqlx_cli {
                checks.push(sqlx_cli);
            }
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
        })),
        checks,
    ))
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
            probe_environment: [
                "SystemRoot",
                "WINDIR",
                "COMSPEC",
                "RUSTUP_HOME",
                "RUSTUP_TOOLCHAIN",
            ]
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
    rust_runtime: Option<DoctorCheck>,
    node_runtime: Option<DoctorCheck>,
    sqlx_cli: Option<DoctorCheck>,
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
    let rust_runtime = rust_runtime_check(ctx, environment, process_control);
    let node_runtime = node_runtime_check(ctx, environment, process_control);
    let sqlx_cli = sqlx_cli_version_check(ctx, environment, process_control);
    let agent = agent_check(ctx, process_control);
    let proxy = proxy_check_with_process_control(ctx, process_control);
    DoctorContextChecks {
        required_tools,
        rust_runtime,
        node_runtime,
        sqlx_cli,
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
    sqlx_probe_required
        || rust_runtime_probe_required(ctx)
        || node_runtime_probe_required(ctx)
        || !ctx.codex_marketplaces().is_empty()
        || proxy_configured(ctx)
}

#[cfg(unix)]
fn rust_runtime_probe_required(ctx: &RepoContext) -> bool {
    fs::symlink_metadata(ctx.root().join("Cargo.toml")).is_ok()
}

#[cfg(unix)]
fn node_runtime_probe_required(ctx: &RepoContext) -> bool {
    !ctx.frontend_apps().is_empty()
        && fs::symlink_metadata(ctx.root().join(".node-version")).is_ok()
}
