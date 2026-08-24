use std::fs;
use std::io::{Read, Write, copy};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use tempfile::NamedTempFile;

use crate::bootstrap::scrub_known_repository_git_environment;

#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use jig_owned_process::{
    ProcessOutputLimits, format_exit_status, require_success, run_checked_output_with_context,
    run_owned_process_tree_with_output_limits,
};

const MAX_INLINE_UNTRACKED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_INLINE_UNTRACKED_BYTES: u64 = 32 * 1024 * 1024;
const FINGERPRINT_HASH_WRITE_CHUNK: usize = 64 * 1024;
const MAX_RECEIPT_CHANGED_PATHS: usize = 100;
const CHANGED_PATHS_DIGEST_DOMAIN: &[u8] = b"jig-changed-paths-v1\0";
/// Git pathspec authority shared by source fingerprints and affected planning.
/// `.agent/**` is runtime and harness metadata; the canonical contract model is
/// represented separately by `RepoContext::contract_digest`.
const SOURCE_IDENTITY_PATHSPECS: &[&str] = &[".", ":(exclude).agent/**"];

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DiffStat {
    pub(crate) files: usize,
    pub(crate) insertions: u64,
    pub(crate) deletions: u64,
}

#[derive(Debug, Default)]
pub(crate) struct GitReceiptMetadata {
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_path_count: Option<usize>,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: Option<String>,
    pub(crate) diff_stat: DiffStat,
    pub(crate) git_status_error: Option<String>,
    pub(crate) git_diff_stat_error: Option<String>,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

pub(crate) fn collect_git_receipt_metadata(root: &Path) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(root, true, GitReceiptCollection::Blocking)
}

pub(crate) fn collect_git_receipt_metadata_without_worktree_fingerprint(
    root: &Path,
) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(root, false, GitReceiptCollection::Blocking)
}

