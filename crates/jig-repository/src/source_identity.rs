#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, copy};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
#[cfg(test)]
use jig_contract::StrictInventoryReasonV1;
use jig_contract::{
    ComparisonRequestV1, CurrentViewV1, ExactTreeProvenanceV1, ResolvedComparisonV1,
};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use jig_git::scrub_known_repository_git_environment;

#[cfg(unix)]
use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};

use jig_owned_process::{
    OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError, ProcessOutputLimits,
    ProcessOutputOverflowPolicy, format_exit_status, require_success,
    run_checked_output_with_context, run_owned_process_tree_with_output_limits,
    run_owned_process_tree_with_output_policy_and_observer,
};

mod change_scope;
mod comparison;
mod content;
mod exact_path;
mod gitlinks;
mod metadata;
mod process;
mod scope;
mod worktree;

pub use change_scope::*;
pub use comparison::*;
pub use content::*;
pub use exact_path::*;
use gitlinks::*;
use process::*;
use scope::*;
use worktree::*;

const MAX_INLINE_UNTRACKED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_INLINE_UNTRACKED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CHANGED_PATH_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CHANGED_PATH_DISCOVERY_ENTRIES: usize = 250_000;
const MAX_WORKTREE_STATUS_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORKTREE_DIFF_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_GATE_SCOPE_DIFF_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_WORKTREE_STATUS_ENTRIES: usize = 250_000;
const MAX_GIT_LITERAL_PATHS_PER_DIFF: usize = 512;
const MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF: usize = 64 * 1024;
const WORKTREE_FINGERPRINT_DOMAIN: &[u8] = b"jig-worktree-fingerprint-v4\0";
const MAX_GIT_ERROR_PREVIEW_BYTES: u64 = 64 * 1024;

