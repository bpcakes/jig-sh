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

fn plan_change_snapshot_inner(
    root: &Path,
    baseline_oid: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    collection.ensure_active()?;
    let baseline_oid = resolve_git_commit_inner(root, baseline_oid, collection)
        .with_context(|| format!("Failed to resolve plan baseline commit {baseline_oid}"))?;
    plan_change_snapshot_from_resolved_oid(root, baseline_oid, collection)
}

fn plan_change_snapshot_from_empty_tree_inner(
    root: &Path,
    expected_oid: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    collection.ensure_active()?;
    let actual_oid = resolve_empty_tree_oid_inner(root, collection)?;
    if actual_oid != expected_oid {
        bail!(
            "Stored empty-tree baseline {expected_oid} does not match repository hash format {actual_oid}"
        );
    }
    plan_change_snapshot_from_resolved_oid(root, actual_oid, collection)
}

fn plan_change_snapshot_from_resolved_oid(
    root: &Path,
    baseline_oid: String,
    collection: GitReceiptCollection<'_>,
) -> Result<PlanChangeSnapshot> {
    #[cfg(test)]
    PLAN_CHANGE_COLLECTION_COUNT.set(PLAN_CHANGE_COLLECTION_COUNT.get() + 1);
    let (changed_paths, untracked_paths) =
        changed_paths_since_baseline(root, &baseline_oid, collection)?;
    Ok(PlanChangeSnapshot {
        baseline_oid,
        changed_paths,
        untracked_paths,
        scope_cache: RefCell::new(BTreeMap::new()),
    })
}

fn gate_scope_snapshot_from_plan_change_inner(
    root: &Path,
    plan: &PlanChangeSnapshot,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    gate_signature: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<GateScopeSnapshot> {
    collection.ensure_active()?;
    let key = gate_scope_policy_key(paths, paths_ignore);
    if let Some(cached) = plan.scope_cache.borrow().get(&key).cloned() {
        return cached
            .map(|snapshot| snapshot.for_gate_signature(gate_signature))
            .map_err(anyhow::Error::msg);
    }
    let snapshot = match gate_scope_input_snapshot_from_plan_change_inner(
        root,
        plan,
        key.paths.as_deref(),
        &key.paths_ignore,
        collection,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) if is_git_receipt_collection_cancellation(&error) => return Err(error),
        Err(error) => {
            let message = format!("{error:#}");
            plan.scope_cache
                .borrow_mut()
                .insert(key, Err(message.clone()));
            return Err(anyhow::Error::msg(message));
        }
    };
    plan.scope_cache
        .borrow_mut()
        .insert(key, Ok(snapshot.clone()));
    Ok(snapshot.for_gate_signature(gate_signature))
}

fn gate_scope_policy_key(paths: Option<&[String]>, paths_ignore: &[String]) -> GateScopePolicyKey {
    fn normalized(patterns: &[String]) -> Vec<String> {
        let mut patterns = patterns.to_vec();
        patterns.sort();
        patterns.dedup();
        patterns
    }

    GateScopePolicyKey {
        paths: paths.map(normalized),
        paths_ignore: normalized(paths_ignore),
    }
}

fn gate_scope_input_snapshot_from_plan_change_inner(
    root: &Path,
    plan: &PlanChangeSnapshot,
    paths: Option<&[String]>,
    paths_ignore: &[String],
    collection: GitReceiptCollection<'_>,
) -> Result<GateScopeInputSnapshot> {
    collection.ensure_active()?;
    #[cfg(test)]
    GATE_SCOPE_INPUT_COLLECTION_COUNT.set(GATE_SCOPE_INPUT_COLLECTION_COUNT.get() + 1);
    let baseline_oid = &plan.baseline_oid;
    let all_changed = &plan.changed_paths;
    let matcher = paths.map(build_gate_glob_set).transpose()?;
    let ignore_matcher = build_gate_glob_set(paths_ignore)?;
    let matching = all_changed
        .iter()
        .filter(|path| {
            is_global_gate_authority(path)
                || (matcher
                    .as_ref()
                    .is_none_or(|matcher| matcher.is_match(path))
                    && !ignore_matcher.is_match(path))
        })
        .cloned()
        .collect::<Vec<_>>();
    let applicability = if paths.is_none() || !matching.is_empty() {
        GateApplicability::Applicable
    } else {
        GateApplicability::NotApplicable
    };
    let reason = match applicability {
        GateApplicability::Applicable if paths.is_none() => {
            "gate has no path filter and is always applicable".to_string()
        }
        GateApplicability::Applicable
            if matching.iter().any(|path| is_global_gate_authority(path)) =>
        {
            format!(
                "{} changed path(s) matched, including a global gate authority",
                matching.len()
            )
        }
        GateApplicability::Applicable => {
            format!(
                "{} changed path(s) matched the gate path policy",
                matching.len()
            )
        }
        GateApplicability::NotApplicable => format!(
            "none of the {} changed path(s) matched the gate path policy",
            all_changed.len()
        ),
    };

    let input_fingerprint = gate_scope_input_fingerprint(
        root,
        baseline_oid,
        &matching,
        &plan.untracked_paths,
        collection,
    )?;
    let all_bounded = bounded_changed_paths(all_changed.clone());
    let matching_bounded = bounded_changed_paths(matching);
    Ok(GateScopeInputSnapshot {
        facts: GateScopeFacts {
            baseline_oid: baseline_oid.clone(),
            applicability,
            reason,
            changed_paths: all_bounded.preview,
            changed_path_count: all_bounded.total,
            changed_paths_truncated: all_bounded.truncated,
            changed_paths_digest: all_bounded.digest,
            matching_paths: matching_bounded.preview,
            matching_path_count: matching_bounded.total,
            matching_paths_truncated: matching_bounded.truncated,
            matching_paths_digest: matching_bounded.digest,
        },
        input_fingerprint,
    })
}

fn build_gate_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .with_context(|| format!("Invalid gate path glob '{pattern}'"))?,
        );
    }
    builder.build().context("Failed to compile gate path globs")
}

fn changed_paths_since_baseline(
    root: &Path,
    baseline_oid: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<(Vec<String>, Vec<String>)> {
    collection.ensure_active()?;
    let mut discovered_entries = 0;
    let tracked = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --name-status baseline",
    )?;
    let mut changed = Vec::new();
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &tracked,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "baseline-to-worktree diff",
        )?,
        &mut discovered_entries,
        "baseline-to-worktree diff",
    )?;
    let staged = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --cached --name-status baseline",
    )?;
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &staged,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "baseline-to-index diff",
        )?,
        &mut discovered_entries,
        "baseline-to-index diff",
    )?;
    let manifest_tracked = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".jig.toml",
            ".agent/jig-contract.json",
        ],
        "git diff --name-status contract manifest",
    )?;
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &manifest_tracked,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "contract-manifest worktree diff",
        )?,
        &mut discovered_entries,
        "contract-manifest worktree diff",
    )?;
    let manifest_staged = collection.git_changed_path_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--ignore-submodules=none",
            baseline_oid,
            "--",
            ".jig.toml",
            ".agent/jig-contract.json",
        ],
        "git diff --cached --name-status contract manifest",
    )?;
    extend_discovered_paths(
        &mut changed,
        parse_name_status_z(
            &manifest_staged,
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "contract-manifest index diff",
        )?,
        &mut discovered_entries,
        "contract-manifest index diff",
    )?;
    collection.ensure_active()?;
    let untracked_output = collection.git_changed_path_stdout(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git ls-files untracked",
    )?;
    let mut untracked = Vec::new();
    extend_discovered_paths(
        &mut untracked,
        parse_nul_utf8_paths_with_limit(
            &untracked_output,
            "git ls-files",
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "untracked files",
        )?,
        &mut discovered_entries,
        "untracked files",
    )?;
    let manifest_untracked = collection.git_changed_path_stdout(
        root,
        &[
            "ls-files",
            "--others",
            "-z",
            "--",
            ".jig.toml",
            ".agent/jig-contract.json",
        ],
        "git ls-files untracked contract manifest",
    )?;
    extend_discovered_paths(
        &mut untracked,
        parse_nul_utf8_paths_with_limit(
            &manifest_untracked,
            "git ls-files contract manifest",
            MAX_CHANGED_PATH_DISCOVERY_ENTRIES - discovered_entries,
            "untracked contract manifests",
        )?,
        &mut discovered_entries,
        "untracked contract manifests",
    )?;
    untracked.sort();
    untracked.dedup();
    changed.extend(untracked.iter().cloned());
    changed.sort();
    changed.dedup();
    Ok((changed, untracked))
}

fn extend_discovered_paths(
    destination: &mut Vec<String>,
    paths: Vec<String>,
    discovered_entries: &mut usize,
    label: &str,
) -> Result<()> {
    extend_discovered_paths_with_limit(
        destination,
        paths,
        discovered_entries,
        label,
        MAX_CHANGED_PATH_DISCOVERY_ENTRIES,
    )
}

