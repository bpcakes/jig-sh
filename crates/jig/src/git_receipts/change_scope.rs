use super::*;

use self::parse::{RawDiffEntry, parse_index_stage_z, parse_raw_diff_z};

pub(super) mod parse;

const MAX_SCOPE_RENAME_CANDIDATES_V1: usize = 1_000;
const MAX_SCOPE_GIT_DIAGNOSTIC_CHARS_V1: usize = 512;

#[cfg(test)]
thread_local! {
    static SCOPE_RENAME_LIMIT_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
}

#[allow(dead_code, reason = "staged native file-budget change kinds")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum FileChangeKindV1 {
    Added,
    Modified,
    TypeChanged,
    Renamed,
    Untracked,
    Unchanged,
}

#[allow(dead_code, reason = "staged native file-budget content sources")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum CurrentSourceV1 {
    WorktreePath,
    IndexBlob { oid: String },
}

#[allow(dead_code, reason = "staged native file-budget baseline authority")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BaselineFileV1 {
    pub(crate) path: String,
    pub(crate) blob_oid: String,
}

#[allow(dead_code, reason = "staged native file-budget scope entry")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ScopeEntryV1 {
    pub(crate) kind: FileChangeKindV1,
    pub(crate) current_path: String,
    pub(crate) current_source: CurrentSourceV1,
    pub(crate) baseline: Option<BaselineFileV1>,
}

#[allow(dead_code, reason = "staged native file-budget scope issue kinds")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ScopeIssueKindV1 {
    EmbeddedRepository,
    Gitlink,
    GitDiagnostic,
    IntentToAdd,
    MissingWorktreeEntry,
    NonUtf8Path,
    RenameLimit,
    Sparse,
    Special,
    Symlink,
    Unmerged,
    UnsupportedMode,
}

#[allow(dead_code, reason = "staged native file-budget scope issue")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ScopeIssueV1 {
    pub(crate) kind: ScopeIssueKindV1,
    pub(crate) path: Option<String>,
    pub(crate) message: String,
}

#[allow(dead_code, reason = "staged native file-budget measurable scope")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScopeSnapshotV1 {
    pub(crate) view: CurrentViewV1,
    pub(crate) entries: Vec<ScopeEntryV1>,
    pub(crate) complete: bool,
    pub(crate) issues: Vec<ScopeIssueV1>,
}

#[allow(dead_code, reason = "staged native file-budget measurable scope API")]
pub(crate) fn capture_scope_v1(
    root: &Path,
    comparison: &ResolvedComparisonV1,
    view: CurrentViewV1,
) -> Result<ScopeSnapshotV1> {
    capture_scope_inner(root, comparison, view, GitReceiptCollection::Blocking)
}

