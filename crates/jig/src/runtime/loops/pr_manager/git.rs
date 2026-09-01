const PR_MANAGER_GIT_NAME: &str = "Jig PR Manager";
const PR_MANAGER_GIT_EMAIL: &str = "jig-pr-manager@users.noreply.github.com";
const PR_WORKTREE_REF_PREFIX_LEN: usize = 48;

fn bounded_path_component(value: &str) -> String {
    let readable = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .take(PR_WORKTREE_REF_PREFIX_LEN)
        .collect::<String>();
    let readable = if readable.is_empty() {
        "branch"
    } else {
        &readable
    };
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    format!("{readable}-{digest}")
}

fn git_with_pr_manager_identity<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = vec![
        OsString::from("-c"),
        OsString::from(format!("user.name={PR_MANAGER_GIT_NAME}")),
        OsString::from("-c"),
        OsString::from(format!("user.email={PR_MANAGER_GIT_EMAIL}")),
    ];
    command.extend(
        args.into_iter()
            .map(|argument| argument.as_ref().to_os_string()),
    );
    command
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

fn conflict_marker_diagnostics(
    ctx: &RepoContext,
    cwd: &Path,
    baseline: &str,
    candidate_head: Option<&str>,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<BTreeSet<Vec<u8>>> {
    let mut args = vec![
        OsString::from("-c"),
        OsString::from(
            "core.whitespace=-trailing-space,-space-before-tab,-indent-with-non-tab,-tab-in-indent",
        ),
        OsString::from("diff"),
        OsString::from("--check"),
        OsString::from(baseline.trim()),
    ];
    if let Some(candidate_head) = candidate_head {
        args.push(OsString::from(candidate_head.trim()));
    }
    args.push(OsString::from("--"));
    let output = git_output(ctx, cwd, args, observer)?;
    if output.status.success() {
        return Ok(BTreeSet::new());
    }
    if output.stdout.is_empty() || !output.stderr.is_empty() {
        return Err(PrRepairStepError::failed(git_error(
            "git conflict-marker check failed",
            output,
        )));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

fn require_no_merge_introduced_conflict_markers(
    ctx: &RepoContext,
    cwd: &Path,
    observed_head: &str,
    incoming_base_head: Option<&str>,
    candidate_head: Option<&str>,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()> {
    let observed = conflict_marker_diagnostics(
        ctx,
        cwd,
        observed_head,
        candidate_head,
        observer,
    )?;
    let introduced = if let Some(incoming_base_head) = incoming_base_head {
        let incoming = conflict_marker_diagnostics(
            ctx,
            cwd,
            incoming_base_head,
            candidate_head,
            observer,
        )?;
        observed.intersection(&incoming).cloned().collect::<Vec<_>>()
    } else {
        observed.into_iter().collect()
    };
    if introduced.is_empty() {
        return Ok(());
    }
    Err(PrRepairStepError::failed(anyhow!(
        "PR repair contains conflict markers absent from its pre-worker parent tree(s):\n{}",
        introduced
            .iter()
            .map(|line| String::from_utf8_lossy(line))
            .collect::<Vec<_>>()
            .join("\n")
    )))
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
    let stdout = git_stdout_bytes(ctx, cwd, args, observer)?;
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}

fn git_stdout_path<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let stdout = git_stdout_bytes(ctx, cwd, args, observer)?;
    let path = trim_ascii_line(&stdout);
    if path.is_empty() || path.contains(&b'\r') || path.contains(&b'\n') {
        return Err(PrRepairStepError::failed(anyhow!(
            "git command returned an empty or multiline path"
        )));
    }
    Ok(path_from_git_bytes(path))
}

fn git_stdout_bytes<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Vec<u8>>
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
    Ok(output.stdout)
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
    let mut arguments = args.iter();
    let operation = loop {
        let Some(argument) = arguments.next() else {
            break None;
        };
        if argument == "-c" {
            let _ = arguments.next();
            continue;
        }
        break Some(argument);
    }
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
