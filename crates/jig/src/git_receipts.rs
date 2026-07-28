use std::fs;
use std::io::{Read, Write, copy};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use tempfile::NamedTempFile;

#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use crate::process::{
    OwnedProcessTreeError, ProcessOutputLimits, format_exit_status, require_success,
    run_checked_output_with_context, run_owned_process_tree_with_output_limits,
};

const MAX_INLINE_UNTRACKED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_INLINE_UNTRACKED_BYTES: u64 = 32 * 1024 * 1024;
const FINGERPRINT_HASH_WRITE_CHUNK: usize = 64 * 1024;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DiffStat {
    pub(crate) files: usize,
    pub(crate) insertions: u64,
    pub(crate) deletions: u64,
}

#[derive(Debug, Default)]
pub(crate) struct GitReceiptMetadata {
    pub(crate) changed_paths: Vec<String>,
    pub(crate) diff_stat: DiffStat,
    pub(crate) git_status_error: Option<String>,
    pub(crate) git_diff_stat_error: Option<String>,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

pub(crate) fn collect_git_receipt_metadata(root: &Path) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(root, true)
}

pub(crate) fn collect_git_receipt_metadata_without_worktree_fingerprint(
    root: &Path,
) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(root, false)
}

fn collect_git_receipt_metadata_with_options(
    root: &Path,
    collect_worktree_fingerprint: bool,
) -> GitReceiptMetadata {
    let (changed_paths, git_status_error) = match repo_changed_paths(root) {
        Ok(changed_paths) => (changed_paths, None),
        Err(error) => (Vec::new(), Some(format!("{error:#}"))),
    };
    let (diff_stat, git_diff_stat_error) = match repo_diff_stat(root) {
        Ok(diff_stat) => (diff_stat, None),
        Err(error) => (DiffStat::default(), Some(format!("{error:#}"))),
    };
    let (worktree_fingerprint, worktree_fingerprint_error) = if collect_worktree_fingerprint {
        match repo_worktree_fingerprint(root) {
            Ok(fingerprint) => (Some(fingerprint), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        }
    } else {
        (None, None)
    };

    GitReceiptMetadata {
        changed_paths,
        diff_stat,
        git_status_error,
        git_diff_stat_error,
        worktree_fingerprint,
        worktree_fingerprint_error,
    }
}

fn repo_changed_paths(root: &Path) -> Result<Vec<String>> {
    let output = git_output(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        "git status --porcelain -z",
    )?;
    parse_porcelain_status_z(&output.stdout).map(|entries| {
        entries
            .into_iter()
            .flat_map(|entry| {
                let mut paths = vec![entry.path.display().to_string()];
                if let Some(original_path) = entry.original_path {
                    paths.push(original_path.display().to_string());
                }
                paths
            })
            .collect()
    })
}

fn repo_diff_stat(root: &Path) -> Result<DiffStat> {
    let output = git_output(root, &["diff", "--numstat"], "git diff --numstat")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diff_stat_output(&stdout)
}

pub(crate) fn repo_worktree_fingerprint(root: &Path) -> Result<String> {
    repo_worktree_fingerprint_inner(root, FingerprintCollection::Blocking)
}

pub(crate) fn repo_worktree_fingerprint_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    repo_worktree_fingerprint_inner(root, FingerprintCollection::Cancellable(cancelled))
}

pub(crate) fn is_worktree_fingerprint_cancellation(error: &anyhow::Error) -> bool {
    error.is::<WorktreeFingerprintCancelled>()
}

#[derive(Clone, Copy)]
enum FingerprintCollection<'a> {
    Blocking,
    Cancellable(&'a dyn Fn() -> bool),
}

impl FingerprintCollection<'_> {
    fn ensure_active(self) -> Result<()> {
        if matches!(self, Self::Cancellable(cancelled) if cancelled()) {
            return Err(WorktreeFingerprintCancelled.into());
        }
        Ok(())
    }

    fn git_output(self, root: &Path, args: &[&str], label: &str) -> Result<Output> {
        match self {
            Self::Blocking => git_output(root, args, label),
            Self::Cancellable(cancelled) => {
                git_output_with_cancellation(root, args, label, cancelled)
            }
        }
    }

    fn git_hash_object(self, root: &Path, input: &[u8]) -> Result<String> {
        match self {
            Self::Blocking => git_hash_object(root, input),
            Self::Cancellable(cancelled) => {
                git_hash_object_with_cancellation(root, input, cancelled)
            }
        }
    }

    fn git_hash_file(self, root: &Path, full_path: &Path) -> Result<String> {
        match self {
            Self::Blocking => git_hash_file(root, full_path),
            Self::Cancellable(cancelled) => {
                git_hash_file_with_cancellation(root, full_path, cancelled)
            }
        }
    }
}

