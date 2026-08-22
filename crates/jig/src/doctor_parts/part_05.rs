
fn proxy_list_failure_context(launcher: &Path) -> String {
    format!(
        "Failed to run proxy diagnostics through {}",
        launcher.display()
    )
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