#[cfg(test)]
thread_local! {
    static GATE_SCOPE_INPUT_COLLECTION_COUNT: Cell<usize> = const { Cell::new(0) };
    static PLAN_CHANGE_COLLECTION_COUNT: Cell<usize> = const { Cell::new(0) };
    static CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
    static WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
    static GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
    static WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
    static WORKTREE_FINGERPRINT_COLLECTION_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
pub struct PlanChangeSnapshot {
    changed_paths: Vec<String>,
    untracked_paths: Vec<String>,
}

pub fn plan_change_snapshot_with_cancellation(
    root: &Path,
    baseline_oid: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanChangeSnapshot> {
    plan_change_snapshot_inner(root, baseline_oid, GitCollection::Cancellable(cancelled))
}

pub fn plan_change_snapshot_from_empty_tree_with_cancellation(
    root: &Path,
    expected_oid: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanChangeSnapshot> {
    plan_change_snapshot_from_empty_tree_inner(
        root,
        expected_oid,
        GitCollection::Cancellable(cancelled),
    )
}

pub fn resolve_git_commit(root: &Path, reference: &str) -> Result<String> {
    resolve_git_commit_inner(root, reference, GitCollection::Blocking)
}

pub fn resolve_empty_tree_for_unborn_repository(root: &Path) -> Result<Option<String>> {
    resolve_empty_tree_for_unborn_repository_inner(root, GitCollection::Blocking)
}

/// Makes one narrow attempt to obtain an exact push-before object without
/// updating refs, tags, or FETCH_HEAD. Resolution and authentication remain a
/// separate step after this object transfer.
pub fn fetch_exact_push_before_object_v1(root: &Path, oid: &str) -> Result<()> {
    GitCollection::Blocking
        .git_bounded_output_with_timeout(
            root,
            &[
                "--no-replace-objects",
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                "--depth=1",
                "origin",
                oid,
            ],
            "git fetch exact push-before object",
            MAX_GIT_ERROR_PREVIEW_BYTES as usize,
            "push-before-fetch",
            Duration::from_secs(60),
        )
        .map(|_| ())
}

fn resolve_empty_tree_for_unborn_repository_inner(
    root: &Path,
    collection: GitCollection<'_>,
) -> Result<Option<String>> {
    if !has_unborn_symbolic_head(root, collection)? {
        return Ok(None);
    }
    Ok(Some(resolve_empty_tree_oid_inner(root, collection)?))
}

fn resolve_empty_tree_oid_inner(root: &Path, collection: GitCollection<'_>) -> Result<String> {
    let output = collection.git_bounded_output(
        root,
        &[
            "--no-replace-objects",
            "hash-object",
            "-t",
            "tree",
            "--stdin",
        ],
        "git hash-object empty baseline",
        MAX_GIT_ERROR_PREVIEW_BYTES as usize,
        "empty-tree",
    )?;
    parse_git_object_oid(&output.stdout, "empty tree")
}

fn parse_git_object_oid(stdout: &[u8], label: &str) -> Result<String> {
    let oid = std::str::from_utf8(stdout)
        .with_context(|| format!("Git {label} object id was not UTF-8"))?
        .trim();
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Git returned an invalid {label} object id");
    }
    Ok(oid.to_ascii_lowercase())
}

fn resolve_git_commit_inner(
    root: &Path,
    reference: &str,
    collection: GitCollection<'_>,
) -> Result<String> {
    let reference = reference.trim();
    if reference.is_empty() || reference.starts_with('-') || reference.contains(['\0', '\n', '\r'])
    {
        bail!("Unsupported Git baseline ref '{reference}'");
    }
    let commit_ref = format!("{reference}^{{commit}}");
    let output = collection.git_output(
        root,
        &["rev-parse", "--verify", "--end-of-options", &commit_ref],
        "git rev-parse baseline",
    )?;
    parse_git_object_oid(&output.stdout, "baseline")
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
    std::str::from_utf8(path.as_os_str().as_encoded_bytes())
        .context("Affected selection requires UTF-8 Git paths")
        .map(str::to_owned)
}

/// Returns the deterministic union of paths changed from the merge base of an
/// explicit Git revision to `HEAD` and paths currently changed in the worktree.
pub fn repo_changed_paths_since(root: &Path, base: &str) -> Result<Vec<String>> {
    let comparison = resolve_comparison_v1(
        root,
        ComparisonRequestV1::MergeBaseRef {
            requested_ref: base.to_owned(),
        },
    )?;
    capture_affected_paths_v1(root, &comparison)
}

fn ignored_dotenv_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let output = GitCollection::Blocking.git_changed_path_stdout(
        root,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
            "--",
            ":(glob)**/.env",
            ":(glob)**/.env.*",
        ],
        "git ls-files for ignored dotenv files",
    )?;
    parse_name_only_z(&output).map(|paths| {
        paths
            .into_iter()
            .filter(|path| !path.as_os_str().as_encoded_bytes().ends_with(b"/"))
            .collect()
    })
}

fn initialized_submodule_paths_for_affected(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.join(".gitmodules").is_file() {
        return Ok(Vec::new());
    }
    let output = git_output(
        root,
        &[
            "config",
            "-z",
            "--file",
            ".gitmodules",
            "--get-regexp",
            "^submodule\\..*\\.path$",
        ],
        "git config for affected submodules",
    )?;
    let mut paths = crate::source_projection::initialized_submodule_paths(root, &output.stdout)?;
    paths.sort();
    Ok(paths)
}

fn affected_ignored_dotenv_paths(root: &Path, submodule_depth: usize) -> Result<Vec<PathBuf>> {
    if submodule_depth > crate::source_projection::MAX_SUBMODULE_DEPTH {
        bail!("affected dotenv inputs exceed the supported submodule nesting depth");
    }
    let mut paths = ignored_dotenv_paths(root)?;
    for submodule in initialized_submodule_paths_for_affected(root)? {
        for nested in affected_ignored_dotenv_paths(&root.join(&submodule), submodule_depth + 1)? {
            paths.push(submodule.join(nested));
        }
    }
    Ok(paths)
}