#[derive(Debug)]
struct WorktreeFingerprintCancelled;

impl std::fmt::Display for WorktreeFingerprintCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("worktree fingerprint collection was cancelled")
    }
}

impl std::error::Error for WorktreeFingerprintCancelled {}

fn repo_worktree_fingerprint_inner(
    root: &Path,
    collection: FingerprintCollection<'_>,
) -> Result<String> {
    collection.ensure_active()?;
    let status = collection.git_output(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git status --porcelain",
    )?;
    collection.ensure_active()?;
    let unstaged = collection.git_output(
        root,
        &["diff", "--binary", "--", ".", ":(exclude).agent/**"],
        "git diff --binary",
    )?;
    collection.ensure_active()?;
    let staged = collection.git_output(
        root,
        &[
            "diff",
            "--cached",
            "--binary",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --cached --binary",
    )?;
    collection.ensure_active()?;
    let untracked = untracked_file_contents(root, &status.stdout, collection)?;

    let mut input = Vec::new();
    input.extend_from_slice(b"status\0");
    input.extend_from_slice(&status.stdout);
    input.extend_from_slice(b"\0unstaged\0");
    input.extend_from_slice(&unstaged.stdout);
    input.extend_from_slice(b"\0staged\0");
    input.extend_from_slice(&staged.stdout);
    input.extend_from_slice(b"\0untracked\0");
    input.extend_from_slice(&untracked);

    collection.ensure_active()?;
    collection.git_hash_object(root, &input)
}

fn untracked_file_contents(
    root: &Path,
    status_stdout: &[u8],
    collection: FingerprintCollection<'_>,
) -> Result<Vec<u8>> {
    let mut contents = Vec::new();
    let mut remaining_inline_bytes = MAX_TOTAL_INLINE_UNTRACKED_BYTES;
    for entry in parse_porcelain_status_z(status_stdout)? {
        collection.ensure_active()?;
        if entry.status != "??" {
            continue;
        }
        let full_path = root.join(&entry.path);
        let metadata = fs::symlink_metadata(&full_path).with_context(|| {
            format!(
                "Failed to read untracked path metadata {}",
                full_path.display()
            )
        })?;

        contents.extend_from_slice(entry.path.as_os_str().as_encoded_bytes());
        contents.push(0);
        append_untracked_path_fingerprint(
            &mut contents,
            root,
            &full_path,
            &metadata,
            &mut remaining_inline_bytes,
            collection,
        )?;
        contents.push(0);
    }
    collection.ensure_active()?;
    Ok(contents)
}

#[derive(Debug, Eq, PartialEq)]
struct PorcelainStatusEntry {
    status: String,
    path: PathBuf,
    original_path: Option<PathBuf>,
}

fn parse_porcelain_status_z(stdout: &[u8]) -> Result<Vec<PorcelainStatusEntry>> {
    let fields = stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let field = fields[index];
        if field.is_empty() {
            if index == fields.len() - 1 {
                break;
            }
            bail!("Malformed git status --porcelain -z output: empty path field");
        }
        if field.len() < 4 || field[2] != b' ' {
            bail!(
                "Malformed git status --porcelain -z record: {}",
                String::from_utf8_lossy(field)
            );
        }
        let status = String::from_utf8_lossy(&field[..2]).to_string();
        let path = path_buf_from_git_bytes(&field[3..])?;
        index += 1;

        let original_path = if status.as_bytes().contains(&b'R')
            || status.as_bytes().contains(&b'C')
        {
            let original = fields.get(index).context(
                "Malformed git status --porcelain -z output: rename/copy record missing original path",
            )?;
            if original.is_empty() {
                bail!("Malformed git status --porcelain -z output: empty original path");
            }
            index += 1;
            Some(path_buf_from_git_bytes(original)?)
        } else {
            None
        };

        entries.push(PorcelainStatusEntry {
            status,
            path,
            original_path,
        });
    }
    Ok(entries)
}