fn extend_discovered_paths_with_limit(
    destination: &mut Vec<String>,
    paths: Vec<String>,
    discovered_entries: &mut usize,
    label: &str,
    limit: usize,
) -> Result<()> {
    let next = discovered_entries
        .checked_add(paths.len())
        .ok_or_else(|| anyhow::anyhow!("Changed-path discovery count overflowed"))?;
    if next > limit {
        bail!(
            "Changed-path discovery exceeded the limit of {limit} path entries while reading {label}; split or reduce the worktree change set before collecting gate evidence"
        );
    }
    *discovered_entries = next;
    destination.extend(paths);
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct NameStatusPath {
    status: String,
    path: String,
}

fn parse_name_status_z(stdout: &[u8], entry_limit: usize, label: &str) -> Result<Vec<String>> {
    Ok(parse_name_status_paths_z(stdout, entry_limit, label)?
        .into_iter()
        .map(|entry| entry.path)
        .collect())
}

fn parse_name_status_paths_z(
    stdout: &[u8],
    entry_limit: usize,
    label: &str,
) -> Result<Vec<NameStatusPath>> {
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    let mut paths = Vec::new();
    while let Some(status) = fields.next() {
        if status.is_empty() {
            if fields.peek().is_some() {
                bail!("Malformed git diff --name-status -z output: empty status field");
            }
            break;
        }
        let status = std::str::from_utf8(status).context("Git diff status was not UTF-8")?;
        let path_count = usize::from(status.starts_with('R') || status.starts_with('C')) + 1;
        for _ in 0..path_count {
            let path = fields
                .next()
                .filter(|field| !field.is_empty())
                .ok_or_else(|| anyhow::anyhow!("Malformed git diff --name-status -z output"))?;
            if paths.len() == entry_limit {
                bail!(
                    "Changed-path discovery exceeded the remaining limit of {entry_limit} path entries while parsing {label}; split or reduce the worktree change set before collecting gate evidence"
                );
            }
            paths.push(NameStatusPath {
                status: status.to_string(),
                path: std::str::from_utf8(path)
                    .context("Changed repository path was not UTF-8")?
                    .to_string(),
            });
        }
    }
    Ok(paths)
}

fn parse_nul_utf8_paths(stdout: &[u8], label: &str) -> Result<Vec<String>> {
    parse_nul_utf8_paths_with_limit(stdout, label, usize::MAX, label)
}

fn parse_nul_utf8_paths_with_limit(
    stdout: &[u8],
    label: &str,
    entry_limit: usize,
    discovery_label: &str,
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    while let Some(path) = fields.next() {
        if path.is_empty() {
            if fields.peek().is_some() {
                bail!("Malformed {label} -z output: empty path field");
            }
            break;
        }
        if paths.len() == entry_limit {
            bail!(
                "Changed-path discovery exceeded the remaining limit of {entry_limit} path entries while parsing {discovery_label}; split or reduce the worktree change set before collecting gate evidence"
            );
        }
        paths.push(
            std::str::from_utf8(path)
                .with_context(|| format!("{label} path was not UTF-8"))?
                .to_string(),
        );
    }
    Ok(paths)
}

fn gate_scope_input_fingerprint(
    root: &Path,
    baseline_oid: &str,
    matching_paths: &[String],
    untracked: &[String],
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    collection.ensure_active()?;
    let mut digest = Sha256::new();
    digest.update(GATE_SCOPE_INPUT_FINGERPRINT_DOMAIN);
    hash_field(&mut digest, baseline_oid.as_bytes());
    let all_matching = matching_paths.iter().collect::<Vec<_>>();
    ensure_no_partially_staged_paths(root, baseline_oid, &all_matching, collection)?;
    let tracked_matching = matching_paths
        .iter()
        .filter(|path| untracked.binary_search(path).is_err())
        .collect::<Vec<_>>();
    ensure_selected_gitlinks_are_stable(root, &tracked_matching, collection)?;
    hash_field(&mut digest, b"tracked-paths");
    hash_field(&mut digest, &(tracked_matching.len() as u64).to_be_bytes());
    for path in &tracked_matching {
        hash_field(&mut digest, path.as_bytes());
    }
    let order_file = NamedTempFile::new().context("Failed to create Git diff order file")?;
    for (chunk_index, chunk) in literal_pathspec_chunks(&tracked_matching)
        .into_iter()
        .enumerate()
    {
        collection.ensure_active()?;
        hash_field(&mut digest, b"tracked-diff-chunk");
        hash_field(&mut digest, &(chunk_index as u64).to_be_bytes());
        let mut args = canonical_binary_diff_args(order_file.path(), false, Some(baseline_oid));
        args.extend(
            chunk
                .iter()
                .map(|path| OsString::from(format!(":(top,literal){path}"))),
        );
        let mut index_args =
            canonical_binary_diff_args(order_file.path(), true, Some(baseline_oid));
        index_args.extend(
            chunk
                .iter()
                .map(|path| OsString::from(format!(":(top,literal){path}"))),
        );
        let index_diff = git_bounded_proof_stdout_os(
            root,
            &index_args,
            "git diff --cached gate scope",
            gate_scope_diff_output_limit(),
            "gate-scope proof",
            collection,
        )?;
        hash_field(&mut digest, b"baseline-to-index");
        hash_field(&mut digest, &index_diff);
        let diff = git_bounded_proof_stdout_os(
            root,
            &args,
            "git diff gate scope",
            gate_scope_diff_output_limit(),
            "gate-scope proof",
            collection,
        )?;
        hash_field(&mut digest, b"baseline-to-worktree");
        hash_field(&mut digest, &diff);
    }
    let mut remaining_inline_bytes = MAX_TOTAL_INLINE_UNTRACKED_BYTES;
    for path in untracked
        .iter()
        .filter(|path| matching_paths.binary_search(path).is_ok())
    {
        collection.ensure_active()?;
        hash_field(&mut digest, path.as_bytes());
        let full_path = root.join(path);
        let metadata = fs::symlink_metadata(&full_path).with_context(|| {
            format!("Failed to inspect gate-scope path {}", full_path.display())
        })?;
        let mut encoded = Vec::new();
        append_untracked_path_fingerprint(
            &mut encoded,
            root,
            &full_path,
            &metadata,
            &mut remaining_inline_bytes,
            collection,
        )?;
        hash_field(&mut digest, &encoded);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn ensure_no_partially_staged_paths(
    root: &Path,
    baseline_oid: &str,
    paths: &[&String],
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    let mut staged_since_baseline = BTreeSet::new();
    let mut index_to_worktree = BTreeSet::new();
    for chunk in literal_pathspec_chunks(paths) {
        collection.ensure_active()?;
        let mut staged_args = vec![
            "-c".to_string(),
            "core.fileMode=true".to_string(),
            "-c".to_string(),
            "diff.ignoreSubmodules=none".to_string(),
            "diff".to_string(),
            "--cached".to_string(),
            "--name-status".to_string(),
            "-z".to_string(),
            "--no-renames".to_string(),
            "--no-ext-diff".to_string(),
            "--ignore-submodules=none".to_string(),
            baseline_oid.to_string(),
            "--".to_string(),
        ];
        staged_args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        let staged_refs = staged_args.iter().map(String::as_str).collect::<Vec<_>>();
        let staged = collection.git_output(
            root,
            &staged_refs,
            "git diff --cached --name-status gate scope",
        )?;
        let staged_entries = parse_name_status_paths_z(
            &staged.stdout,
            usize::MAX,
            "gate-scope baseline-to-index diff",
        )?;
        for entry in staged_entries {
            if entry.status.starts_with('D') {
                ensure_staged_deletion_has_no_worktree_replacement(root, &entry.path)?;
            }
            staged_since_baseline.insert(entry.path);
        }

        let mut unstaged_args = vec![
            "-c".to_string(),
            "core.fileMode=true".to_string(),
            "-c".to_string(),
            "diff.ignoreSubmodules=none".to_string(),
            "diff".to_string(),
            "--name-only".to_string(),
            "-z".to_string(),
            "--no-renames".to_string(),
            "--no-ext-diff".to_string(),
            "--ignore-submodules=none".to_string(),
            "--".to_string(),
        ];
        unstaged_args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        let unstaged_refs = unstaged_args.iter().map(String::as_str).collect::<Vec<_>>();
        let unstaged = collection.git_output(
            root,
            &unstaged_refs,
            "git diff --name-only index to worktree gate scope",
        )?;
        index_to_worktree.extend(parse_nul_utf8_paths(
            &unstaged.stdout,
            "git diff --name-only index to worktree",
        )?);
    }

    if let Some(path) = staged_since_baseline
        .intersection(&index_to_worktree)
        .next()
    {
        bail!(
            "Cannot attest partially staged gate input {path}: the index differs from the plan baseline and the worktree differs from the index; stage the checked version or unstage the index version before recording gate evidence"
        );
    }
    Ok(())
}

fn ensure_staged_deletion_has_no_worktree_replacement(root: &Path, path: &str) -> Result<()> {
    let full_path = root.join(path);
    match fs::symlink_metadata(&full_path) {
        Ok(_) => bail!(
            "Cannot attest staged deletion {path}: the repository path still exists in the worktree and may be an ignored same-path replacement; remove the replacement, or restore and stage the checked version before recording gate evidence"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to inspect staged deletion replacement {}",
                full_path.display()
            )
        }),
    }
}

fn literal_path_chunks<T>(paths: &[T], encoded_path_bytes: impl Fn(&T) -> usize) -> Vec<&[T]> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < paths.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < paths.len() && end - start < MAX_GIT_LITERAL_PATHS_PER_DIFF {
            let pathspec_bytes = b":(top,literal)".len() + encoded_path_bytes(&paths[end]) + 1;
            if end > start && bytes + pathspec_bytes > MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF {
                break;
            }
            bytes += pathspec_bytes;
            end += 1;
        }
        chunks.push(&paths[start..end]);
        start = end;
    }
    chunks
}

