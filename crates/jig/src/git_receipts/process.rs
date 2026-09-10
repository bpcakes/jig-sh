use super::*;

pub(super) fn changed_path_git_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_CHANGED_PATH_GIT_OUTPUT_BYTES
}

pub(super) fn worktree_status_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_WORKTREE_STATUS_OUTPUT_BYTES
}

pub(super) fn worktree_diff_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_WORKTREE_DIFF_OUTPUT_BYTES
}

pub(super) fn gate_scope_diff_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_GATE_SCOPE_DIFF_OUTPUT_BYTES
}

pub(super) fn worktree_status_entry_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_WORKTREE_STATUS_ENTRIES
}

pub(super) fn git_worktree_proof_stdout(
    root: &Path,
    args: &[&str],
    label: &str,
    limit: usize,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    git_bounded_proof_stdout(root, args, label, limit, "worktree proof", collection)
}

pub(super) fn git_worktree_proof_stdout_os(
    root: &Path,
    args: &[OsString],
    label: &str,
    limit: usize,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    git_bounded_proof_stdout_os(root, args, label, limit, "worktree proof", collection)
}

pub(super) fn git_bounded_proof_stdout_os(
    root: &Path,
    args: &[OsString],
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    collection.ensure_active()?;
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    git_bounded_proof_command_stdout(root, &mut command, label, limit, proof_kind, collection)
}

pub(super) fn git_bounded_proof_stdout(
    root: &Path,
    args: &[&str],
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    Ok(git_bounded_proof_output(root, args, label, limit, proof_kind, collection)?.stdout)
}

pub(super) fn git_bounded_proof_output(
    root: &Path,
    args: &[&str],
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Output> {
    git_bounded_proof_output_with_timeout(
        root,
        args,
        label,
        limit,
        proof_kind,
        collection,
        Duration::MAX,
    )
}

pub(super) fn git_bounded_proof_output_with_timeout(
    root: &Path,
    args: &[&str],
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
    timeout: Duration,
) -> Result<Output> {
    collection.ensure_active()?;
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    git_bounded_proof_command_output_with_timeout(
        root,
        &mut command,
        label,
        limit,
        proof_kind,
        collection,
        timeout,
    )
}

pub(super) fn git_bounded_proof_command_stdout(
    root: &Path,
    command: &mut Command,
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    Ok(
        git_bounded_proof_command_output(root, command, label, limit, proof_kind, collection)?
            .stdout,
    )
}

pub(super) fn git_bounded_proof_command_output(
    root: &Path,
    command: &mut Command,
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Output> {
    git_bounded_proof_command_output_with_timeout(
        root,
        command,
        label,
        limit,
        proof_kind,
        collection,
        Duration::MAX,
    )
}

pub(super) fn git_bounded_proof_command_output_with_timeout(
    root: &Path,
    command: &mut Command,
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
    timeout: Duration,
) -> Result<Output> {
    let output = git_bounded_proof_command_capture_with_timeout(
        root, command, label, limit, proof_kind, collection, timeout,
    )?;
    require_success(&output, |output| {
        format!(
            "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    collection.ensure_active()?;
    Ok(output)
}

fn git_bounded_proof_command_capture_with_timeout(
    root: &Path,
    command: &mut Command,
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
    timeout: Duration,
) -> Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_read_only_git_environment(command);
    let mut observer = GitReceiptProcessObserver { collection };
    let output = match run_owned_process_tree_with_output_policy_and_observer(
        command,
        timeout,
        ProcessOutputLimits {
            stdout: limit,
            stderr: MAX_GIT_ERROR_PREVIEW_BYTES as usize,
        },
        ProcessOutputOverflowPolicy::Error,
        &mut observer,
    ) {
        Ok(output) => output,
        Err(error) if error.is_cancellation() => {
            return Err(GitReceiptCollectionCancelled.into());
        }
        Err(
            error @ OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stdout),
        ) => {
            return Err(anyhow::Error::new(error).context(format!(
                "{label} exceeded the {proof_kind} Git output limit of {limit} bytes; split or reduce the change set before collecting evidence"
            )));
        }
        Err(
            error @ OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stderr),
        ) => {
            return Err(anyhow::Error::new(error).context(format!(
                "{label} exceeded the Git diagnostic output limit of {MAX_GIT_ERROR_PREVIEW_BYTES} bytes"
            )));
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!("Failed to run {label} in {}", root.display())));
        }
    };
    let stdout = output
        .stdout
        .context("supervised proof Git command did not capture stdout")?;
    let stderr = output
        .stderr
        .context("supervised proof Git command did not capture stderr")?;
    if !stdout.complete || !stderr.complete {
        bail!(
            "Failed to capture complete output from {label} in {}",
            root.display()
        );
    }
    let output = Output {
        status: output.status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    };
    collection.ensure_active()?;
    Ok(output)
}

#[derive(Debug)]
pub(crate) struct FreshnessGitObservationFailure(pub(crate) String);

impl std::fmt::Display for FreshnessGitObservationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FreshnessGitObservationFailure {}