#[cfg(unix)]
fn path_buf_from_git_bytes(bytes: &[u8]) -> Result<PathBuf> {
    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn path_buf_from_git_bytes(bytes: &[u8]) -> Result<PathBuf> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .context("Git status path is not UTF-8")
}

fn append_untracked_path_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    metadata: &fs::Metadata,
    remaining_inline_bytes: &mut u64,
    collection: FingerprintCollection<'_>,
) -> Result<()> {
    collection.ensure_active()?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        contents.extend_from_slice(b"symlink\0");
        let target = fs::read_link(full_path)
            .with_context(|| format!("Failed to read symlink target {}", full_path.display()))?;
        contents.extend_from_slice(target.as_os_str().as_encoded_bytes());
        return Ok(());
    }

    if metadata.is_dir() {
        contents.extend_from_slice(b"dir");
        return Ok(());
    }

    if metadata.is_file() {
        append_untracked_file_fingerprint(
            contents,
            root,
            full_path,
            metadata,
            remaining_inline_bytes,
            collection,
        )?;
        return Ok(());
    }

    contents.extend_from_slice(b"other\0");
    append_metadata_fallback(contents, metadata);
    Ok(())
}

fn append_untracked_file_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    metadata: &fs::Metadata,
    remaining_inline_bytes: &mut u64,
    collection: FingerprintCollection<'_>,
) -> Result<()> {
    collection.ensure_active()?;
    if metadata.len() > MAX_INLINE_UNTRACKED_BYTES || metadata.len() > *remaining_inline_bytes {
        append_hashed_file_fingerprint(contents, root, full_path, collection)?;
        return Ok(());
    }

    let mut file = fs::File::open(full_path)
        .with_context(|| format!("Failed to open untracked file {}", full_path.display()))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_INLINE_UNTRACKED_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Failed to read untracked file {}", full_path.display()))?;
    collection.ensure_active()?;

    if bytes.len() as u64 > MAX_INLINE_UNTRACKED_BYTES {
        append_hashed_file_fingerprint(contents, root, full_path, collection)?;
        return Ok(());
    }

    contents.extend_from_slice(b"file\0");
    contents.extend_from_slice(&bytes);
    *remaining_inline_bytes = remaining_inline_bytes.saturating_sub(bytes.len() as u64);
    Ok(())
}

fn append_hashed_file_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    collection: FingerprintCollection<'_>,
) -> Result<()> {
    contents.extend_from_slice(b"file-hash\0");
    contents.extend_from_slice(collection.git_hash_file(root, full_path)?.as_bytes());
    Ok(())
}

fn append_metadata_fallback(contents: &mut Vec<u8>, metadata: &fs::Metadata) {
    contents.extend_from_slice(format!("len={}\0", metadata.len()).as_bytes());
    contents.extend_from_slice(
        format!("modified={}\0", system_time_key(metadata.modified().ok())).as_bytes(),
    );
}

