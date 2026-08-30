fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}
fn git_checked<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(ctx, cwd, args, observer)?;
    if !output.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git command failed",
            output,
        )));
    }
    Ok(())
}

fn git_stdout<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(ctx, cwd, args, observer)?;
    if !output.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git command failed",
            output,
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_output<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let label = pr_git_label(&args);
    git_execution_output(cwd, args, ctx.command_timeout(), observer)
        .map_err(|error| pr_git_execution_error(&label, error))
}

fn git_execution_output<I, S>(
    cwd: &Path,
    args: I,
    timeout: CommandTimeout,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Output, ExecutionCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let label = pr_git_label(&args);
    let mut command = git_command(cwd, &args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_authoritative_execution_command(
        &mut command,
        timeout,
        crate::execution::internal_execution_output_limit(),
        &label,
        observer,
    )?;
    Ok(Output {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn pr_git_label(args: &[OsString]) -> String {
    let operation = args
        .first()
        .map(|arg| arg.to_string_lossy())
        .unwrap_or_else(|| "command".into());
    format!("PR manager git {operation}")
}

fn pr_git_execution_error(label: &str, error: ExecutionCommandError) -> PrRepairStepError {
    match error {
        ExecutionCommandError::CancelledBeforeStart => {
            PrRepairStepError::Cancelled(format!("{label} was cancelled before it started"))
        }
        ExecutionCommandError::Cancelled => {
            PrRepairStepError::Cancelled(format!("{label} was cancelled while it was running"))
        }
        ExecutionCommandError::Failed { error, .. } => PrRepairStepError::Failed(error),
    }
}

fn git_command<I, S>(cwd: &Path, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    command
        .current_dir(cwd)
        .arg("--no-replace-objects")
        .args(args);
    // PR-manager commands target this known checkout but may fetch or push.
    // Strip repository redirection while retaining the transport/authentication
    // variables allowed by the shared known-repository policy.
    scrub_known_repository_git_environment(&mut command);
    command
}

fn git_error(label: &str, output: std::process::Output) -> anyhow::Error {
    anyhow!(
        "{} with status {}. stdout: {} stderr: {}",
        label,
        output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".into()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
