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
    key == "DATABASE_URL"
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
        .env("TMPDIR", temp.path())
        .env("TMP", temp.path())
        .env("TEMP", temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = sanitized_probe_search_path(root, executable) {
        command.env("PATH", path);
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
        OwnedProcessTreeError::CancelledBeforeStart => "the driver probe was cancelled",
        OwnedProcessTreeError::Cancelled => "the driver probe was cancelled",
        OwnedProcessTreeError::OutputLimitExceeded(_) => {
            "the driver probe output exceeded the diagnostic capture limit"
        }
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
    let mut command = Command::new(&launcher);
    crate::shell::sanitize_bash_environment(&mut command);
    command.args(["proxy", "list", "--json"]).current_dir(&root);
    Ok((launcher, command))
}