fn system_time_key(time: Option<SystemTime>) -> u128 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn git_output(root: &Path, args: &[&str], label: &str) -> Result<Output> {
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

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn git_output_with_cancellation(
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

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn git_output_with_cancellation(
    root: &Path,
    args: &[&str],
    label: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output> {
    FingerprintCollection::Cancellable(cancelled).ensure_active()?;
    let output = git_output(root, args, label)?;
    FingerprintCollection::Cancellable(cancelled).ensure_active()?;
    Ok(output)
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn run_git_command_with_cancellation(
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
        Err(OwnedProcessTreeError::Cancelled) => {
            return Err(WorktreeFingerprintCancelled.into());
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

fn git_hash_object(root: &Path, input: &[u8]) -> Result<String> {
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

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn git_hash_object_with_cancellation(
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

fn write_fingerprint_hash_input(
    writer: &mut impl Write,
    input: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    let collection = FingerprintCollection::Cancellable(cancelled);
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

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn git_hash_object_with_cancellation(
    root: &Path,
    input: &[u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    FingerprintCollection::Cancellable(cancelled).ensure_active()?;
    let hash = git_hash_object(root, input)?;
    FingerprintCollection::Cancellable(cancelled).ensure_active()?;
    Ok(hash)
}

fn git_hash_file(root: &Path, full_path: &Path) -> Result<String> {
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

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn git_hash_file_with_cancellation(
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

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn git_hash_file_with_cancellation(
    root: &Path,
    full_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    FingerprintCollection::Cancellable(cancelled).ensure_active()?;
    let hash = git_hash_file(root, full_path)?;
    FingerprintCollection::Cancellable(cancelled).ensure_active()?;
    Ok(hash)
}

fn configure_read_only_git_environment(command: &mut Command) {
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

fn parse_numstat_count(field: &str, line_number: usize, kind: &str) -> Result<u64> {
    if field == "-" {
        return Ok(0);
    }
    field.parse::<u64>().with_context(|| {
        format!("Invalid git diff --numstat {kind} count on line {line_number}: {field}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::ffi::OsStr;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::tempdir;

    #[test]
    fn read_only_git_commands_disable_optional_locks() {
        let mut command = Command::new("git");
        command.env("GIT_OPTIONAL_LOCKS", "1");

        configure_read_only_git_environment(&mut command);

        assert_eq!(
            command
                .get_envs()
                .find(|(name, _)| *name == OsStr::new("GIT_OPTIONAL_LOCKS"))
                .and_then(|(_, value)| value),
            Some(OsStr::new("0"))
        );
    }

    #[test]
    fn fingerprint_hash_staging_checks_cancellation_between_chunks() {
        let input = vec![b'x'; FINGERPRINT_HASH_WRITE_CHUNK * 3];
        let checks = Cell::new(0);
        let mut staged = Vec::new();

        let error = write_fingerprint_hash_input(&mut staged, &input, &|| {
            let current = checks.get();
            checks.set(current + 1);
            current >= 1
        })
        .unwrap_err();

        assert!(is_worktree_fingerprint_cancellation(&error));
        assert_eq!(staged.len(), FINGERPRINT_HASH_WRITE_CHUNK);
    }

    #[test]
    fn parse_diff_stat_output_counts_binary_files_without_swallowing_other_errors() {
        let diff_stat =
            parse_diff_stat_output("12\t3\tsrc/main.rs\n-\t-\tassets/logo.png\n").unwrap();
        assert_eq!(diff_stat.files, 2);
        assert_eq!(diff_stat.insertions, 12);
        assert_eq!(diff_stat.deletions, 3);
    }

    #[test]
    fn parse_diff_stat_output_rejects_invalid_counts() {
        let error = parse_diff_stat_output("oops\t3\tsrc/main.rs\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("Invalid git diff --numstat insertions count"));
    }

    #[test]
    fn collect_git_receipt_metadata_records_git_failures() {
        let temp = tempdir().unwrap();
        let metadata = collect_git_receipt_metadata(temp.path());

        assert!(metadata.changed_paths.is_empty());
        assert_eq!(metadata.diff_stat.files, 0);
        assert!(metadata.git_status_error.is_some());
        assert!(metadata.git_diff_stat_error.is_some());
        assert!(metadata.worktree_fingerprint.is_none());
        assert!(metadata.worktree_fingerprint_error.is_some());
    }

    #[test]
    fn changed_paths_preserve_spaces_and_rename_paths() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("old name.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "old name.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        run_git(temp.path(), &["mv", "old name.txt", "new name.txt"]);
        std::fs::write(temp.path().join("loose note.txt"), "untracked").unwrap();

        let paths = repo_changed_paths(temp.path()).unwrap();

        assert!(paths.contains(&"new name.txt".to_string()));
        assert!(paths.contains(&"old name.txt".to_string()));
        assert!(paths.contains(&"loose note.txt".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn porcelain_z_parser_preserves_non_utf8_path_bytes() {
        let entries = parse_porcelain_status_z(b"?? bad\xFFname\0").unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].path.as_os_str().as_encoded_bytes(),
            b"bad\xFFname"
        );
    }

    #[test]
    fn worktree_fingerprint_changes_when_untracked_file_content_changes() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        std::fs::write(temp.path().join("new.txt"), "one").unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();
        std::fs::write(temp.path().join("new.txt"), "two").unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn worktree_fingerprint_changes_when_large_untracked_file_content_changes() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        let large_path = temp.path().join("large.bin");
        let fixed_mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        std::fs::write(
            &large_path,
            vec![b'a'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::File::open(&large_path)
            .unwrap()
            .set_modified(fixed_mtime)
            .unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();

        std::fs::write(
            &large_path,
            vec![b'b'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::File::open(&large_path)
            .unwrap()
            .set_modified(fixed_mtime)
            .unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn worktree_fingerprint_changes_when_untracked_symlink_target_changes() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        let first_target = temp.path().join("outside-one");
        let second_target = temp.path().join("outside-two");
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&first_target, &link).unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second_target, &link).unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