#[allow(dead_code, reason = "staged cancellable native file-budget scope API")]
pub(crate) fn capture_scope_v1_with_cancellation(
    root: &Path,
    comparison: &ResolvedComparisonV1,
    view: CurrentViewV1,
    cancelled: &dyn Fn() -> bool,
) -> Result<ScopeSnapshotV1> {
    capture_scope_inner(
        root,
        comparison,
        view,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

pub(crate) fn capture_all_current_scope_v1_with_cancellation(
    root: &Path,
    view: CurrentViewV1,
    include_untracked: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<ScopeSnapshotV1> {
    capture_inventory_scope(
        root,
        view,
        include_untracked,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

pub(crate) fn capture_affected_paths_v1(
    root: &Path,
    comparison: &ResolvedComparisonV1,
) -> Result<Vec<String>> {
    let baseline_oid = match comparison {
        ResolvedComparisonV1::MergeBase { merge_base_oid, .. }
        | ResolvedComparisonV1::ExactTree {
            tree_oid: merge_base_oid,
            ..
        } => merge_base_oid,
        ResolvedComparisonV1::IndexAgainstHead { .. }
        | ResolvedComparisonV1::StrictInventory { .. } => {
            bail!("affected path capture requires a worktree comparison")
        }
    };
    let collection = GitReceiptCollection::Blocking;
    let raw = raw_scope_diff(root, baseline_oid, false, collection)?;
    let status = worktree_status_output(root, collection)?;
    let mut paths = BTreeSet::new();
    for entry in parse_raw_diff_z(&raw.stdout, MAX_CHANGED_PATH_DISCOVERY_ENTRIES)? {
        paths.insert(affected_utf8_path(&entry.current_path)?);
        if let Some(path) = entry.baseline_path {
            paths.insert(affected_utf8_path(&path)?);
        }
    }
    for entry in parse_porcelain_status_z(&status.stdout)? {
        paths.insert(strict_git_path(entry.path)?);
        if let Some(path) = entry.original_path {
            paths.insert(strict_git_path(path)?);
        }
    }
    if paths.len() > MAX_CHANGED_PATH_DISCOVERY_ENTRIES {
        bail!(
            "affected path capture exceeded the combined limit of {MAX_CHANGED_PATH_DISCOVERY_ENTRIES} paths"
        );
    }
    Ok(paths.into_iter().collect())
}

fn capture_scope_inner(
    root: &Path,
    comparison: &ResolvedComparisonV1,
    view: CurrentViewV1,
    collection: GitReceiptCollection<'_>,
) -> Result<ScopeSnapshotV1> {
    collection.ensure_active()?;
    match (comparison, view) {
        (ResolvedComparisonV1::MergeBase { .. }, CurrentViewV1::Worktree)
        | (ResolvedComparisonV1::ExactTree { .. }, CurrentViewV1::Worktree) => {
            capture_changed_scope(
                root,
                comparison
                    .baseline_oid()
                    .expect("worktree comparisons have a baseline"),
                view,
                false,
                collection,
            )
        }
        (ResolvedComparisonV1::IndexAgainstHead { .. }, CurrentViewV1::Index) => {
            capture_changed_scope(
                root,
                comparison
                    .baseline_oid()
                    .expect("index comparisons have a baseline"),
                view,
                true,
                collection,
            )
        }
        (ResolvedComparisonV1::StrictInventory { .. }, CurrentViewV1::Inventory) => {
            capture_inventory_scope(root, CurrentViewV1::Inventory, true, collection)
        }
        _ => bail!("comparison strategy and current view are incompatible"),
    }
}

fn capture_changed_scope(
    root: &Path,
    baseline_oid: &str,
    view: CurrentViewV1,
    cached: bool,
    collection: GitReceiptCollection<'_>,
) -> Result<ScopeSnapshotV1> {
    let output = raw_scope_diff(root, baseline_oid, cached, collection)?;
    let mut issues = rename_diagnostics(&output.stderr);
    let (intent_to_add, intent_issues) = intent_to_add_paths(root, collection)?;
    issues.extend(intent_issues);
    let sparse = if view == CurrentViewV1::Worktree {
        let (sparse, sparse_issues) = sparse_index_paths(root, collection)?;
        issues.extend(sparse_issues);
        sparse
    } else {
        BTreeSet::new()
    };
    let mut entries = Vec::new();
    for raw in parse_raw_diff_z(&output.stdout, MAX_CHANGED_PATH_DISCOVERY_ENTRIES)? {
        append_raw_entry(
            root,
            view,
            raw,
            &intent_to_add,
            &sparse,
            &mut entries,
            &mut issues,
        )?;
    }
    if view == CurrentViewV1::Worktree {
        append_untracked_entries(root, collection, &mut entries, &mut issues)?;
    }
    finish_snapshot(view, entries, issues)
}

fn capture_inventory_scope(
    root: &Path,
    view: CurrentViewV1,
    include_untracked: bool,
    collection: GitReceiptCollection<'_>,
) -> Result<ScopeSnapshotV1> {
    let output = scope_git_output(
        root,
        &[
            "--no-replace-objects",
            "ls-files",
            "--stage",
            "-z",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git ls-files inventory",
        collection,
    )?;
    let mut issues = rename_diagnostics(&output.stderr);
    let sparse = if view == CurrentViewV1::Index {
        BTreeSet::new()
    } else {
        let (sparse, sparse_issues) = sparse_index_paths(root, collection)?;
        issues.extend(sparse_issues);
        sparse
    };
    let (intent_to_add, intent_issues) = intent_to_add_paths(root, collection)?;
    issues.extend(intent_issues);
    let mut entries = Vec::new();
    for entry in parse_index_stage_z(&output.stdout, MAX_CHANGED_PATH_DISCOVERY_ENTRIES)? {
        let Some(path) = utf8_path(&entry.path, &mut issues) else {
            continue;
        };
        if entry.stage != "0" {
            issues.push(scope_issue(
                ScopeIssueKindV1::Unmerged,
                Some(path),
                "tracked inventory path has an unmerged index stage",
            ));
            continue;
        }
        if intent_to_add.contains(&path) {
            issues.push(scope_issue(
                ScopeIssueKindV1::IntentToAdd,
                Some(path),
                "tracked inventory path is intent-to-add and has no stable index authority",
            ));
            continue;
        }
        if view != CurrentViewV1::Index && sparse.contains(&path) {
            issues.push(scope_issue(
                ScopeIssueKindV1::Sparse,
                Some(path),
                "tracked inventory path is sparse and has no worktree authority",
            ));
            continue;
        }
        if let Some(kind) = unsupported_mode(&entry.mode) {
            issues.push(scope_issue(
                kind,
                Some(path),
                "tracked inventory path is not a regular file",
            ));
            continue;
        }
        if view == CurrentViewV1::Index {
            entries.push(ScopeEntryV1 {
                kind: FileChangeKindV1::Unchanged,
                current_path: path,
                current_source: CurrentSourceV1::IndexBlob {
                    oid: entry.oid.clone(),
                },
                baseline: None,
            });
        } else {
            match inspect_worktree_path(root, &path)? {
                InspectedWorktreePath::Regular => entries.push(ScopeEntryV1 {
                    kind: FileChangeKindV1::Unchanged,
                    current_path: path,
                    current_source: CurrentSourceV1::WorktreePath,
                    baseline: None,
                }),
                InspectedWorktreePath::Missing => issues.push(scope_issue(
                    ScopeIssueKindV1::MissingWorktreeEntry,
                    Some(path),
                    "tracked inventory path is missing from the worktree",
                )),
                InspectedWorktreePath::Unsupported(kind) => issues.push(scope_issue(
                    kind,
                    Some(path),
                    "tracked inventory path is unsupported in the worktree",
                )),
            }
        }
    }
    if include_untracked && view != CurrentViewV1::Index {
        append_untracked_entries(root, collection, &mut entries, &mut issues)?;
    }
    finish_snapshot(view, entries, issues)
}

fn append_raw_entry(
    root: &Path,
    view: CurrentViewV1,
    raw: RawDiffEntry,
    intent_to_add: &BTreeSet<String>,
    sparse: &BTreeSet<String>,
    entries: &mut Vec<ScopeEntryV1>,
    issues: &mut Vec<ScopeIssueV1>,
) -> Result<()> {
    let Some(current_path) = utf8_path(&raw.current_path, issues) else {
        return Ok(());
    };
    let status = raw.status.as_bytes().first().copied().unwrap_or_default();
    if status == b'D' {
        if view == CurrentViewV1::Worktree {
            ensure_staged_deletion_has_no_worktree_replacement(root, &current_path)?;
        }
        return Ok(());
    }
    if status == b'U' || raw.status.len() > 1 && raw.status.contains('U') {
        issues.push(scope_issue(
            ScopeIssueKindV1::Unmerged,
            Some(current_path),
            "scope path has an unmerged Git status",
        ));
        return Ok(());
    }
    if intent_to_add.contains(&current_path) {
        issues.push(scope_issue(
            ScopeIssueKindV1::IntentToAdd,
            Some(current_path),
            "scope path is intent-to-add and has no stable index authority",
        ));
        return Ok(());
    }
    if sparse.contains(&current_path) {
        issues.push(scope_issue(
            ScopeIssueKindV1::Sparse,
            Some(current_path),
            "scope path is sparse and has no worktree authority",
        ));
        return Ok(());
    }
    if let Some(kind) = unsupported_mode(&raw.new_mode) {
        issues.push(scope_issue(
            kind,
            Some(current_path),
            "scope path is not a regular file",
        ));
        return Ok(());
    }
    let current_source = match view {
        CurrentViewV1::Worktree => match inspect_worktree_path(root, &current_path)? {
            InspectedWorktreePath::Regular => CurrentSourceV1::WorktreePath,
            InspectedWorktreePath::Missing => {
                issues.push(scope_issue(
                    ScopeIssueKindV1::MissingWorktreeEntry,
                    Some(current_path),
                    "changed tracked path is missing from the worktree",
                ));
                return Ok(());
            }
            InspectedWorktreePath::Unsupported(kind) => {
                issues.push(scope_issue(
                    kind,
                    Some(current_path),
                    "changed tracked path is unsupported in the worktree",
                ));
                return Ok(());
            }
        },
        CurrentViewV1::Index => CurrentSourceV1::IndexBlob {
            oid: raw.new_oid.clone(),
        },
        CurrentViewV1::Inventory => unreachable!("inventory uses index enumeration"),
    };
    let baseline_path = match status {
        b'M' | b'T' => Some(current_path.clone()),
        b'R' => {
            let Some(path) = raw.baseline_path.as_deref() else {
                issues.push(scope_issue(
                    ScopeIssueKindV1::UnsupportedMode,
                    Some(current_path),
                    "rename scope entry has no baseline path",
                ));
                return Ok(());
            };
            let Some(path) = utf8_path(path, issues) else {
                return Ok(());
            };
            Some(path)
        }
        b'A' | b'C' => None,
        _ => None,
    };
    let kind = match status {
        b'A' | b'C' => FileChangeKindV1::Added,
        b'M' => FileChangeKindV1::Modified,
        b'T' => FileChangeKindV1::TypeChanged,
        b'R' => FileChangeKindV1::Renamed,
        _ => {
            issues.push(scope_issue(
                ScopeIssueKindV1::UnsupportedMode,
                Some(current_path),
                "scope path has an unsupported Git change status",
            ));
            return Ok(());
        }
    };
    let baseline = baseline_path
        .filter(|_| unsupported_mode(&raw.old_mode).is_none())
        .map(|path| BaselineFileV1 {
            path,
            blob_oid: raw.old_oid,
        });
    entries.push(ScopeEntryV1 {
        kind,
        current_path,
        current_source,
        baseline,
    });
    Ok(())
}

fn append_untracked_entries(
    root: &Path,
    collection: GitReceiptCollection<'_>,
    entries: &mut Vec<ScopeEntryV1>,
    issues: &mut Vec<ScopeIssueV1>,
) -> Result<()> {
    let output = worktree_status_output(root, collection)?;
    issues.extend(rename_diagnostics(&output.stderr));
    for entry in parse_porcelain_status_z(&output.stdout)? {
        if entry.status != "??" {
            continue;
        }
        let Some(path) = utf8_os_path(&entry.path, issues) else {
            continue;
        };
        let inspection = inspect_worktree_path(root, &path)?;
        append_inspected_untracked_entry(path, inspection, entries, issues);
    }
    Ok(())
}

fn append_inspected_untracked_entry(
    path: String,
    inspection: InspectedWorktreePath,
    entries: &mut Vec<ScopeEntryV1>,
    issues: &mut Vec<ScopeIssueV1>,
) {
    match inspection {
        InspectedWorktreePath::Regular => entries.push(ScopeEntryV1 {
            kind: FileChangeKindV1::Untracked,
            current_path: path,
            current_source: CurrentSourceV1::WorktreePath,
            baseline: None,
        }),
        InspectedWorktreePath::Missing => {}
        InspectedWorktreePath::Unsupported(kind) => issues.push(scope_issue(
            kind,
            Some(path),
            "untracked scope path is unsupported",
        )),
    }
}

#[cfg(test)]
pub(super) fn append_disappeared_untracked_entry_for_test(
    entries: &mut Vec<ScopeEntryV1>,
    issues: &mut Vec<ScopeIssueV1>,
) {
    append_inspected_untracked_entry(
        "disappeared.txt".to_owned(),
        InspectedWorktreePath::Missing,
        entries,
        issues,
    );
}

fn worktree_status_output(root: &Path, collection: GitReceiptCollection<'_>) -> Result<Output> {
    scope_git_output(
        root,
        &[
            "--no-replace-objects",
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
        "git status untracked scope",
        collection,
    )
}

fn raw_scope_diff(
    root: &Path,
    baseline_oid: &str,
    cached: bool,
    collection: GitReceiptCollection<'_>,
) -> Result<Output> {
    let rename_limit = scope_rename_limit().to_string();
    let mut args = vec![
        "--no-replace-objects".to_string(),
        "-c".to_string(),
        "core.fileMode=true".to_string(),
        "-c".to_string(),
        "diff.ignoreSubmodules=none".to_string(),
        "-c".to_string(),
        "diff.renames=true".to_string(),
        "-c".to_string(),
        format!("diff.renameLimit={rename_limit}"),
        "diff".to_string(),
    ];
    if cached {
        args.push("--cached".to_string());
    }
    args.extend([
        "--raw".to_string(),
        "-z".to_string(),
        "--no-abbrev".to_string(),
        "--find-renames=50%".to_string(),
        "--no-ext-diff".to_string(),
        "--no-textconv".to_string(),
        "--ignore-submodules=none".to_string(),
        baseline_oid.to_string(),
        "--".to_string(),
        ".".to_string(),
        ":(exclude).agent/**".to_string(),
    ]);
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    scope_git_output(root, &refs, "git diff canonical scope", collection)
}

pub(super) fn intent_to_add_paths(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<(BTreeSet<String>, Vec<ScopeIssueV1>)> {
    let visible = diff_ita_paths(root, "--ita-visible-in-index", collection)?;
    let invisible = diff_ita_paths(root, "--ita-invisible-in-index", collection)?;
    let paths = visible
        .0
        .difference(&invisible.0)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut issues = visible.1;
    issues.extend(invisible.1);
    Ok((paths, issues))
}

fn diff_ita_paths(
    root: &Path,
    flag: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<(BTreeSet<String>, Vec<ScopeIssueV1>)> {
    let output = scope_git_output(
        root,
        &[
            "--no-replace-objects",
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            flag,
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff intent-to-add scope",
        collection,
    )?;
    let paths = parse_nul_utf8_paths(&output.stdout, "git diff intent-to-add")?
        .into_iter()
        .collect();
    Ok((paths, rename_diagnostics(&output.stderr)))
}

pub(super) fn sparse_index_paths(
    root: &Path,
    collection: GitReceiptCollection<'_>,
) -> Result<(BTreeSet<String>, Vec<ScopeIssueV1>)> {
    let output = scope_git_output(
        root,
        &[
            "--no-replace-objects",
            "ls-files",
            "-t",
            "-z",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git ls-files sparse scope",
        collection,
    )?;
    require_nul_terminated(&output.stdout, "git ls-files sparse scope -z")?;
    let mut sparse = BTreeSet::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        if record.len() < 3 || record[1] != b' ' {
            bail!("malformed git ls-files -t sparse record");
        }
        if record[0] == b'S' {
            let path =
                std::str::from_utf8(&record[2..]).context("sparse Git path was not UTF-8")?;
            sparse.insert(path.to_owned());
        }
    }
    Ok((sparse, rename_diagnostics(&output.stderr)))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn rename_diagnostics(stderr: &[u8]) -> Vec<ScopeIssueV1> {
    if stderr.is_empty() {
        return Vec::new();
    }
    let diagnostic = String::from_utf8_lossy(stderr);
    diagnostic
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let excerpt = bounded_git_diagnostic(line);
            if line.contains("rename detection was skipped") {
                scope_issue(
                    ScopeIssueKindV1::RenameLimit,
                    None,
                    format!(
                        "Git skipped exhaustive rename detection at the pinned limit: {excerpt}"
                    ),
                )
            } else {
                scope_issue(
                    ScopeIssueKindV1::GitDiagnostic,
                    None,
                    format!("Git emitted a diagnostic while capturing comparison scope: {excerpt}"),
                )
            }
        })
        .collect()
}

fn bounded_git_diagnostic(line: &str) -> String {
    let mut excerpt = line
        .chars()
        .take(MAX_SCOPE_GIT_DIAGNOSTIC_CHARS_V1)
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect::<String>();
    if line.chars().count() > MAX_SCOPE_GIT_DIAGNOSTIC_CHARS_V1 {
        excerpt.push('…');
    }
    excerpt
}

pub(super) fn unsupported_mode(mode: &str) -> Option<ScopeIssueKindV1> {
    match mode {
        "100644" | "100755" => None,
        "120000" => Some(ScopeIssueKindV1::Symlink),
        "160000" => Some(ScopeIssueKindV1::Gitlink),
        "000000" => None,
        _ => Some(ScopeIssueKindV1::UnsupportedMode),
    }
}

fn finish_snapshot(
    view: CurrentViewV1,
    mut entries: Vec<ScopeEntryV1>,
    mut issues: Vec<ScopeIssueV1>,
) -> Result<ScopeSnapshotV1> {
    entries.sort();
    entries.dedup();
    if entries.len() > MAX_CHANGED_PATH_DISCOVERY_ENTRIES {
        bail!(
            "scope contains more than {MAX_CHANGED_PATH_DISCOVERY_ENTRIES} entries; reduce the selected repository view"
        );
    }
    issues.sort();
    issues.dedup();
    Ok(ScopeSnapshotV1 {
        view,
        complete: issues.is_empty(),
        entries,
        issues,
    })
}

fn utf8_path(bytes: &[u8], issues: &mut Vec<ScopeIssueV1>) -> Option<String> {
    match std::str::from_utf8(bytes) {
        Ok(path) => Some(path.to_owned()),
        Err(_) => {
            issues.push(scope_issue(
                ScopeIssueKindV1::NonUtf8Path,
                None,
                "Git scope contains a non-UTF-8 path",
            ));
            None
        }
    }
}

fn affected_utf8_path(bytes: &[u8]) -> Result<String> {
    std::str::from_utf8(bytes)
        .context("Affected selection requires UTF-8 Git paths")
        .map(str::to_owned)
}

fn utf8_os_path(path: &Path, issues: &mut Vec<ScopeIssueV1>) -> Option<String> {
    match std::str::from_utf8(path.as_os_str().as_encoded_bytes()) {
        Ok(path) => Some(path.to_owned()),
        Err(_) => {
            issues.push(scope_issue(
                ScopeIssueKindV1::NonUtf8Path,
                None,
                "Git scope contains a non-UTF-8 path",
            ));
            None
        }
    }
}

fn scope_issue(
    kind: ScopeIssueKindV1,
    path: Option<String>,
    message: impl Into<String>,
) -> ScopeIssueV1 {
    ScopeIssueV1 {
        kind,
        path,
        message: message.into(),
    }
}

include!("change_scope/tail.rs");