pub(crate) fn collect_git_receipt_metadata_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(
        root,
        true,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

pub(crate) fn collect_git_receipt_metadata_without_worktree_fingerprint_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(
        root,
        false,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

fn collect_git_receipt_metadata_with_options(
    root: &Path,
    collect_worktree_fingerprint: bool,
    collection: GitReceiptCollection<'_>,
) -> GitReceiptMetadata {
    let (
        changed_paths,
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest,
        git_status_error,
    ) = match repo_changed_paths_inner(root, collection) {
        Ok(changed_paths) => {
            let changed_paths = bounded_changed_paths(changed_paths);
            (
                changed_paths.preview,
                Some(changed_paths.total),
                changed_paths.truncated,
                Some(changed_paths.digest),
                None,
            )
        }
        Err(error) => (Vec::new(), None, false, None, Some(format!("{error:#}"))),
    };
    let (diff_stat, git_diff_stat_error) = match repo_diff_stat_inner(root, collection) {
        Ok(diff_stat) => (diff_stat, None),
        Err(error) => (DiffStat::default(), Some(format!("{error:#}"))),
    };
    let (worktree_fingerprint, worktree_fingerprint_error) = if collect_worktree_fingerprint {
        match repo_worktree_fingerprint_inner(root, collection, 0) {
            Ok(snapshot) => (Some(snapshot.worktree_fingerprint), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        }
    } else {
        (None, None)
    };

    GitReceiptMetadata {
        changed_paths,
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest,
        diff_stat,
        git_status_error,
        git_diff_stat_error,
        worktree_fingerprint,
        worktree_fingerprint_error,
    }
}

#[cfg(test)]
fn repo_changed_paths(root: &Path) -> Result<Vec<String>> {
    repo_changed_paths_inner(root, GitReceiptCollection::Blocking)
}

fn repo_changed_paths_inner(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<String>> {
    collection.ensure_active()?;
    let output = collection.git_output(
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

fn repo_affected_worktree_changed_path_buffers(root: &Path) -> Result<Vec<PathBuf>> {
    let mut args = vec![
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--",
    ];
    args.extend_from_slice(SOURCE_IDENTITY_PATHSPECS);
    let output = git_output(root, &args, "git status --porcelain -z")?;
    parse_porcelain_status_z(&output.stdout).map(|entries| {
        entries
            .into_iter()
            .flat_map(|entry| {
                let mut paths = vec![entry.path];
                if let Some(original_path) = entry.original_path {
                    paths.push(original_path);
                }
                paths
            })
            .collect()
    })
}

fn affected_ignored_dotenv_paths(root: &Path, submodule_depth: usize) -> Result<Vec<PathBuf>> {
    if submodule_depth > crate::source_projection::MAX_SUBMODULE_DEPTH {
        bail!("affected dotenv inputs exceed the supported submodule nesting depth");
    }
    let collection = GitReceiptCollection::Blocking;
    let mut paths = ignored_dotenv_paths(root, collection)?;
    for submodule in initialized_submodule_paths(root, collection)? {
        for nested in affected_ignored_dotenv_paths(&root.join(&submodule), submodule_depth + 1)? {
            paths.push(submodule.join(nested));
        }
    }
    Ok(paths)
}

/// Returns presence-only observations for ignored dotenv files. Git has no
/// committed value to diff for these paths, so they are not changed paths.
/// The affected planner uses them only when an action explicitly declares the
/// path as an input; they never become unclaimed changes or component-root
/// fallbacks.
pub(crate) fn repo_observed_ignored_dotenv_paths(root: &Path) -> Result<Vec<String>> {
    let mut paths = affected_ignored_dotenv_paths(root, 0)?
        .into_iter()
        .map(strict_git_path)
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Returns the deterministic union of paths changed from the merge base of an
/// explicit Git revision to `HEAD` and paths currently changed in the
/// worktree. Runtime-owned agent state and cache data are excluded because
/// planning and execution mutate them. Contract authority is tracked by the
/// separately canonicalized repository configuration digest.
pub(crate) fn repo_changed_paths_since(root: &Path, base: &str) -> Result<Vec<String>> {
    let base_commit = resolve_commit(root, base, "affected Git base")?;
    let head_commit = resolve_commit(root, "HEAD", "Git HEAD for affected selection")?;
    let range = format!("{base_commit}...{head_commit}");
    let mut args = vec!["diff", "--name-only", "-z", "--no-renames", &range, "--"];
    args.extend_from_slice(SOURCE_IDENTITY_PATHSPECS);
    let committed = git_output(root, &args, "git diff for affected selection")?;

    let mut paths = parse_name_only_z(&committed.stdout)?;
    paths.extend(repo_affected_worktree_changed_path_buffers(root)?);
    let mut normalized = paths
        .into_iter()
        .map(strict_git_path)
        .collect::<Result<Vec<_>>>()?;
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn resolve_commit(root: &Path, revision: &str, label: &str) -> Result<String> {
    let peeled = revision.strip_suffix("^{commit}").unwrap_or(revision);
    let resolved_revision = format!("{peeled}^{{commit}}");
    let output = git_output(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &resolved_revision,
        ],
        label,
    )
    .with_context(|| format!("Invalid {label} {revision:?}"))?;
    let object_id = String::from_utf8(output.stdout)
        .with_context(|| format!("{label} resolved to a non-UTF-8 object id"))?;
    let object_id = object_id.trim();
    if !matches!(object_id.len(), 40 | 64)
        || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("{label} resolved to invalid object id {object_id:?}");
    }
    Ok(object_id.to_owned())
}

fn parse_name_only_z(stdout: &[u8]) -> Result<Vec<PathBuf>> {
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    if !stdout.ends_with(&[0]) {
        bail!("Malformed git diff --name-only -z output: missing terminator");
    }
    stdout[..stdout.len() - 1]
        .split(|byte| *byte == 0)
        .map(|path| {
            if path.is_empty() {
                bail!("Malformed git diff --name-only -z output: empty path");
            }
            #[cfg(unix)]
            {
                Ok(path_buf_from_git_bytes(path))
            }
            #[cfg(not(unix))]
            {
                path_buf_from_git_bytes(path)
            }
        })
        .collect()
}

fn strict_git_path(path: PathBuf) -> Result<String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let path = std::str::from_utf8(bytes)
        .context("Affected selection requires UTF-8 Git paths")?
        .to_owned();
    Ok(path)
}

fn repo_diff_stat_inner(root: &Path, collection: GitReceiptCollection<'_>) -> Result<DiffStat> {
    collection.ensure_active()?;
    let output = collection.git_output(
        root,
        &["diff", "--numstat", "--", ".", ":(exclude).agent/**"],
        "git diff --numstat",
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diff_stat_output(&stdout)
}

#[derive(Debug, Eq, PartialEq)]
struct BoundedChangedPaths {
    preview: Vec<String>,
    total: usize,
    truncated: bool,
    digest: String,
}

fn bounded_changed_paths(mut paths: Vec<String>) -> BoundedChangedPaths {
    paths.sort();
    paths.dedup();

    let total = paths.len();
    let digest = changed_paths_digest(&paths);
    paths.truncate(MAX_RECEIPT_CHANGED_PATHS);

    BoundedChangedPaths {
        truncated: total > paths.len(),
        preview: paths,
        total,
        digest,
    }
}

fn changed_paths_digest(paths: &[String]) -> String {
    let mut digest = Sha256::new();
    digest.update(CHANGED_PATHS_DIGEST_DOMAIN);
    digest.update((paths.len() as u64).to_be_bytes());
    for path in paths {
        let bytes = path.as_bytes();
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    format!("sha256:{:x}", digest.finalize())
}

pub(crate) struct RepositorySourceSnapshot {
    pub(crate) head_commit: Option<String>,
    pub(crate) worktree_fingerprint: String,
}

pub(crate) fn repository_source_snapshot(root: &Path) -> Result<RepositorySourceSnapshot> {
    repo_worktree_fingerprint_inner(root, GitReceiptCollection::Blocking, 0)
}

pub(crate) fn repo_worktree_fingerprint(root: &Path) -> Result<String> {
    Ok(repository_source_snapshot(root)?.worktree_fingerprint)
}

pub(crate) fn repo_worktree_fingerprint_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    Ok(
        repo_worktree_fingerprint_inner(root, GitReceiptCollection::Cancellable(cancelled), 0)?
            .worktree_fingerprint,
    )
}

pub(crate) fn is_git_receipt_collection_cancellation(error: &anyhow::Error) -> bool {
    error.is::<GitReceiptCollectionCancelled>()
}

#[derive(Clone, Copy)]
enum GitReceiptCollection<'a> {
    Blocking,
    Cancellable(&'a dyn Fn() -> bool),
}

impl GitReceiptCollection<'_> {
    fn ensure_active(self) -> Result<()> {
        if matches!(self, Self::Cancellable(cancelled) if cancelled()) {
            return Err(GitReceiptCollectionCancelled.into());
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

    fn git_output_unchecked(self, root: &Path, args: &[&str], label: &str) -> Result<Output> {
        match self {
            Self::Blocking => git_output_unchecked(root, args, label),
            Self::Cancellable(cancelled) => {
                git_output_unchecked_with_cancellation(root, args, label, cancelled)
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
struct GitReceiptCollectionCancelled;

impl std::fmt::Display for GitReceiptCollectionCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Git receipt metadata collection was cancelled")
    }
}

impl std::error::Error for GitReceiptCollectionCancelled {}

fn repo_worktree_fingerprint_inner(
    root: &Path,
    collection: GitReceiptCollection<'_>,
    submodule_depth: usize,
) -> Result<RepositorySourceSnapshot> {
    if submodule_depth > crate::source_projection::MAX_SUBMODULE_DEPTH {
        bail!("worktree fingerprint submodules exceed the supported nesting depth");
    }
    collection.ensure_active()?;
    let committed_source = committed_source_tree_identity(root, collection)?;
    collection.ensure_active()?;
    let mut status_args = vec![
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--",
    ];
    status_args.extend_from_slice(SOURCE_IDENTITY_PATHSPECS);
    let status = collection.git_output(root, &status_args, "git status --porcelain")?;
    collection.ensure_active()?;
    let mut unstaged_args = vec!["diff", "--binary", "--"];
    unstaged_args.extend_from_slice(SOURCE_IDENTITY_PATHSPECS);
    let unstaged = collection.git_output(root, &unstaged_args, "git diff --binary")?;
    collection.ensure_active()?;
    let mut staged_args = vec!["diff", "--cached", "--binary", "--"];
    staged_args.extend_from_slice(SOURCE_IDENTITY_PATHSPECS);
    let staged = collection.git_output(root, &staged_args, "git diff --cached --binary")?;
    collection.ensure_active()?;
    let mut remaining_inline_bytes = MAX_TOTAL_INLINE_UNTRACKED_BYTES;
    let untracked = untracked_file_contents(
        root,
        &status.stdout,
        &mut remaining_inline_bytes,
        collection,
    )?;
    collection.ensure_active()?;
    let ignored_dotenv = ignored_dotenv_contents(root, &mut remaining_inline_bytes, collection)?;
    collection.ensure_active()?;
    let submodules = initialized_submodule_fingerprints(root, collection, submodule_depth)?;

    let mut input = Vec::new();
    input.extend_from_slice(b"committed-source-tree\0");
    input.extend_from_slice(&committed_source.tree_identity);
    input.extend_from_slice(b"\0status\0");
    input.extend_from_slice(&status.stdout);
    input.extend_from_slice(b"\0unstaged\0");
    input.extend_from_slice(&unstaged.stdout);
    input.extend_from_slice(b"\0staged\0");
    input.extend_from_slice(&staged.stdout);
    input.extend_from_slice(b"\0untracked\0");
    input.extend_from_slice(&untracked);
    input.extend_from_slice(b"\0ignored-dotenv\0");
    input.extend_from_slice(&ignored_dotenv);
    input.extend_from_slice(b"\0initialized-submodules\0");
    input.extend_from_slice(&submodules);

    collection.ensure_active()?;
    Ok(RepositorySourceSnapshot {
        head_commit: committed_source.head_commit,
        worktree_fingerprint: collection.git_hash_object(root, &input)?,
    })
}

struct CommittedSourceIdentity {
    head_commit: Option<String>,
    tree_identity: Vec<u8>,
}

fn committed_source_tree_identity(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<CommittedSourceIdentity> {
    let head = collection.git_output_unchecked(
        root,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        "git rev-parse HEAD for worktree fingerprint",
    )?;
    match head.status.code() {
        Some(0) => {
            let object_id =
                String::from_utf8(head.stdout).context("Git HEAD object id is not UTF-8")?;
            let object_id = object_id.trim();
            if !matches!(object_id.len(), 40 | 64)
                || !object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                bail!("Git HEAD resolved to invalid object id {object_id:?}");
            }
            collection.ensure_active()?;
            let tree = collection.git_output(
                root,
                &["ls-tree", "-z", "--full-tree", object_id],
                "git ls-tree HEAD for worktree fingerprint",
            )?;
            Ok(CommittedSourceIdentity {
                head_commit: Some(object_id.to_owned()),
                tree_identity: committed_source_tree_without_agent_state(&tree.stdout, collection)?,
            })
        }
        Some(1) => Ok(CommittedSourceIdentity {
            head_commit: None,
            tree_identity: b"unborn".to_vec(),
        }),
        _ => {
            bail!(
                "git rev-parse HEAD for worktree fingerprint failed with {}.\nstdout:\n{}\nstderr:\n{}",
                format_exit_status(&head.status),
                String::from_utf8_lossy(&head.stdout),
                String::from_utf8_lossy(&head.stderr)
            )
        }
    }
}

fn committed_source_tree_without_agent_state(
    tree: &[u8],
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    let mut source_tree = Vec::with_capacity(tree.len());
    for record in tree
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        collection.ensure_active()?;
        let path_offset = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("Git ls-tree record is missing its path separator")?
            + 1;
        let path = &record[path_offset..];
        if path == b".agent" || path.starts_with(b".agent/") {
            continue;
        }
        source_tree.extend_from_slice(record);
        source_tree.push(0);
    }
    Ok(source_tree)
}

fn untracked_file_contents(
    root: &Path,
    status_stdout: &[u8],
    remaining_inline_bytes: &mut u64,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    let paths = parse_porcelain_status_z(status_stdout)?
        .into_iter()
        .filter(|entry| entry.status == "??")
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    path_fingerprint_contents(root, paths, remaining_inline_bytes, collection)
}

fn ignored_dotenv_contents(
    root: &Path,
    remaining_inline_bytes: &mut u64,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    path_fingerprint_contents(
        root,
        ignored_dotenv_paths(root, collection)?,
        remaining_inline_bytes,
        collection,
    )
}

fn ignored_dotenv_paths(root: &Path, collection: GitReceiptCollection<'_>) -> Result<Vec<PathBuf>> {
    let mut args = vec![
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--directory",
        "-z",
        "--",
    ];
    args.extend_from_slice(crate::source_projection::IGNORED_DOTENV_PATHSPECS);
    let output = collection.git_output(root, &args, "git ls-files for ignored dotenv files")?;
    // `--directory` lets Git prune a wholly ignored tree instead of walking
    // build products and dependency caches looking for dotenv-shaped files.
    // Directory records end in `/`; only actual files are source inputs.
    parse_name_only_z(&output.stdout).map(|paths| {
        paths
            .into_iter()
            .filter(|path| !path.as_os_str().as_encoded_bytes().ends_with(b"/"))
            .collect()
    })
}

fn path_fingerprint_contents(
    root: &Path,
    paths: Vec<PathBuf>,
    remaining_inline_bytes: &mut u64,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    let mut contents = Vec::new();
    for path in paths {
        collection.ensure_active()?;
        let full_path = root.join(&path);
        let metadata = fs::symlink_metadata(&full_path).with_context(|| {
            format!(
                "Failed to read observable path metadata {}",
                full_path.display()
            )
        })?;

        contents.extend_from_slice(path.as_os_str().as_encoded_bytes());
        contents.push(0);
        append_untracked_path_fingerprint(
            &mut contents,
            root,
            &full_path,
            &metadata,
            remaining_inline_bytes,
            collection,
        )?;
        contents.push(0);
    }
    collection.ensure_active()?;
    Ok(contents)
}

fn initialized_submodule_fingerprints(
    root: &Path,
    collection: GitReceiptCollection<'_>,
    submodule_depth: usize,
) -> Result<Vec<u8>> {
    let paths = initialized_submodule_paths(root, collection)?;

    let mut fingerprints = Vec::new();
    for relative in paths {
        collection.ensure_active()?;
        let fingerprint = repo_worktree_fingerprint_inner(
            &root.join(&relative),
            collection,
            submodule_depth + 1,
        )?;
        fingerprints.extend_from_slice(relative.as_os_str().as_encoded_bytes());
        fingerprints.push(0);
        fingerprints.extend_from_slice(fingerprint.worktree_fingerprint.as_bytes());
        fingerprints.push(0);
    }
    Ok(fingerprints)
}

fn initialized_submodule_paths(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<PathBuf>> {
    if !root.join(".gitmodules").is_file() {
        return Ok(Vec::new());
    }
    let output = collection.git_output_unchecked(
        root,
        &[
            "config",
            "-z",
            "--file",
            ".gitmodules",
            "--get-regexp",
            "^submodule\\..*\\.path$",
        ],
        "git config for worktree fingerprint submodules",
    )?;
    let mut paths = match output.status.code() {
        Some(0) => crate::source_projection::initialized_submodule_paths(root, &output.stdout)?,
        Some(1) => return Ok(Vec::new()),
        _ => {
            bail!(
                "git config for worktree fingerprint submodules failed with {}.\nstdout:\n{}\nstderr:\n{}",
                format_exit_status(&output.status),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }
    };
    paths.sort();
    Ok(paths)
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
        #[cfg(unix)]
        let path = path_buf_from_git_bytes(&field[3..]);
        #[cfg(not(unix))]
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
            #[cfg(unix)]
            {
                Some(path_buf_from_git_bytes(original))
            }
            #[cfg(not(unix))]
            {
                Some(path_buf_from_git_bytes(original)?)
            }
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
fn path_buf_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
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
    collection: GitReceiptCollection<'_>,
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
    collection: GitReceiptCollection<'_>,
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
    collection: GitReceiptCollection<'_>,
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

mod git_command;
use git_command::*;

#[cfg(test)]
mod tests;
