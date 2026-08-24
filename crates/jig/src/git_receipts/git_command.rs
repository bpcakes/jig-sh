use super::*;

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

pub(super) fn git_output_unchecked(root: &Path, args: &[&str], label: &str) -> Result<Output> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    configure_read_only_git_environment(&mut command);
    command
        .output()
        .with_context(|| format!("Failed to run {label} in {}", root.display()))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn git_output_unchecked_with_cancellation(
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
    run_git_command_with_cancellation(root, &mut command, label, cancelled)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn git_output_unchecked_with_cancellation(
    root: &Path,
    args: &[&str],
    label: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output> {
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    let output = git_output_unchecked(root, args, label)?;
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    Ok(output)
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

pub(super) fn git_hash_object(root: &Path, input: &[u8]) -> Result<String> {
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

    child
        .stdin
        .as_mut()
        .context("git hash-object stdin was not available")?
        .write_all(input)
        .context("Failed to write worktree fingerprint input to git hash-object")?;

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
pub(super) fn git_hash_object_with_cancellation(
    root: &Path,
    input: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let mut input_file =
        NamedTempFile::new().context("Failed to stage worktree fingerprint hash input")?;
    write_fingerprint_hash_input(&mut input_file, input, cancelled)?;
    let stdin = input_file
        .reopen()
        .context("Failed to reopen worktree fingerprint hash input")?;
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::from(stdin))
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

pub(super) fn write_fingerprint_hash_input(
    writer: &mut impl Write,
    input: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let collection = GitReceiptCollection::Cancellable(cancelled);
    for chunk in input.chunks(FINGERPRINT_HASH_WRITE_CHUNK) {
        collection.ensure_active()?;
        writer
            .write_all(chunk)
            .context("Failed to write worktree fingerprint hash input")?;
    }
    collection.ensure_active()?;
    writer
        .flush()
        .context("Failed to flush worktree fingerprint hash input")?;
    collection.ensure_active()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn git_hash_object_with_cancellation(
    root: &Path,
    input: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    let hash = git_hash_object(root, input)?;
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    Ok(hash)
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
    scrub_known_repository_git_environment(command);
    // Receipt and gate fingerprint probes are observational. In particular,
    // `git status` must not refresh stat data by taking an optional index lock.
    command.env("GIT_OPTIONAL_LOCKS", "0");
}

pub(crate) fn parse_diff_stat_output(stdout: &str) -> Result<DiffStat> {
    let mut diff_stat = DiffStat::default();
    for (index, line) in stdout.lines().enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("Unexpected git diff --numstat line {}: {}", index + 1, line);
        }
        diff_stat.files += 1;
        diff_stat.insertions += parse_numstat_count(fields[0], index + 1, "insertions")?;
        diff_stat.deletions += parse_numstat_count(fields[1], index + 1, "deletions")?;
    }
    Ok(diff_stat)
}

pub(super) fn parse_numstat_count(field: &str, line_number: usize, kind: &str) -> Result<u64> {
    if field == "-" {
        return Ok(0);
    }
    field.parse::<u64>().with_context(|| {
        format!("Invalid git diff --numstat {kind} count on line {line_number}: {field}")
    })
}