pub fn repo_observed_ignored_dotenv_paths(root: &Path) -> Result<Vec<String>> {
    let mut paths = affected_ignored_dotenv_paths(root, 0)?
        .into_iter()
        .map(strict_git_path)
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub struct RepositorySourceSnapshot {
    pub head_commit: Option<String>,
    pub worktree_fingerprint: String,
}

pub fn repository_source_snapshot(root: &Path) -> Result<RepositorySourceSnapshot> {
    repository_source_snapshot_inner(root, GitCollection::Blocking)
}

pub fn repository_source_snapshot_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<RepositorySourceSnapshot> {
    repository_source_snapshot_inner(root, GitCollection::Cancellable(cancelled))
}

fn repository_source_snapshot_inner(
    root: &Path,
    collection: GitCollection<'_>,
) -> Result<RepositorySourceSnapshot> {
    collection.ensure_active()?;
    let head_commit = match resolve_git_commit_inner(root, "HEAD", collection) {
        Ok(commit) => Some(commit),
        Err(error) => match resolve_empty_tree_for_unborn_repository_inner(root, collection)? {
            Some(_) => None,
            None => return Err(error),
        },
    };
    collection.ensure_active()?;
    let committed_tree = if let Some(head_commit) = head_commit.as_deref() {
        let tree = git_worktree_proof_stdout(
            root,
            &["ls-tree", "-z", "--full-tree", head_commit],
            "git ls-tree HEAD for repository source identity",
            worktree_diff_output_limit(),
            collection,
        )?;
        metadata::committed_source_tree_without_agent_state(
            &tree,
            &metadata::tracker_state_paths(root)?,
            collection,
        )?
    } else {
        b"unborn".to_vec()
    };
    collection.ensure_active()?;
    let base = repo_worktree_fingerprint_inner(root, collection)?;
    let mut digest = Sha256::new();
    digest.update(b"jig-repository-source-v6\0");
    hash_field(&mut digest, &committed_tree);
    digest.update(base.as_bytes());
    for path in affected_ignored_dotenv_paths(root, 0)? {
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update([0]);
        let bytes = fs::read(root.join(&path)).with_context(|| {
            format!(
                "Failed to read ignored dotenv source input {}",
                root.join(&path).display()
            )
        })?;
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(RepositorySourceSnapshot {
        head_commit,
        worktree_fingerprint: format!("sha256:{:x}", digest.finalize()),
    })
}

pub use worktree::is_git_collection_cancellation;
#[cfg(test)]
pub use worktree::{repo_worktree_fingerprint, repo_worktree_fingerprint_with_cancellation};

#[derive(Clone, Copy)]
enum GitCollection<'a> {
    Blocking,
    Cancellable(&'a dyn Fn() -> bool),
}

impl GitCollection<'_> {
    fn ensure_active(self) -> Result<()> {
        if matches!(self, Self::Cancellable(cancelled) if cancelled()) {
            return Err(GitCollectionCancelled.into());
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

    fn git_changed_path_stdout(self, root: &Path, args: &[&str], label: &str) -> Result<Vec<u8>> {
        git_changed_path_stdout(root, args, label, self)
    }

    fn git_bounded_output(
        self,
        root: &Path,
        args: &[&str],
        label: &str,
        limit: usize,
        proof_kind: &str,
    ) -> Result<Output> {
        git_bounded_proof_output(root, args, label, limit, proof_kind, self)
    }

    fn git_bounded_output_with_timeout(
        self,
        root: &Path,
        args: &[&str],
        label: &str,
        limit: usize,
        proof_kind: &str,
        timeout: Duration,
    ) -> Result<Output> {
        git_bounded_proof_output_with_timeout(root, args, label, limit, proof_kind, self, timeout)
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
struct GitCollectionCancelled;

impl std::fmt::Display for GitCollectionCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Git source collection was cancelled")
    }
}

impl std::error::Error for GitCollectionCancelled {}

#[cfg(test)]
mod tests;