fn literal_pathspec_chunks<'a>(paths: &'a [&'a String]) -> Vec<&'a [&'a String]> {
    literal_path_chunks(paths, |path| path.len())
}

fn ensure_selected_gitlinks_are_stable(
    root: &Path,
    paths: &[&String],
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    for chunk in literal_pathspec_chunks(paths) {
        let mut args = vec![
            "ls-files".to_string(),
            "--stage".to_string(),
            "-z".to_string(),
            "--".to_string(),
        ];
        args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = collection.git_output(root, &arg_refs, "git ls-files gate gitlinks")?;
        for gitlink in parse_gitlinks(&output.stdout)? {
            ensure_gitlink_checkout_is_stable(root, &gitlink, collection)?;
        }
    }
    Ok(())
}

fn ensure_worktree_gitlinks_are_stable(
    root: &Path,
    changed_tracked_paths: &[PathBuf],
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    if changed_tracked_paths.is_empty() {
        return Ok(());
    }
    for chunk in literal_os_path_chunks(changed_tracked_paths) {
        let mut args = ["ls-files", "--stage", "-z", "--"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        for path in chunk {
            let mut pathspec = OsString::from(":(top,literal)");
            pathspec.push(path);
            args.push(pathspec);
        }
        let stdout = git_worktree_proof_stdout_os(
            root,
            &args,
            "git ls-files worktree gitlinks",
            worktree_diff_output_limit(),
            collection,
        )?;
        for gitlink in parse_gitlinks(&stdout)? {
            ensure_gitlink_checkout_is_stable(root, &gitlink, collection)?;
        }
    }
    Ok(())
}

fn literal_os_path_chunks(paths: &[PathBuf]) -> Vec<&[PathBuf]> {
    literal_path_chunks(paths, |path| path.as_os_str().as_encoded_bytes().len())
}

#[derive(Debug)]
struct GitlinkIndexEntry {
    oid: String,
    path: PathBuf,
    stage: String,
}

fn parse_gitlinks(stdout: &[u8]) -> Result<Vec<GitlinkIndexEntry>> {
    let mut gitlinks = Vec::new();
    for record in stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| anyhow::anyhow!("Malformed git ls-files --stage entry"))?;
        let metadata = std::str::from_utf8(&record[..separator])
            .context("Git index entry metadata was not UTF-8")?;
        let path_bytes = &record[separator + 1..];
        if path_bytes.is_empty() {
            bail!("Malformed git ls-files --stage entry: empty path");
        }
        let mut fields = metadata.split_ascii_whitespace();
        let mode = fields.next().unwrap_or_default();
        let oid = fields.next().unwrap_or_default();
        let stage = fields.next().unwrap_or_default();
        if fields.next().is_some() || oid.is_empty() || stage.is_empty() {
            bail!("Malformed git ls-files --stage metadata");
        }
        if mode == "160000" {
            #[cfg(unix)]
            let path = path_buf_from_git_bytes(path_bytes);
            #[cfg(not(unix))]
            let path = path_buf_from_git_bytes(path_bytes)?;
            gitlinks.push(GitlinkIndexEntry {
                oid: oid.to_ascii_lowercase(),
                path,
                stage: stage.to_string(),
            });
        }
    }
    Ok(gitlinks)
}

