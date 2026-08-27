#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, copy};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::bootstrap::scrub_known_repository_git_environment;

#[cfg(unix)]
use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};

use jig_owned_process::{
    OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError, ProcessOutputLimits,
    ProcessOutputOverflowPolicy, format_exit_status, require_success,
    run_checked_output_with_context, run_owned_process_tree_with_output_limits,
    run_owned_process_tree_with_output_policy_and_observer,
};

mod process;
mod scope;
mod worktree;

use process::*;
use scope::*;
use worktree::*;

const MAX_INLINE_UNTRACKED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_INLINE_UNTRACKED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RECEIPT_CHANGED_PATHS: usize = 100;
const MAX_CHANGED_PATH_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CHANGED_PATH_DISCOVERY_ENTRIES: usize = 250_000;
const MAX_WORKTREE_STATUS_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORKTREE_DIFF_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_GATE_SCOPE_DIFF_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_WORKTREE_STATUS_ENTRIES: usize = 250_000;
const MAX_GIT_LITERAL_PATHS_PER_DIFF: usize = 512;
const MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF: usize = 64 * 1024;
const CHANGED_PATHS_DIGEST_DOMAIN: &[u8] = b"jig-changed-paths-v1\0";
const GATE_SCOPE_FINGERPRINT_DOMAIN: &[u8] = b"jig-gate-scope-v1\0";
const GATE_SCOPE_INPUT_FINGERPRINT_DOMAIN: &[u8] = b"jig-gate-scope-input-v2\0";
const WORKTREE_FINGERPRINT_DOMAIN: &[u8] = b"jig-worktree-fingerprint-v4\0";
const MAX_GIT_ERROR_PREVIEW_BYTES: u64 = 64 * 1024;
const GLOBAL_GATE_AUTHORITY_PATHS: &[&str] = &[".jig.toml", ".agent/jig-contract.json"];

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

#[cfg(test)]
pub(crate) fn reset_worktree_fingerprint_collection_count() {
    WORKTREE_FINGERPRINT_COLLECTION_COUNT.set(0);
}

#[cfg(test)]
pub(crate) fn worktree_fingerprint_collection_count() -> usize {
    WORKTREE_FINGERPRINT_COLLECTION_COUNT.get()
}

#[cfg(test)]
pub(crate) fn reset_gate_scope_collection_counts() {
    PLAN_CHANGE_COLLECTION_COUNT.set(0);
    GATE_SCOPE_INPUT_COLLECTION_COUNT.set(0);
}

#[cfg(test)]
pub(crate) fn plan_change_collection_count() -> usize {
    PLAN_CHANGE_COLLECTION_COUNT.get()
}