fn freshness_git_diagnostic(output: &Output) -> FreshnessGitObservationFailure {
    // Git diagnostics can contain config values and private paths. Persist a
    // bounded category, exit status, and a concrete local diagnostic command.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = if stderr.contains("Permission denied") || stderr.contains("unable to access") {
        "Git could not access configuration or source; check permissions for the current user"
    } else if stderr.contains("fsmonitor") {
        "Git reported an fsmonitor problem; inspect repository fsmonitor configuration"
    } else if stderr.contains("dubious ownership") {
        "Git rejected repository ownership; inspect repository ownership and safe.directory"
    } else if stderr.contains("not a git repository") {
        "Git could not resolve repository metadata"
    } else if stderr.contains("Needed a single revision") || stderr.contains("unknown revision") {
        "Git could not resolve HEAD; verify that a commit exists"
    } else {
        "Git returned diagnostics or failed; run git status --porcelain=v1 and git ls-files --stage directly to inspect the failure"
    };
    FreshnessGitObservationFailure(format!("{detail} ({})", format_exit_status(&output.status)))
}

/// One bounded owned process tree for a fixed read-only Git batch. The caller
/// validates its complete framed output; diagnostics never become source bytes.
pub(crate) fn read_freshness_git_batch(
    root: &Path,
    command: &mut Command,
    limit: usize,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let output = git_bounded_proof_command_capture_with_timeout(
        root,
        command,
        "git freshness observation",
        limit,
        "target freshness",
        GitReceiptCollection::Cancellable(cancelled),
        timeout,
    )?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(freshness_git_diagnostic(&output).into());
    }
    Ok(output.stdout)
}

pub(super) struct GitReceiptProcessObserver<'a> {
    pub(super) collection: GitReceiptCollection<'a>,
}

impl OwnedProcessObserver for GitReceiptProcessObserver<'_> {
    fn cancelled(&mut self) -> bool {
        matches!(
            self.collection,
            GitReceiptCollection::Cancellable(cancelled) | GitReceiptCollection::Observed { cancelled, .. } if cancelled()
        )
    }

    fn output(&mut self, _stream: OwnedProcessOutputStream, output: &[u8]) {
        if let GitReceiptCollection::Observed { bytes, .. } = self.collection {
            bytes.set(bytes.get().saturating_add(output.len() as u64));
        }
    }
}

pub(super) fn git_changed_path_stdout(
    root: &Path,
    args: &[&str],
    label: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    git_bounded_proof_stdout(
        root,
        args,
        label,
        changed_path_git_output_limit(),
        "changed-path",
        collection,
    )
}

pub(super) fn git_output(root: &Path, args: &[&str], label: &str) -> Result<Output> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    configure_read_only_git_environment(&mut command);

    run_checked_output_with_context(
        &mut command,
        || format!("Failed to run {label} in {}", root.display()),
        |output| {
            format!(
                "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
                format_exit_status(&output.status),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        },
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn git_output_with_cancellation(
    root: &Path,
    args: &[&str],
    label: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_git_command_with_cancellation(root, &mut command, label, cancelled)?;
    require_success(&output, |output| {
        format!(
            "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok(output)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn git_output_with_cancellation(
    root: &Path,
    args: &[&str],
    label: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output> {
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    let output = git_output(root, args, label)?;
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    Ok(output)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn run_git_command_with_cancellation(
    root: &Path,
    command: &mut Command,
    label: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output> {
    configure_read_only_git_environment(command);
    let output = match run_owned_process_tree_with_output_limits(
        command,
        Duration::MAX,
        ProcessOutputLimits {
            stdout: usize::MAX,
            stderr: usize::MAX,
        },
        cancelled,
    ) {
        Ok(output) => output,
        Err(error) if error.is_cancellation() => {
            return Err(GitReceiptCollectionCancelled.into());
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!("Failed to run {label} in {}", root.display())));
        }
    };
    let stdout = output
        .stdout
        .context("supervised Git command did not capture stdout")?;
    let stderr = output
        .stderr
        .context("supervised Git command did not capture stderr")?;
    if !stdout.complete || !stderr.complete {
        bail!(
            "Failed to capture complete output from {label} in {}",
            root.display()
        );
    }
    if stdout.truncated || stderr.truncated {
        bail!(
            "Unexpected bounded output from {label} in {}",
            root.display()
        );
    }
    Ok(Output {
        status: output.status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

pub(super) fn git_hash_file(root: &Path, full_path: &Path) -> Result<String> {
    let mut file = fs::File::open(full_path)
        .with_context(|| format!("Failed to open untracked file {}", full_path.display()))?;
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_read_only_git_environment(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to start git hash-object in {}", root.display()))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .context("git hash-object stdin was not available")?;
        copy(&mut file, &mut stdin)
            .with_context(|| format!("Failed to hash untracked file {}", full_path.display()))?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for git hash-object")?;
    require_success(&output, |output| {
        format!(
            "git hash-object failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn git_hash_file_with_cancellation(
    root: &Path,
    full_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let file = fs::File::open(full_path)
        .with_context(|| format!("Failed to open untracked file {}", full_path.display()))?;
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output =
        run_git_command_with_cancellation(root, &mut command, "git hash-object", cancelled)?;
    require_success(&output, |output| {
        format!(
            "git hash-object failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn git_hash_file_with_cancellation(
    root: &Path,
    full_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    let hash = git_hash_file(root, full_path)?;
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    Ok(hash)
}

pub(super) fn configure_read_only_git_environment(command: &mut Command) {
    // Receipt and gate fingerprint probes are observational. In particular,
    // `git status` must not refresh stat data by taking an optional index lock.
    scrub_known_repository_git_environment(command);
    command.env("GIT_OPTIONAL_LOCKS", "0");
    command.env("LC_ALL", "C");
}