fn ensure_gitlink_checkout_is_stable(
    root: &Path,
    gitlink: &GitlinkIndexEntry,
    collection: GitReceiptCollection<'_>,
) -> Result<()> {
    if gitlink.stage != "0" {
        bail!(
            "Cannot attest conflicted submodule gitlink {}",
            gitlink.path.display()
        );
    }
    let checkout = root.join(&gitlink.path);
    if !checkout.exists() {
        return Ok(());
    }
    if !checkout.is_dir() {
        return Ok(());
    }
    if !checkout.join(".git").exists() {
        if fs::read_dir(&checkout)
            .with_context(|| format!("Failed to inspect submodule path {}", checkout.display()))?
            .next()
            .is_none()
        {
            return Ok(());
        }
        bail!(
            "Cannot attest gitlink {} because its checkout is not an initialized submodule",
            gitlink.path.display()
        );
    }
    let head = collection.git_output(
        &checkout,
        &["rev-parse", "--verify", "HEAD"],
        "git rev-parse submodule HEAD",
    )?;
    let head = parse_git_object_oid(&head.stdout, "submodule HEAD")?;
    if head != gitlink.oid {
        bail!(
            "Cannot attest gitlink {}: checkout HEAD {head} differs from index {}",
            gitlink.path.display(),
            gitlink.oid
        );
    }
    let dirty = git_status_is_dirty(
        &checkout,
        &[
            "-c",
            "core.fileMode=true",
            "-c",
            "diff.ignoreSubmodules=none",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
        "git status submodule",
        collection,
    )?;
    if dirty {
        bail!(
            "Cannot attest gitlink {} because its checkout contains changes",
            gitlink.path.display()
        );
    }
    Ok(())
}

fn git_status_is_dirty(
    root: &Path,
    args: &[&str],
    label: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<bool> {
    collection.ensure_active()?;
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_read_only_git_environment(&mut command);
    let mut observer = GitReceiptProcessObserver { collection };
    let output = match run_owned_process_tree_with_output_policy_and_observer(
        &mut command,
        Duration::MAX,
        ProcessOutputLimits {
            stdout: 1,
            stderr: MAX_GIT_ERROR_PREVIEW_BYTES as usize,
        },
        ProcessOutputOverflowPolicy::Error,
        &mut observer,
    ) {
        Ok(output) => output,
        Err(error) if error.is_cancellation() => {
            return Err(GitReceiptCollectionCancelled.into());
        }
        Err(OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stdout)) => {
            return Ok(true);
        }
        Err(OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stderr)) => {
            bail!(
                "{label} exceeded the Git diagnostic output limit of {MAX_GIT_ERROR_PREVIEW_BYTES} bytes"
            );
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!("Failed to run {label} in {}", root.display())));
        }
    };
    let stdout = output
        .stdout
        .context("supervised Git dirtiness probe did not capture stdout")?;
    let stderr = output
        .stderr
        .context("supervised Git dirtiness probe did not capture stderr")?;
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
    require_success(&output, |output| {
        format!(
            "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    collection.ensure_active()?;
    Ok(!output.stdout.is_empty())
}

fn is_global_gate_authority(path: &str) -> bool {
    GLOBAL_GATE_AUTHORITY_PATHS.contains(&path)
}

fn gate_scope_fingerprint(
    baseline_oid: &str,
    gate_signature: &str,
    input_fingerprint: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(GATE_SCOPE_FINGERPRINT_DOMAIN);
    hash_field(&mut digest, baseline_oid.as_bytes());
    hash_field(&mut digest, gate_signature.as_bytes());
    hash_field(&mut digest, input_fingerprint.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn hash_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn canonical_binary_diff_args(
    order_file: &Path,
    cached: bool,
    baseline_oid: Option<&str>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("-c"),
        OsString::from("core.fileMode=true"),
        OsString::from("-c"),
        OsString::from("diff.ignoreSubmodules=none"),
        OsString::from("-c"),
        OsString::from("diff.algorithm=myers"),
        OsString::from("-c"),
        OsString::from("diff.indentHeuristic=false"),
        OsString::from("-c"),
        OsString::from("diff.renames=false"),
        OsString::from("-c"),
        OsString::from("diff.context=3"),
        OsString::from("-c"),
        OsString::from("diff.interHunkContext=0"),
        OsString::from("-c"),
        OsString::from("diff.relative=false"),
        OsString::from("diff"),
    ];
    if cached {
        args.push(OsString::from("--cached"));
    }
    args.extend(
        [
            "--binary",
            "--full-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--no-indent-heuristic",
            "--diff-algorithm=myers",
            "--unified=3",
            "--inter-hunk-context=0",
            "--no-relative",
            "--ignore-submodules=none",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ]
        .into_iter()
        .map(OsString::from),
    );
    let mut order_arg = OsString::from("-O");
    order_arg.push(order_file.as_os_str());
    args.push(order_arg);
    if let Some(baseline_oid) = baseline_oid {
        args.push(OsString::from(baseline_oid));
    }
    args.push(OsString::from("--"));
    args
}

fn repo_worktree_fingerprint_inner(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<String> {
    collection.ensure_active()?;
    #[cfg(test)]
    WORKTREE_FINGERPRINT_COLLECTION_COUNT.set(WORKTREE_FINGERPRINT_COLLECTION_COUNT.get() + 1);
    let status = git_worktree_proof_stdout(
        root,
        &[
            "-c",
            "core.fileMode=true",
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
        "git status --porcelain",
        worktree_status_output_limit(),
        collection,
    )?;
    collection.ensure_active()?;
    let order_file = NamedTempFile::new().context("Failed to create worktree diff order file")?;
    let mut unstaged_args = canonical_binary_diff_args(order_file.path(), false, None);
    unstaged_args.extend([OsString::from("."), OsString::from(":(exclude).agent/**")]);
    let unstaged = git_worktree_proof_stdout_os(
        root,
        &unstaged_args,
        "git diff --binary",
        worktree_diff_output_limit(),
        collection,
    )?;
    collection.ensure_active()?;
    let mut staged_args = canonical_binary_diff_args(order_file.path(), true, None);
    staged_args.extend([OsString::from("."), OsString::from(":(exclude).agent/**")]);
    let staged = git_worktree_proof_stdout_os(
        root,
        &staged_args,
        "git diff --cached --binary",
        worktree_diff_output_limit(),
        collection,
    )?;
    collection.ensure_active()?;
    let status_entries = parse_porcelain_status_z(&status)?;
    ensure_no_whole_worktree_staged_deletion_replacements(root, &status_entries)?;
    let tracked_status_paths = status_entries
        .iter()
        .filter(|entry| entry.status != "??")
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    ensure_worktree_gitlinks_are_stable(root, &tracked_status_paths, collection)?;
    let untracked = untracked_file_contents(root, &status, collection)?;

    let mut digest = Sha256::new();
    digest.update(WORKTREE_FINGERPRINT_DOMAIN);
    hash_field(&mut digest, &status);
    hash_field(&mut digest, &unstaged);
    hash_field(&mut digest, &staged);
    hash_field(&mut digest, &untracked);

    collection.ensure_active()?;
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn untracked_file_contents(
    root: &Path,
    status_stdout: &[u8],
    collection: GitReceiptCollection<'_>,
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

        let mut payload = Vec::new();
        append_untracked_path_fingerprint(
            &mut payload,
            root,
            &full_path,
            &metadata,
            &mut remaining_inline_bytes,
            collection,
        )?;
        append_length_prefixed(&mut contents, entry.path.as_os_str().as_encoded_bytes());
        append_length_prefixed(&mut contents, &payload);
    }
    collection.ensure_active()?;
    Ok(contents)
}

fn append_length_prefixed(output: &mut Vec<u8>, field: &[u8]) {
    output.extend_from_slice(&(field.len() as u64).to_be_bytes());
    output.extend_from_slice(field);
}

#[derive(Debug, Eq, PartialEq)]
struct PorcelainStatusEntry {
    status: String,
    path: PathBuf,
    original_path: Option<PathBuf>,
}

fn ensure_no_whole_worktree_staged_deletion_replacements(
    root: &Path,
    entries: &[PorcelainStatusEntry],
) -> Result<()> {
    for entry in entries {
        if entry.status.as_bytes().first() != Some(&b'D') {
            continue;
        }
        let full_path = root.join(&entry.path);
        match fs::symlink_metadata(&full_path) {
            Ok(_) => bail!(
                "Cannot attest staged deletion {}: the repository path still exists in the worktree and may be an ignored same-path replacement; remove the replacement, or restore and stage the checked version before recording gate evidence",
                entry.path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect staged deletion replacement {}",
                        full_path.display()
                    )
                });
            }
        }
    }
    Ok(())
}

fn parse_porcelain_status_z(stdout: &[u8]) -> Result<Vec<PorcelainStatusEntry>> {
    let mut fields = stdout.split(|byte| *byte == 0).peekable();
    let mut entries = Vec::new();
    while let Some(field) = fields.next() {
        if field.is_empty() {
            if fields.peek().is_none() {
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

        let original_path = if status.as_bytes().contains(&b'R')
            || status.as_bytes().contains(&b'C')
        {
            let original = fields.next().context(
                "Malformed git status --porcelain -z output: rename/copy record missing original path",
            )?;
            if original.is_empty() {
                bail!("Malformed git status --porcelain -z output: empty original path");
            }
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
        let limit = worktree_status_entry_limit();
        if entries.len() > limit {
            bail!(
                "git status --porcelain exceeded the worktree proof entry limit of {limit}; split or reduce the worktree change set before collecting evidence"
            );
        }
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
        bail!(
            "Cannot attest untracked directory {}; it may be an embedded repository",
            full_path.display()
        );
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
    contents.extend_from_slice(b"mode\0");
    #[cfg(unix)]
    contents.extend_from_slice(&(metadata.permissions().mode() & 0o7777).to_be_bytes());
    #[cfg(not(unix))]
    contents.push(u8::from(metadata.permissions().readonly()));
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

fn changed_path_git_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_CHANGED_PATH_GIT_OUTPUT_BYTES
}

fn worktree_status_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_WORKTREE_STATUS_OUTPUT_BYTES
}

fn worktree_diff_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_WORKTREE_DIFF_OUTPUT_BYTES
}

fn gate_scope_diff_output_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_GATE_SCOPE_DIFF_OUTPUT_BYTES
}

fn worktree_status_entry_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_WORKTREE_STATUS_ENTRIES
}

fn git_worktree_proof_stdout(
    root: &Path,
    args: &[&str],
    label: &str,
    limit: usize,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    git_bounded_proof_stdout(root, args, label, limit, "worktree proof", collection)
}

fn git_worktree_proof_stdout_os(
    root: &Path,
    args: &[OsString],
    label: &str,
    limit: usize,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    git_bounded_proof_stdout_os(root, args, label, limit, "worktree proof", collection)
}

fn git_bounded_proof_stdout_os(
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

fn git_bounded_proof_stdout(
    root: &Path,
    args: &[&str],
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

fn git_bounded_proof_command_stdout(
    root: &Path,
    command: &mut Command,
    label: &str,
    limit: usize,
    proof_kind: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_read_only_git_environment(command);
    let mut observer = GitReceiptProcessObserver { collection };
    let output = match run_owned_process_tree_with_output_policy_and_observer(
        command,
        Duration::MAX,
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
        Err(OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stdout)) => {
            bail!(
                "{label} exceeded the {proof_kind} Git output limit of {limit} bytes; split or reduce the change set before collecting evidence"
            );
        }
        Err(OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stderr)) => {
            bail!(
                "{label} exceeded the Git diagnostic output limit of {MAX_GIT_ERROR_PREVIEW_BYTES} bytes"
            );
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
    require_success(&output, |output| {
        format!(
            "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    collection.ensure_active()?;
    Ok(output.stdout)
}

struct GitReceiptProcessObserver<'a> {
    collection: GitReceiptCollection<'a>,
}

impl OwnedProcessObserver for GitReceiptProcessObserver<'_> {
    fn cancelled(&mut self) -> bool {
        matches!(
            self.collection,
            GitReceiptCollection::Cancellable(cancelled) if cancelled()
        )
    }
}

fn git_changed_path_stdout(
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn git_output_with_cancellation(
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn git_hash_file_with_cancellation(
    root: &Path,
    full_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    let hash = git_hash_file(root, full_path)?;
    GitReceiptCollection::Cancellable(cancelled).ensure_active()?;
    Ok(hash)
}

fn configure_read_only_git_environment(command: &mut Command) {
    // Receipt and gate fingerprint probes are observational. In particular,
    // `git status` must not refresh stat data by taking an optional index lock.
    scrub_known_repository_git_environment(command);
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
    fn read_only_git_commands_scrub_repository_and_command_config_redirects() {
        let mut command = Command::new("git");
        command
            .env("GIT_DIR", "elsewhere/.git")
            .env("GIT_WORK_TREE", "elsewhere")
            .env("GIT_INDEX_FILE", "elsewhere/index")
            .env("GIT_REPLACE_REF_BASE", "refs/replacements")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.worktree")
            .env("GIT_CONFIG_VALUE_0", "elsewhere");

        configure_read_only_git_environment(&mut command);

        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_REPLACE_REF_BASE",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_0",
        ] {
            assert_eq!(
                command
                    .get_envs()
                    .find(|(candidate, _)| *candidate == OsStr::new(name))
                    .map(|(_, value)| value),
                Some(None),
                "{name} was not scrubbed"
            );
        }
    }

    #[test]
    fn repository_redirect_environment_cannot_change_scope_or_whole_worktree_proofs() {
        let _env = crate::test_env::lock_env();
        let root = tempdir().unwrap();
        let decoy = tempdir().unwrap();
        for repo in [root.path(), decoy.path()] {
            run_git(repo, &["init"]);
            run_git(repo, &["config", "user.email", "fixture@example.com"]);
            run_git(repo, &["config", "user.name", "Fixture"]);
            std::fs::write(repo.join("tracked.txt"), "baseline\n").unwrap();
            run_git(repo, &["add", "."]);
            run_git(repo, &["commit", "-m", "baseline"]);
        }
        let baseline = resolve_git_commit(root.path(), "HEAD").unwrap();
        std::fs::write(root.path().join("tracked.txt"), "changed\n").unwrap();
        let expected_whole = repo_worktree_fingerprint(root.path()).unwrap();
        let expected_scope = gate_scope_snapshot(
            root.path(),
            &baseline,
            Some(&["tracked.txt".into()]),
            &[],
            "fixture",
        )
        .unwrap();

        for (name, value) in [
            ("GIT_DIR", decoy.path().join(".git")),
            ("GIT_WORK_TREE", decoy.path().to_path_buf()),
            ("GIT_INDEX_FILE", decoy.path().join(".git/index")),
        ] {
            let guard = crate::test_env::EnvVarGuard::set(name, &value);
            assert_eq!(
                repo_worktree_fingerprint(root.path()).unwrap(),
                expected_whole,
                "ambient {name} changed the whole-worktree proof"
            );
            assert_eq!(
                gate_scope_snapshot(
                    root.path(),
                    &baseline,
                    Some(&["tracked.txt".into()]),
                    &[],
                    "fixture",
                )
                .unwrap()
                .scope_fingerprint,
                expected_scope.scope_fingerprint,
                "ambient {name} changed the scoped proof"
            );
            drop(guard);
        }
    }

    #[cfg(unix)]
    #[test]
    fn whole_worktree_fingerprint_disables_external_diff_and_textconv_configuration() {
        use std::os::unix::fs::PermissionsExt as _;

        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        let tools = tempdir().unwrap();
        let marker = tools.path().join("external-diff-ran");
        let external = tools.path().join("external-diff.sh");
        std::fs::write(
            &external,
            format!("#!/bin/sh\n: > '{}'\nexit 0\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&external, std::fs::Permissions::from_mode(0o755)).unwrap();

        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join(".gitattributes"), "*.txt diff=fixture\n").unwrap();
        std::fs::write(temp.path().join("tracked.txt"), "baseline\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "baseline"]);
        std::fs::write(temp.path().join("tracked.txt"), "changed once\n").unwrap();
        let expected = repo_worktree_fingerprint(temp.path()).unwrap();

        let global = tools.path().join("global.gitconfig");
        std::fs::write(
            &global,
            format!(
                "[diff]\n\texternal = {}\n[diff \"fixture\"]\n\ttextconv = {}\n",
                external.display(),
                external.display()
            ),
        )
        .unwrap();
        let _global = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", &global);
        assert_eq!(repo_worktree_fingerprint(temp.path()).unwrap(), expected);
        assert!(!marker.exists(), "global diff program was executed");

        run_git(
            temp.path(),
            &["config", "diff.external", external.to_str().unwrap()],
        );
        run_git(
            temp.path(),
            &[
                "config",
                "diff.fixture.textconv",
                external.to_str().unwrap(),
            ],
        );
        assert_eq!(repo_worktree_fingerprint(temp.path()).unwrap(), expected);
        assert!(!marker.exists(), "local diff program was executed");

        std::fs::write(temp.path().join("tracked.txt"), "changed twice\n").unwrap();
        assert_ne!(repo_worktree_fingerprint(temp.path()).unwrap(), expected);
        assert!(
            !marker.exists(),
            "diff program was executed after content changed"
        );
    }

    #[test]
    fn whole_worktree_fingerprint_fails_closed_on_large_binary_diff_output() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("asset.bin"), [0_u8; 32]).unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "baseline"]);
        let mut state = 0x1234_5678_u32;
        let changed = (0..16_384)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect::<Vec<_>>();
        std::fs::write(temp.path().join("asset.bin"), changed).unwrap();

        WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(Some(256));
        let result = repo_worktree_fingerprint(temp.path());
        WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(None);
        let error = result.unwrap_err();

        assert!(
            format!("{error:#}").contains("worktree proof Git output limit of 256 bytes"),
            "{error:#}"
        );
    }

    #[test]
    fn gate_scope_fingerprint_fails_closed_on_oversized_committed_binary_diff() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("asset.bin"), [0_u8; 32]).unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "baseline"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let mut state = 0x0bad_f00d_u32;
        let changed = (0..16_384)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect::<Vec<_>>();
        std::fs::write(temp.path().join("asset.bin"), changed).unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "large binary"]);

        GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.set(Some(256));
        let result = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&["asset.bin".into()]),
            &[],
            "fixture",
        );
        GATE_SCOPE_DIFF_OUTPUT_LIMIT_OVERRIDE.set(None);
        let error = result.unwrap_err();

        assert!(
            format!("{error:#}").contains("gate-scope proof Git output limit of 256 bytes"),
            "{error:#}"
        );
    }

    #[test]
    fn whole_worktree_fingerprint_fails_closed_on_too_many_status_entries() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        for name in ["one.txt", "two.txt", "three.txt"] {
            std::fs::write(temp.path().join(name), name).unwrap();
        }

        WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE.set(Some(2));
        let result = repo_worktree_fingerprint(temp.path());
        WORKTREE_STATUS_ENTRY_LIMIT_OVERRIDE.set(None);
        let error = result.unwrap_err();

        assert!(
            format!("{error:#}").contains("worktree proof entry limit of 2"),
            "{error:#}"
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn cancellation_before_fingerprint_git_spawn_remains_typed() {
        let temp = tempdir().unwrap();
        let calls = Cell::new(0);
        let error = repo_worktree_fingerprint_with_cancellation(temp.path(), &|| {
            let current = calls.get();
            calls.set(current + 1);
            current == 1
        })
        .unwrap_err();

        assert!(is_git_receipt_collection_cancellation(&error), "{error:#}");
        assert_eq!(calls.get(), 2);
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
        assert_eq!(metadata.changed_path_count, None);
        assert!(!metadata.changed_paths_truncated);
        assert_eq!(metadata.changed_paths_digest, None);
        assert_eq!(metadata.diff_stat.files, 0);
        assert!(metadata.git_status_error.is_some());
        assert!(metadata.git_diff_stat_error.is_some());
        assert!(metadata.worktree_fingerprint.is_none());
        assert!(metadata.worktree_fingerprint_error.is_some());
    }

    #[test]
    fn cancelled_receipt_metadata_starts_no_git_subcollection() {
        let temp = tempdir().unwrap();
        let checks = Cell::new(0);

        let metadata = collect_git_receipt_metadata_with_cancellation(temp.path(), &|| {
            checks.set(checks.get() + 1);
            true
        });

        assert!(metadata.changed_paths.is_empty());
        assert_eq!(metadata.changed_path_count, None);
        assert_eq!(metadata.diff_stat.files, 0);
        assert!(
            metadata
                .git_status_error
                .as_deref()
                .is_some_and(|error| error.contains("collection was cancelled"))
        );
        assert!(
            metadata
                .git_diff_stat_error
                .as_deref()
                .is_some_and(|error| error.contains("collection was cancelled"))
        );
        assert!(metadata.worktree_fingerprint.is_none());
        assert!(
            metadata
                .worktree_fingerprint_error
                .as_deref()
                .is_some_and(|error| error.contains("collection was cancelled"))
        );
        assert_eq!(checks.get(), 3);
    }

    #[test]
    fn changed_paths_preserve_spaces_and_rename_paths() {
        let _env = crate::test_env::lock_env();
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

    #[test]
    fn receipt_metadata_excludes_agent_state_from_paths_and_diff_stat() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
        std::fs::write(temp.path().join("src.rs"), "one\n").unwrap();
        std::fs::write(temp.path().join(".agent/state/receipts.jsonl"), "old\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        std::fs::write(temp.path().join("src.rs"), "one\ntwo\n").unwrap();
        std::fs::write(
            temp.path().join(".agent/state/receipts.jsonl"),
            "old\nnew\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("note.txt"), "untracked\n").unwrap();
        std::fs::write(
            temp.path().join(".agent/state/untracked.jsonl"),
            "ignored by receipt metadata\n",
        )
        .unwrap();

        let metadata = collect_git_receipt_metadata_without_worktree_fingerprint(temp.path());

        assert_eq!(metadata.changed_paths, ["note.txt", "src.rs"]);
        assert_eq!(metadata.changed_path_count, Some(2));
        assert!(!metadata.changed_paths_truncated);
        assert!(metadata.changed_paths_digest.is_some());
        assert_eq!(metadata.diff_stat.files, 1);
        assert_eq!(metadata.diff_stat.insertions, 1);
        assert_eq!(metadata.diff_stat.deletions, 0);
    }

    #[test]
    fn changed_path_preview_is_bounded_sorted_and_digest_covers_the_full_set() {
        let paths = (0..105)
            .rev()
            .map(|index| format!("src/path-{index:03}.rs"))
            .chain(["src/path-042.rs".to_string()])
            .collect::<Vec<_>>();
        let bounded = bounded_changed_paths(paths.clone());
        let reordered = bounded_changed_paths(paths.into_iter().rev().collect());

        assert_eq!(bounded.preview.len(), MAX_RECEIPT_CHANGED_PATHS);
        assert_eq!(bounded.total, 105);
        assert!(bounded.truncated);
        assert_eq!(bounded.preview[0], "src/path-000.rs");
        assert_eq!(bounded.preview[99], "src/path-099.rs");
        assert!(bounded.digest.starts_with("sha256:"));
        assert_eq!(bounded.digest, reordered.digest);
        assert_eq!(bounded.preview, reordered.preview);

        let preview_only_digest = changed_paths_digest(&bounded.preview);
        assert_ne!(bounded.digest, preview_only_digest);
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

    #[cfg(unix)]
    #[test]
    fn whole_worktree_fingerprint_preserves_non_utf8_tracked_paths() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        let file_name = OsString::from_vec(b"tracked-\xff.rs".to_vec());
        let path = temp.path().join(&file_name);
        fs::write(&path, "one\n").unwrap();
        let added = Command::new("git")
            .current_dir(temp.path())
            .arg("add")
            .arg("--")
            .arg(&file_name)
            .output()
            .unwrap();
        assert!(added.status.success(), "{:?}", added.stderr);
        run_git(temp.path(), &["commit", "-m", "non-UTF-8 fixture"]);

        let clean = repo_worktree_fingerprint(temp.path()).unwrap();
        fs::write(&path, "two\n").unwrap();
        let changed = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(clean, changed);
    }

    #[test]
    fn worktree_gitlink_probe_scales_with_changed_paths_not_full_index() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        for index in 0..100 {
            fs::write(
                temp.path()
                    .join(format!("unrelated-index-entry-{index:03}.txt")),
                "stable\n",
            )
            .unwrap();
        }
        fs::write(temp.path().join("selected.txt"), "one\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "large index fixture"]);
        fs::write(temp.path().join("selected.txt"), "two\n").unwrap();

        WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(Some(512));
        let result = repo_worktree_fingerprint(temp.path());
        WORKTREE_PROOF_GIT_OUTPUT_LIMIT_OVERRIDE.set(None);

        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn worktree_fingerprint_changes_when_untracked_file_content_changes() {
        let _env = crate::test_env::lock_env();
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

    #[cfg(unix)]
    #[test]
    fn worktree_fingerprint_frames_untracked_entries_against_nul_collisions() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        let first_path = temp.path().join("a");
        let second_path = temp.path().join("b");
        fs::write(&first_path, b"x").unwrap();
        fs::write(&second_path, b"placeholder").unwrap();
        fs::set_permissions(&first_path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&second_path, fs::Permissions::from_mode(0o644)).unwrap();

        let mut old_entry_boundary = b"\0b\0mode\0".to_vec();
        old_entry_boundary.extend_from_slice(&0o644_u32.to_be_bytes());
        old_entry_boundary.extend_from_slice(b"file\0");
        let first_second = [b"y".as_slice(), &old_entry_boundary, b"z"].concat();
        fs::write(&second_path, first_second).unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();

        let second_first = [b"x".as_slice(), &old_entry_boundary, b"y"].concat();
        fs::write(&first_path, second_first).unwrap();
        fs::write(&second_path, b"z").unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn fingerprints_change_when_an_untracked_file_execution_mode_changes() {
        use std::os::unix::fs::PermissionsExt as _;

        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked\n").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script = temp.path().join("scripts/check.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
        let paths = vec!["scripts/**".to_string()];
        let whole_before = repo_worktree_fingerprint(temp.path()).unwrap();
        let scope_before =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();

        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let whole_after = repo_worktree_fingerprint(temp.path()).unwrap();
        let scope_after =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();

        assert_ne!(whole_before, whole_after);
        assert_ne!(
            scope_before.scope_fingerprint,
            scope_after.scope_fingerprint
        );
    }

    #[test]
    fn empty_tree_baseline_classifies_initial_repository_files() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "pub fn initial() {}\n").unwrap();
        let empty_tree = resolve_empty_tree_for_unborn_repository(temp.path())
            .unwrap()
            .unwrap();
        let plan = plan_change_snapshot_from_empty_tree(temp.path(), &empty_tree).unwrap();
        let paths = vec!["src/**".to_string()];

        let scope = gate_scope_snapshot_from_plan_change(
            temp.path(),
            &plan,
            Some(&paths),
            &[],
            "initial-rust",
        )
        .unwrap();

        assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
        assert_eq!(scope.facts.matching_paths, ["src/lib.rs"]);
    }

    #[test]
    fn masked_staged_change_is_classified_and_fails_scope_proof() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        let source = temp.path().join("src/lib.rs");
        std::fs::write(&source, "pub const VALUE: u8 = 1;\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "baseline"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();

        std::fs::write(&source, "pub const VALUE: u8 = 2;\n").unwrap();
        run_git(temp.path(), &["add", "src/lib.rs"]);
        std::fs::write(&source, "pub const VALUE: u8 = 1;\n").unwrap();

        let plan = plan_change_snapshot(temp.path(), &baseline).unwrap();
        assert!(plan.changed_paths.iter().any(|path| path == "src/lib.rs"));
        let paths = vec!["src/**".to_string()];
        let error =
            gate_scope_snapshot_from_plan_change(temp.path(), &plan, Some(&paths), &[], "rust")
                .unwrap_err()
                .to_string();
        assert!(
            error.contains("partially staged gate input src/lib.rs"),
            "{error}"
        );

        std::fs::write(&source, "pub const VALUE: u8 = 2;\n").unwrap();
        let aligned = plan_change_snapshot(temp.path(), &baseline).unwrap();
        let scope =
            gate_scope_snapshot_from_plan_change(temp.path(), &aligned, Some(&paths), &[], "rust")
                .unwrap();
        assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
    }

    #[test]
    fn staged_deletion_with_ignored_same_path_replacement_fails_all_evidence_closed() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join(".gitignore"), "ignored-input.txt\n").unwrap();
        std::fs::write(temp.path().join("ignored-input.txt"), "baseline\n").unwrap();
        run_git(
            temp.path(),
            &["add", "-f", ".gitignore", "ignored-input.txt"],
        );
        run_git(temp.path(), &["commit", "-m", "baseline"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        run_git(temp.path(), &["rm", "--cached", "ignored-input.txt"]);
        let paths = vec!["ignored-input.txt".to_string()];

        for replacement in ["first replacement\n", "different replacement\n"] {
            std::fs::write(temp.path().join("ignored-input.txt"), replacement).unwrap();
            let plan = plan_change_snapshot(temp.path(), &baseline).unwrap();
            assert!(
                plan.changed_paths
                    .iter()
                    .any(|path| path == "ignored-input.txt")
            );
            assert!(plan.untracked_paths.is_empty());

            let scoped_error = gate_scope_snapshot_from_plan_change(
                temp.path(),
                &plan,
                Some(&paths),
                &[],
                "ignored-replacement",
            )
            .unwrap_err()
            .to_string();
            assert!(
                scoped_error.contains("staged deletion ignored-input.txt"),
                "{scoped_error}"
            );

            let whole_error = repo_worktree_fingerprint(temp.path())
                .unwrap_err()
                .to_string();
            assert!(
                whole_error.contains("staged deletion ignored-input.txt"),
                "{whole_error}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn canonical_diff_order_file_preserves_non_utf8_temporary_directory() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("source.txt"), "baseline\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "baseline"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        std::fs::write(temp.path().join("source.txt"), "changed\n").unwrap();

        let raw_temp = temp
            .path()
            .join(OsString::from_vec(b"proof-temp-\xff".to_vec()));
        std::fs::create_dir(&raw_temp).unwrap();
        let _tmpdir = crate::test_env::EnvVarGuard::set("TMPDIR", &raw_temp);

        let whole = repo_worktree_fingerprint(temp.path()).unwrap();
        assert!(whole.starts_with("sha256:"));
        let scope = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&["source.txt".to_string()]),
            &[],
            "non-utf8-temp",
        )
        .unwrap();
        assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
        assert!(scope.scope_fingerprint.starts_with("sha256:"));
    }

    #[test]
    fn gate_scope_is_path_sensitive_and_ignores_unrelated_fingerprint_changes() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        std::fs::create_dir_all(temp.path().join("crates/api/src")).unwrap();
        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            "export const v = 1;\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("crates/api/src/lib.rs"),
            "pub const V: u8 = 1;\n",
        )
        .unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let rust_paths = vec!["crates/**".to_string()];
        let before = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&rust_paths),
            &[],
            "rust-signature",
        )
        .unwrap();
        assert_eq!(before.facts.applicability, GateApplicability::NotApplicable);

        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            "export const v = 2;\n",
        )
        .unwrap();
        let frontend_only = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&rust_paths),
            &[],
            "rust-signature",
        )
        .unwrap();
        assert_eq!(
            frontend_only.facts.applicability,
            GateApplicability::NotApplicable
        );
        assert_eq!(before.scope_fingerprint, frontend_only.scope_fingerprint);

        std::fs::write(
            temp.path().join("crates/api/src/lib.rs"),
            "pub const V: u8 = 2;\n",
        )
        .unwrap();
        let rust_changed = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&rust_paths),
            &[],
            "rust-signature",
        )
        .unwrap();
        assert_eq!(
            rust_changed.facts.applicability,
            GateApplicability::Applicable
        );
        assert_ne!(
            frontend_only.scope_fingerprint,
            rust_changed.scope_fingerprint
        );
        assert_eq!(rust_changed.facts.matching_paths, ["crates/api/src/lib.rs"]);
        let changed_signature = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&rust_paths),
            &[],
            "changed-rust-signature",
        )
        .unwrap();
        assert_ne!(
            rust_changed.scope_fingerprint, changed_signature.scope_fingerprint,
            "gate policy and command changes must invalidate scoped evidence"
        );
    }

    #[test]
    fn supported_gate_globs_select_the_same_tracked_diff_they_classify() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            "export const value = 1;\n",
        )
        .unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();

        for pattern in [
            "apps/*/main.ts",
            "apps/web/main.?s",
            "apps/[w]eb/**",
            "apps/**",
        ] {
            let paths = vec![pattern.to_string()];
            let before = gate_scope_snapshot(
                temp.path(),
                &baseline,
                Some(&paths),
                &[],
                "frontend-signature",
            )
            .unwrap();
            assert_eq!(before.facts.applicability, GateApplicability::NotApplicable);

            std::fs::write(
                temp.path().join("apps/web/main.ts"),
                format!("export const selectedBy = {pattern:?};\n"),
            )
            .unwrap();
            let changed = gate_scope_snapshot(
                temp.path(),
                &baseline,
                Some(&paths),
                &[],
                "frontend-signature",
            )
            .unwrap();
            assert_eq!(
                changed.facts.applicability,
                GateApplicability::Applicable,
                "classification did not select {pattern}"
            );
            assert_eq!(changed.facts.matching_paths, ["apps/web/main.ts"]);
            assert_ne!(
                before.scope_fingerprint, changed.scope_fingerprint,
                "Git fingerprint pathspec did not select {pattern}"
            );
            std::fs::write(
                temp.path().join("apps/web/main.ts"),
                "export const value = 1;\n",
            )
            .unwrap();
        }
    }

    #[test]
    fn classifier_selected_directory_paths_are_hashed_literally() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::write(temp.path().join("docs/guide.md"), "before\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let paths = vec!["**".to_string()];

        for ignored in ["docs", "docs/"] {
            let ignores = vec![ignored.to_string()];
            let before =
                gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &ignores, "docs")
                    .unwrap();
            std::fs::write(
                temp.path().join("docs/guide.md"),
                format!("selected despite {ignored:?}\n"),
            )
            .unwrap();
            let after = gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &ignores, "docs")
                .unwrap();
            assert_eq!(after.facts.applicability, GateApplicability::Applicable);
            assert_eq!(after.facts.matching_paths, ["docs/guide.md"]);
            assert_ne!(before.scope_fingerprint, after.scope_fingerprint);
            std::fs::write(temp.path().join("docs/guide.md"), "before\n").unwrap();
        }
    }

    #[test]
    fn global_gate_authorities_apply_to_every_path_aware_gate() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join(".agent")).unwrap();
        std::fs::write(temp.path().join(".jig.toml"), "contract_version = 5\n").unwrap();
        std::fs::write(temp.path().join(".agent/jig-contract.json"), "{}\n").unwrap();
        std::fs::write(temp.path().join("src.rs"), "pub fn stable() {}\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let paths = vec!["crates/**".to_string()];
        let ignores = vec![".jig.toml".to_string()];

        std::fs::write(
            temp.path().join(".jig.toml"),
            "contract_version = 5\n# changed\n",
        )
        .unwrap();
        let config_scope =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &ignores, "rust").unwrap();
        assert_eq!(
            config_scope.facts.applicability,
            GateApplicability::Applicable
        );
        assert_eq!(config_scope.facts.matching_paths, [".jig.toml"]);

        std::fs::write(temp.path().join(".jig.toml"), "contract_version = 5\n").unwrap();
        std::fs::write(
            temp.path().join(".agent/jig-contract.json"),
            "{\"changed\":true}\n",
        )
        .unwrap();
        let manifest_scope =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "rust").unwrap();
        assert_eq!(
            manifest_scope.facts.applicability,
            GateApplicability::Applicable
        );
        assert_eq!(
            manifest_scope.facts.matching_paths,
            [".agent/jig-contract.json"]
        );
    }

    #[test]
    fn gate_scope_diff_is_independent_of_ambient_git_diff_configuration() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(
            temp.path().join("source.txt"),
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("second.txt"), "alpha\nbeta\ngamma\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        std::fs::write(
            temp.path().join("source.txt"),
            "zero\none\nthree\nfour\nfive\nchanged\nseven\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("second.txt"), "alpha\nchanged\ngamma\n").unwrap();
        let paths = vec!["*.txt".to_string()];
        let before =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "source").unwrap();

        run_git(temp.path(), &["config", "diff.algorithm", "histogram"]);
        run_git(temp.path(), &["config", "diff.indentHeuristic", "true"]);
        run_git(temp.path(), &["config", "diff.renames", "copies"]);
        run_git(temp.path(), &["config", "diff.mnemonicPrefix", "true"]);
        run_git(temp.path(), &["config", "diff.context", "0"]);
        run_git(temp.path(), &["config", "diff.interHunkContext", "99"]);
        run_git(temp.path(), &["config", "diff.relative", "true"]);
        std::fs::write(temp.path().join("diff-order"), "second.txt\nsource.txt\n").unwrap();
        run_git(temp.path(), &["config", "diff.orderFile", "diff-order"]);
        let after =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "source").unwrap();

        assert_eq!(before.scope_fingerprint, after.scope_fingerprint);
    }

    #[cfg(unix)]
    #[test]
    fn tracked_execution_mode_changes_are_classified_when_core_file_mode_is_disabled() {
        use std::os::unix::fs::PermissionsExt as _;

        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        let script = temp.path().join("scripts/check.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let paths = vec!["scripts/**".to_string()];
        let whole_before = repo_worktree_fingerprint(temp.path()).unwrap();
        let before =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();
        run_git(temp.path(), &["config", "core.fileMode", "false"]);

        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let after =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "scripts").unwrap();
        let whole_after = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_eq!(before.facts.applicability, GateApplicability::NotApplicable);
        assert_eq!(after.facts.applicability, GateApplicability::Applicable);
        assert_eq!(after.facts.matching_paths, ["scripts/check.sh"]);
        assert_ne!(before.scope_fingerprint, after.scope_fingerprint);
        assert_ne!(whole_before, whole_after);
    }

    #[test]
    fn literal_pathspec_chunking_bounds_many_thousand_paths() {
        let paths = (0..5_000)
            .map(|index| {
                format!("generated/very-long-component-{index:05}/source-file-{index:05}.rs")
            })
            .collect::<Vec<_>>();
        let references = paths.iter().collect::<Vec<_>>();
        let chunks = literal_pathspec_chunks(&references);

        assert!(chunks.len() > 1);
        assert_eq!(chunks.iter().map(|chunk| chunk.len()).sum::<usize>(), 5_000);
        for chunk in chunks {
            assert!(chunk.len() <= MAX_GIT_LITERAL_PATHS_PER_DIFF);
            assert!(
                chunk
                    .iter()
                    .map(|path| b":(top,literal)".len() + path.len() + 1)
                    .sum::<usize>()
                    <= MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF
            );
        }
    }

    #[test]
    fn literal_pathspec_chunking_allows_one_oversized_path_and_keeps_progress() {
        let paths = [
            "x".repeat(MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF + 1),
            "tail.rs".to_string(),
        ];
        let references = paths.iter().collect::<Vec<_>>();

        let chunks = literal_pathspec_chunks(&references);

        assert_eq!(chunks, [&references[..1], &references[1..]]);
    }

    #[cfg(unix)]
    #[test]
    fn literal_os_path_chunking_uses_raw_encoded_byte_lengths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let paths = [
            PathBuf::from(OsString::from_vec(vec![0xff; 40 * 1024])),
            PathBuf::from(OsString::from_vec(vec![0xfe; 40 * 1024])),
        ];

        let chunks = literal_os_path_chunks(&paths);

        assert_eq!(chunks, [&paths[..1], &paths[1..]]);
        assert_eq!(
            chunks.into_iter().flatten().cloned().collect::<Vec<_>>(),
            paths
        );
    }

    #[test]
    fn gate_scope_hashes_thousands_of_tracked_paths_in_bounded_git_calls() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("generated")).unwrap();
        for index in 0..3_000 {
            std::fs::write(
                temp.path().join(format!("generated/file-{index:04}.txt")),
                "before\n",
            )
            .unwrap();
        }
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        for index in 0..3_000 {
            std::fs::write(
                temp.path().join(format!("generated/file-{index:04}.txt")),
                "after\n",
            )
            .unwrap();
        }

        let scope = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&["generated/**".into()]),
            &[],
            "generated",
        )
        .unwrap();

        assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
        assert_eq!(scope.facts.matching_path_count, 3_000);
        assert!(scope.facts.matching_paths_truncated);
        assert!(scope.scope_fingerprint.starts_with("sha256:"));
    }

    #[test]
    fn gate_scope_streams_a_large_binary_diff_into_the_fingerprint() {
        fn fixture_bytes(mut state: u64, length: usize) -> Vec<u8> {
            (0..length)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state as u8
                })
                .collect()
        }

        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("assets")).unwrap();
        let asset = temp.path().join("assets/blob.bin");
        std::fs::write(&asset, fixture_bytes(1, 5 * 1024 * 1024)).unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let paths = vec!["assets/**".to_string()];
        let before =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "asset-signature")
                .unwrap();

        std::fs::write(&asset, fixture_bytes(2, 5 * 1024 * 1024)).unwrap();
        let changed =
            gate_scope_snapshot(temp.path(), &baseline, Some(&paths), &[], "asset-signature")
                .unwrap();

        assert_eq!(changed.facts.applicability, GateApplicability::Applicable);
        assert_eq!(changed.facts.matching_paths, ["assets/blob.bin"]);
        assert_ne!(before.scope_fingerprint, changed.scope_fingerprint);
    }

    #[test]
    fn prepared_plan_change_snapshot_feeds_multiple_gate_scopes() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        std::fs::create_dir_all(temp.path().join("crates/api")).unwrap();
        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            "export const v = 1;\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("crates/api/lib.rs"),
            "pub const V: u8 = 1;\n",
        )
        .unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        std::fs::write(
            temp.path().join("apps/web/main.ts"),
            "export const v = 2;\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("crates/api/lib.rs"),
            "pub const V: u8 = 2;\n",
        )
        .unwrap();

        let plan = plan_change_snapshot(temp.path(), &baseline).unwrap();
        GATE_SCOPE_INPUT_COLLECTION_COUNT.set(0);
        let frontend = gate_scope_snapshot_from_plan_change(
            temp.path(),
            &plan,
            Some(&["apps/**".into()]),
            &[],
            "frontend-signature",
        )
        .unwrap();
        let rust = gate_scope_snapshot_from_plan_change(
            temp.path(),
            &plan,
            Some(&["crates/**".into()]),
            &[],
            "rust-signature",
        )
        .unwrap();

        assert_eq!(frontend.facts.applicability, GateApplicability::Applicable);
        assert_eq!(rust.facts.applicability, GateApplicability::Applicable);
        assert_eq!(frontend.facts.changed_path_count, 2);
        assert_eq!(rust.facts.changed_path_count, 2);
        assert_eq!(frontend.facts.matching_paths, ["apps/web/main.ts"]);
        assert_eq!(rust.facts.matching_paths, ["crates/api/lib.rs"]);
        assert_eq!(GATE_SCOPE_INPUT_COLLECTION_COUNT.get(), 2);

        let equivalent_frontend = gate_scope_snapshot_from_plan_change(
            temp.path(),
            &plan,
            Some(&["apps/**".into(), "apps/**".into()]),
            &[],
            "another-frontend-signature",
        )
        .unwrap();
        assert_eq!(GATE_SCOPE_INPUT_COLLECTION_COUNT.get(), 2);
        assert_eq!(
            equivalent_frontend.facts.matching_paths,
            ["apps/web/main.ts"],
            "normalized equivalent policies must share their cached input snapshot"
        );
        assert_eq!(
            frontend.facts, equivalent_frontend.facts,
            "signature binding must preserve every cached gate-scope fact"
        );
        assert_ne!(
            frontend.scope_fingerprint, equivalent_frontend.scope_fingerprint,
            "the cached input must still be bound to each gate signature"
        );
    }

    #[cfg(unix)]
    #[test]
    fn plan_change_snapshot_fails_closed_for_non_utf8_repository_paths() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let invalid = OsString::from_vec(b"bad-\xff.rs".to_vec());
        std::fs::write(temp.path().join(invalid), "untracked\n").unwrap();

        let error = plan_change_snapshot(temp.path(), &baseline).unwrap_err();

        assert!(
            format!("{error:#}").contains("git ls-files path was not UTF-8"),
            "{error:#}"
        );
    }

    #[test]
    fn plan_change_snapshot_fails_closed_when_changed_path_output_exceeds_limit() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        let path = temp
            .path()
            .join("a-tracked-file-name-longer-than-the-test-output-limit.txt");
        std::fs::write(&path, "baseline\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        std::fs::write(path, "changed\n").unwrap();

        CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE.set(Some(16));
        let result = plan_change_snapshot(temp.path(), &baseline);
        CHANGED_PATH_GIT_OUTPUT_LIMIT_OVERRIDE.set(None);
        let error = result.unwrap_err();

        assert!(
            format!("{error:#}").contains("changed-path Git output limit of 16 bytes"),
            "{error:#}"
        );
    }

    #[test]
    fn changed_path_discovery_fails_closed_at_the_entry_ceiling() {
        let mut destination = vec!["one".into()];
        let mut discovered_entries = 1;

        let error = extend_discovered_paths_with_limit(
            &mut destination,
            vec!["two".into(), "three".into()],
            &mut discovered_entries,
            "test paths",
            2,
        )
        .unwrap_err();

        assert!(error.to_string().contains("limit of 2 path entries"));
        assert_eq!(destination, ["one"]);
        assert_eq!(discovered_entries, 1);
        assert!(
            parse_nul_utf8_paths_with_limit(b"one\0two\0", "test", 1, "test paths")
                .unwrap_err()
                .to_string()
                .contains("remaining limit of 1 path entries")
        );
        assert!(
            parse_name_status_z(b"M\0one\0M\0two\0", 1, "test diff")
                .unwrap_err()
                .to_string()
                .contains("remaining limit of 1 path entries")
        );
    }

    #[test]
    fn gate_scope_classifies_both_sides_of_a_rename() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("crates/api")).unwrap();
        std::fs::write(temp.path().join("crates/api/lib.rs"), "pub fn value() {}\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::rename(
            temp.path().join("crates/api/lib.rs"),
            temp.path().join("docs/lib.rs"),
        )
        .unwrap();

        let snapshot = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&["crates/**".into()]),
            &[],
            "rust-signature",
        )
        .unwrap();
        assert_eq!(snapshot.facts.applicability, GateApplicability::Applicable);
        assert!(
            snapshot
                .facts
                .matching_paths
                .contains(&"crates/api/lib.rs".into())
        );
    }

    #[test]
    fn gate_scope_honors_ignores_and_hashes_matching_untracked_content() {
        let _env = crate::test_env::lock_env();
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::create_dir_all(temp.path().join("crates/api/generated")).unwrap();
        std::fs::write(temp.path().join("crates/api/lib.rs"), "pub fn value() {}\n").unwrap();
        run_git(temp.path(), &["add", "."]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);
        let baseline = resolve_git_commit(temp.path(), "HEAD").unwrap();
        let paths = vec!["crates/**".into()];
        let ignores = vec!["crates/**/generated/**".into()];

        std::fs::write(
            temp.path().join("crates/api/generated/cache.sql"),
            "ignored one\n",
        )
        .unwrap();
        let ignored = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &ignores,
            "rust-signature",
        )
        .unwrap();
        assert_eq!(
            ignored.facts.applicability,
            GateApplicability::NotApplicable
        );
        std::fs::write(
            temp.path().join("crates/api/generated/cache.sql"),
            "ignored two\n",
        )
        .unwrap();
        let ignored_changed = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &ignores,
            "rust-signature",
        )
        .unwrap();
        assert_eq!(ignored.scope_fingerprint, ignored_changed.scope_fingerprint);

        std::fs::write(temp.path().join("crates/api/query.sql"), "select 1;\n").unwrap();
        let first = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &ignores,
            "rust-signature",
        )
        .unwrap();
        assert_eq!(first.facts.applicability, GateApplicability::Applicable);
        assert_eq!(first.facts.matching_paths, ["crates/api/query.sql"]);
        std::fs::write(temp.path().join("crates/api/query.sql"), "select 2;\n").unwrap();
        let second = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &ignores,
            "rust-signature",
        )
        .unwrap();
        assert_ne!(first.scope_fingerprint, second.scope_fingerprint);

        run_git(temp.path(), &["add", "crates/api/query.sql"]);
        run_git(temp.path(), &["commit", "-m", "commit during plan"]);
        let committed = gate_scope_snapshot(
            temp.path(),
            &baseline,
            Some(&paths),
            &ignores,
            "rust-signature",
        )
        .unwrap();
        assert_eq!(committed.facts.applicability, GateApplicability::Applicable);
        assert_eq!(committed.facts.matching_paths, ["crates/api/query.sql"]);
    }

    #[test]
    fn gate_scope_cancellation_before_baseline_resolution_remains_typed() {
        let temp = tempdir().unwrap();
        let error = gate_scope_snapshot_with_cancellation(
            temp.path(),
            "HEAD",
            Some(&["crates/**".into()]),
            &[],
            "signature",
            &|| true,
        )
        .unwrap_err();

        assert!(is_git_receipt_collection_cancellation(&error), "{error:#}");
    }

    #[test]
    fn worktree_fingerprint_changes_when_large_untracked_file_content_changes() {
        let _env = crate::test_env::lock_env();
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
        let _env = crate::test_env::lock_env();
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

    #[test]
    fn dirty_submodules_fail_closed_even_when_ambient_config_ignores_them() {
        let _env = crate::test_env::lock_env();
        let dependency = tempdir().unwrap();
        run_git(dependency.path(), &["init"]);
        run_git(
            dependency.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(dependency.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(dependency.path().join("source.txt"), "one\n").unwrap();
        run_git(dependency.path(), &["add", "."]);
        run_git(dependency.path(), &["commit", "-m", "dependency"]);

        let parent = tempdir().unwrap();
        run_git(parent.path(), &["init"]);
        run_git(
            parent.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(parent.path(), &["config", "user.name", "Fixture"]);
        run_git(
            parent.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                dependency.path().to_str().unwrap(),
                "vendor/dependency",
            ],
        );
        run_git(parent.path(), &["add", "."]);
        run_git(parent.path(), &["commit", "-m", "parent"]);
        let baseline = resolve_git_commit(parent.path(), "HEAD").unwrap();
        run_git(parent.path(), &["config", "diff.ignoreSubmodules", "all"]);
        let untracked = parent.path().join("vendor/dependency/generated");
        std::fs::create_dir_all(&untracked).unwrap();
        for index in 0..2_000 {
            std::fs::write(untracked.join(format!("entry-{index:04}.txt")), "dirty\n").unwrap();
        }

        let scope = gate_scope_snapshot(
            parent.path(),
            &baseline,
            Some(&["vendor/**".into()]),
            &[],
            "submodule",
        )
        .unwrap_err()
        .to_string();
        let whole = repo_worktree_fingerprint(parent.path())
            .unwrap_err()
            .to_string();

        assert!(
            scope.contains("gitlink") || scope.contains("submodule"),
            "{scope}"
        );
        assert!(
            whole.contains("gitlink") || whole.contains("submodule"),
            "{whole}"
        );
    }

    #[test]
    fn staged_clean_submodule_pointer_is_attested() {
        let _env = crate::test_env::lock_env();
        let dependency = tempdir().unwrap();
        run_git(dependency.path(), &["init"]);
        run_git(
            dependency.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(dependency.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(dependency.path().join("source.txt"), "one\n").unwrap();
        run_git(dependency.path(), &["add", "."]);
        run_git(dependency.path(), &["commit", "-m", "dependency"]);

        let parent = tempdir().unwrap();
        run_git(parent.path(), &["init"]);
        run_git(
            parent.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(parent.path(), &["config", "user.name", "Fixture"]);
        run_git(
            parent.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                dependency.path().to_str().unwrap(),
                "vendor/dependency",
            ],
        );
        run_git(parent.path(), &["add", "."]);
        run_git(parent.path(), &["commit", "-m", "parent"]);
        let baseline = resolve_git_commit(parent.path(), "HEAD").unwrap();
        let checkout = parent.path().join("vendor/dependency");
        run_git(&checkout, &["config", "user.email", "fixture@example.com"]);
        run_git(&checkout, &["config", "user.name", "Fixture"]);
        std::fs::write(checkout.join("source.txt"), "two\n").unwrap();
        run_git(&checkout, &["add", "."]);
        run_git(&checkout, &["commit", "-m", "advance dependency"]);

        assert!(
            gate_scope_snapshot(
                parent.path(),
                &baseline,
                Some(&["vendor/**".into()]),
                &[],
                "submodule",
            )
            .is_err()
        );
        run_git(parent.path(), &["add", "vendor/dependency"]);
        let scope = gate_scope_snapshot(
            parent.path(),
            &baseline,
            Some(&["vendor/**".into()]),
            &[],
            "submodule",
        )
        .unwrap();
        assert_eq!(scope.facts.applicability, GateApplicability::Applicable);
    }

    #[test]
    fn untracked_embedded_repository_cannot_be_fingerprinted_as_a_directory() {
        let _env = crate::test_env::lock_env();
        let parent = tempdir().unwrap();
        run_git(parent.path(), &["init"]);
        run_git(
            parent.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(parent.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(parent.path().join("tracked.txt"), "tracked\n").unwrap();
        run_git(parent.path(), &["add", "."]);
        run_git(parent.path(), &["commit", "-m", "parent"]);
        let baseline = resolve_git_commit(parent.path(), "HEAD").unwrap();
        let embedded = parent.path().join("vendor/embedded");
        std::fs::create_dir_all(&embedded).unwrap();
        run_git(&embedded, &["init"]);
        std::fs::write(embedded.join("source.txt"), "nested\n").unwrap();

        let scope = gate_scope_snapshot(
            parent.path(),
            &baseline,
            Some(&["vendor/**".into()]),
            &[],
            "embedded",
        )
        .unwrap_err()
        .to_string();
        let whole = repo_worktree_fingerprint(parent.path())
            .unwrap_err()
            .to_string();

        assert!(scope.contains("untracked directory"), "{scope}");
        assert!(whole.contains("untracked directory"), "{whole}");
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