#[cfg(test)]
pub(crate) fn gate_scope_input_collection_count() -> usize {
    GATE_SCOPE_INPUT_COLLECTION_COUNT.get()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateApplicability {
    Applicable,
    NotApplicable,
}

impl GateApplicability {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Applicable => "applicable",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GateScopeFacts {
    pub(crate) baseline_oid: String,
    pub(crate) applicability: GateApplicability,
    pub(crate) reason: String,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_path_count: usize,
    pub(crate) changed_paths_truncated: bool,
    pub(crate) changed_paths_digest: String,
    pub(crate) matching_paths: Vec<String>,
    pub(crate) matching_path_count: usize,
    pub(crate) matching_paths_truncated: bool,
    pub(crate) matching_paths_digest: String,
}

#[derive(Clone, Debug)]
pub(crate) struct GateScopeSnapshot {
    pub(crate) facts: GateScopeFacts,
    pub(crate) scope_fingerprint: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct GateScopePolicyKey {
    paths: Option<Vec<String>>,
    paths_ignore: Vec<String>,
}

#[derive(Clone, Debug)]
struct GateScopeInputSnapshot {
    facts: GateScopeFacts,
    input_fingerprint: String,
}

impl GateScopeInputSnapshot {
    fn for_gate_signature(self, gate_signature: &str) -> GateScopeSnapshot {
        let scope_fingerprint = gate_scope_fingerprint(
            &self.facts.baseline_oid,
            gate_signature,
            &self.input_fingerprint,
        );
        GateScopeSnapshot {
            facts: self.facts,
            scope_fingerprint,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PlanChangeSnapshot {
    baseline_oid: String,
    changed_paths: Vec<String>,
    untracked_paths: Vec<String>,
    scope_cache:
        RefCell<BTreeMap<GateScopePolicyKey, std::result::Result<GateScopeInputSnapshot, String>>>,
}

#[cfg(test)]
pub(crate) fn gate_scope_snapshot(
    root: &Path,
    baseline_oid: &str,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    gate_signature: &str,
) -> Result<GateScopeSnapshot> {
    let plan = plan_change_snapshot(root, baseline_oid)?;
    gate_scope_snapshot_from_plan_change_inner(
        root,
        &plan,
        paths,
        paths_ignore,
        gate_signature,
        GitReceiptCollection::Blocking,
    )
}

#[cfg(test)]
pub(crate) fn gate_scope_snapshot_with_cancellation(
    root: &Path,
    baseline_oid: &str,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    gate_signature: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<GateScopeSnapshot> {
    let collection = GitReceiptCollection::Cancellable(cancelled);
    let plan = plan_change_snapshot_inner(root, baseline_oid, collection)?;
    gate_scope_snapshot_from_plan_change_inner(
        root,
        &plan,
        paths,
        paths_ignore,
        gate_signature,
        collection,
    )
}

pub(crate) fn plan_change_snapshot(root: &Path, baseline_oid: &str) -> Result<PlanChangeSnapshot> {
    plan_change_snapshot_inner(root, baseline_oid, GitReceiptCollection::Blocking)
}

pub(crate) fn plan_change_snapshot_with_cancellation(
    root: &Path,
    baseline_oid: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanChangeSnapshot> {
    plan_change_snapshot_inner(
        root,
        baseline_oid,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

pub(crate) fn plan_change_snapshot_from_empty_tree(
    root: &Path,
    expected_oid: &str,
) -> Result<PlanChangeSnapshot> {
    plan_change_snapshot_from_empty_tree_inner(root, expected_oid, GitReceiptCollection::Blocking)
}

pub(crate) fn plan_change_snapshot_from_empty_tree_with_cancellation(
    root: &Path,
    expected_oid: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<PlanChangeSnapshot> {
    plan_change_snapshot_from_empty_tree_inner(
        root,
        expected_oid,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

pub(crate) fn gate_scope_snapshot_from_plan_change(
    root: &Path,
    plan: &PlanChangeSnapshot,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    gate_signature: &str,
) -> Result<GateScopeSnapshot> {
    gate_scope_snapshot_from_plan_change_inner(
        root,
        plan,
        paths,
        paths_ignore,
        gate_signature,
        GitReceiptCollection::Blocking,
    )
}

pub(crate) fn gate_scope_snapshot_from_plan_change_with_cancellation(
    root: &Path,
    plan: &PlanChangeSnapshot,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    gate_signature: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<GateScopeSnapshot> {
    gate_scope_snapshot_from_plan_change_inner(
        root,
        plan,
        paths,
        paths_ignore,
        gate_signature,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

pub(crate) fn resolve_git_commit(root: &Path, reference: &str) -> Result<String> {
    resolve_git_commit_inner(root, reference, GitReceiptCollection::Blocking)
}

pub(crate) fn resolve_empty_tree_for_unborn_repository(root: &Path) -> Result<Option<String>> {
    if git_output(
        root,
        &["symbolic-ref", "-q", "HEAD"],
        "git symbolic-ref HEAD",
    )
    .is_err()
    {
        return Ok(None);
    }
    Ok(Some(resolve_empty_tree_oid_inner(
        root,
        GitReceiptCollection::Blocking,
    )?))
}

fn resolve_empty_tree_oid_inner(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    let output = collection.git_output(root, &["mktree"], "git mktree empty baseline")?;
    parse_git_object_oid(&output.stdout, "empty tree")
}

fn parse_git_object_oid(stdout: &[u8], label: &str) -> Result<String> {
    let oid = std::str::from_utf8(stdout)
        .with_context(|| format!("Git {label} object id was not UTF-8"))?
        .trim();
    if oid.is_empty() || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Git returned an invalid {label} object id");
    }
    Ok(oid.to_ascii_lowercase())
}

fn resolve_git_commit_inner(
    root: &Path,
    reference: &str,
    collection: GitReceiptCollection<'_>,
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
        match repo_worktree_fingerprint_inner(root, collection) {
            Ok(fingerprint) => (Some(fingerprint), None),
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
    let output = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "diff.ignoreSubmodules=none",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git status --porcelain -z",
    )?;
    parse_porcelain_status_z(&output).map(|entries| {
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

fn repo_diff_stat_inner(root: &Path, collection: GitReceiptCollection<'_>) -> Result<DiffStat> {
    collection.ensure_active()?;
    let output = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--numstat",
            "--ignore-submodules=none",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --numstat",
    )?;
    let stdout = String::from_utf8_lossy(&output);
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

pub(crate) fn repo_worktree_fingerprint(root: &Path) -> Result<String> {
    repo_worktree_fingerprint_inner(root, GitReceiptCollection::Blocking)
}

pub(crate) fn repo_worktree_fingerprint_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    repo_worktree_fingerprint_inner(root, GitReceiptCollection::Cancellable(cancelled))
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

    fn git_changed_path_stdout(self, root: &Path, args: &[&str], label: &str) -> Result<Vec<u8>> {
        git_changed_path_stdout(root, args, label, self)
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
mod tests;
