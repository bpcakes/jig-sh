use super::*;

use super::change_scope::parse::{IndexStageEntry, parse_index_stage_z};
use super::change_scope::{intent_to_add_paths, sparse_index_paths, unsupported_mode};

#[allow(dead_code, reason = "staged native file-budget exact-path bound")]
pub(crate) const MAX_EXACT_CURRENT_PATH_FACTS_V1: usize = 8_192;
#[allow(dead_code, reason = "staged native file-budget exact-path bound")]
pub(crate) const MAX_EXACT_CURRENT_PATH_BYTES_V1: usize = 4_096;

#[allow(dead_code, reason = "staged native file-budget exact-path states")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ExactCurrentPathStateV1 {
    Regular,
    Missing,
    Unsupported { reason: ScopeIssueKindV1 },
}

#[allow(dead_code, reason = "staged native file-budget exact-path fact")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ExactCurrentPathFactV1 {
    pub(crate) path: String,
    pub(crate) state: ExactCurrentPathStateV1,
}

#[allow(dead_code, reason = "staged native file-budget worktree inspection")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InspectedWorktreePath {
    Regular,
    Missing,
    Unsupported(ScopeIssueKindV1),
}

#[allow(dead_code, reason = "staged native file-budget exact-path API")]
pub(crate) fn observe_exact_paths_v1(
    root: &Path,
    view: CurrentViewV1,
    paths: &[String],
) -> Result<Vec<ExactCurrentPathFactV1>> {
    observe_exact_paths_inner(root, view, paths, GitReceiptCollection::Blocking)
}

#[allow(dead_code, reason = "staged cancellable native exact-path API")]
pub(crate) fn observe_exact_paths_v1_with_cancellation(
    root: &Path,
    view: CurrentViewV1,
    paths: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<ExactCurrentPathFactV1>> {
    observe_exact_paths_inner(
        root,
        view,
        paths,
        GitReceiptCollection::Cancellable(cancelled),
    )
}

fn observe_exact_paths_inner(
    root: &Path,
    view: CurrentViewV1,
    paths: &[String],
    collection: GitReceiptCollection<'_>,
) -> Result<Vec<ExactCurrentPathFactV1>> {
    let paths = validate_exact_paths(paths)?;
    collection.ensure_active()?;
    let index = index_entries_for_paths(root, &paths, collection)?;
    let intent_to_add = require_clean_scope_probe(
        intent_to_add_paths(root, collection)?,
        "intent-to-add observation",
    )?;
    let sparse = if matches!(view, CurrentViewV1::Worktree | CurrentViewV1::Inventory) {
        require_clean_scope_probe(
            sparse_index_paths(root, collection)?,
            "sparse-index observation",
        )?
    } else {
        BTreeSet::new()
    };
    let mut facts = Vec::with_capacity(paths.len());
    for path in paths {
        collection.ensure_active()?;
        let entries = index.get(path).map(Vec::as_slice).unwrap_or(&[]);
        let state = match view {
            CurrentViewV1::Index => observe_index_path(path, entries, &intent_to_add),
            CurrentViewV1::Inventory => {
                if entries.is_empty() {
                    map_inspected_worktree_path(inspect_worktree_path(root, path)?)
                } else {
                    observe_tracked_worktree_path(root, path, entries, &intent_to_add, &sparse)?
                }
            }
            CurrentViewV1::Worktree => {
                if entries.len() > 1 || entries.iter().any(|entry| entry.stage != "0") {
                    ExactCurrentPathStateV1::Unsupported {
                        reason: ScopeIssueKindV1::Unmerged,
                    }
                } else if intent_to_add.contains(path) {
                    ExactCurrentPathStateV1::Unsupported {
                        reason: ScopeIssueKindV1::IntentToAdd,
                    }
                } else if sparse.contains(path) {
                    ExactCurrentPathStateV1::Unsupported {
                        reason: ScopeIssueKindV1::Sparse,
                    }
                } else if let Some(entry) = entries.first()
                    && let Some(reason) = unsupported_mode(&entry.mode)
                {
                    ExactCurrentPathStateV1::Unsupported { reason }
                } else {
                    map_inspected_worktree_path(inspect_worktree_path(root, path)?)
                }
            }
        };
        facts.push(ExactCurrentPathFactV1 {
            path: path.to_owned(),
            state,
        });
    }
    Ok(facts)
}

fn require_clean_scope_probe<T>(
    (value, diagnostics): (T, Vec<ScopeIssueV1>),
    label: &str,
) -> Result<T> {
    if diagnostics.is_empty() {
        Ok(value)
    } else {
        bail!(
            "Git emitted diagnostics during {label}; exact-path observation is incomplete: {}",
            diagnostics[0].message
        )
    }
}

fn observe_index_path(
    path: &str,
    entries: &[IndexStageEntry],
    intent_to_add: &BTreeSet<String>,
) -> ExactCurrentPathStateV1 {
    if entries.is_empty() {
        return ExactCurrentPathStateV1::Missing;
    }
    if entries.len() > 1 || entries.iter().any(|entry| entry.stage != "0") {
        return ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::Unmerged,
        };
    }
    if intent_to_add.contains(path) {
        return ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::IntentToAdd,
        };
    }
    unsupported_mode(&entries[0].mode).map_or(ExactCurrentPathStateV1::Regular, |reason| {
        ExactCurrentPathStateV1::Unsupported { reason }
    })
}

fn observe_tracked_worktree_path(
    root: &Path,
    path: &str,
    entries: &[IndexStageEntry],
    intent_to_add: &BTreeSet<String>,
    sparse: &BTreeSet<String>,
) -> Result<ExactCurrentPathStateV1> {
    if entries.len() > 1 || entries.iter().any(|entry| entry.stage != "0") {
        return Ok(ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::Unmerged,
        });
    }
    if intent_to_add.contains(path) {
        return Ok(ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::IntentToAdd,
        });
    }
    if sparse.contains(path) {
        return Ok(ExactCurrentPathStateV1::Unsupported {
            reason: ScopeIssueKindV1::Sparse,
        });
    }
    if let Some(reason) = unsupported_mode(&entries[0].mode) {
        return Ok(ExactCurrentPathStateV1::Unsupported { reason });
    }
    Ok(map_inspected_worktree_path(inspect_worktree_path(
        root, path,
    )?))
}

fn map_inspected_worktree_path(path: InspectedWorktreePath) -> ExactCurrentPathStateV1 {
    match path {
        InspectedWorktreePath::Regular => ExactCurrentPathStateV1::Regular,
        InspectedWorktreePath::Missing => ExactCurrentPathStateV1::Missing,
        InspectedWorktreePath::Unsupported(reason) => {
            ExactCurrentPathStateV1::Unsupported { reason }
        }
    }
}

pub(super) fn inspect_worktree_path(root: &Path, path: &str) -> Result<InspectedWorktreePath> {
    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("failed to inspect repository root {}", root.display()))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        bail!("repository root is not a real directory");
    }
    let parts = path.split('/').collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(InspectedWorktreePath::Missing);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect exact path component {}",
                        current.display()
                    )
                });
            }
        };
        let leaf = index + 1 == parts.len();
        if metadata.file_type().is_symlink() {
            return Ok(InspectedWorktreePath::Unsupported(
                ScopeIssueKindV1::Symlink,
            ));
        }
        if !leaf {
            if !metadata.is_dir() {
                return Ok(InspectedWorktreePath::Unsupported(
                    ScopeIssueKindV1::Special,
                ));
            }
            continue;
        }
        if metadata.is_file() {
            return Ok(InspectedWorktreePath::Regular);
        }
        return Ok(InspectedWorktreePath::Unsupported(if metadata.is_dir() {
            ScopeIssueKindV1::EmbeddedRepository
        } else {
            ScopeIssueKindV1::Special
        }));
    }
    unreachable!("exact paths are validated as non-empty")
}

fn index_entries_for_paths(
    root: &Path,
    paths: &[&str],
    collection: GitReceiptCollection<'_>,
) -> Result<BTreeMap<String, Vec<IndexStageEntry>>> {
    let mut by_path = BTreeMap::<String, Vec<IndexStageEntry>>::new();
    for chunk in independent_exact_path_chunks(paths) {
        let mut args = vec![
            "--no-replace-objects".to_string(),
            "ls-files".to_string(),
            "--stage".to_string(),
            "-z".to_string(),
            "--".to_string(),
        ];
        args.extend(chunk.iter().map(|path| format!(":(top,literal){path}")));
        args.extend(
            chunk
                .iter()
                .map(|path| format!(":(exclude,top,literal){path}/")),
        );
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = collection.git_bounded_output(
            root,
            &refs,
            "git ls-files exact paths",
            gate_scope_diff_output_limit(),
            "exact-path",
        )?;
        let requested = chunk.iter().copied().collect::<BTreeSet<_>>();
        for entry in parse_index_stage_z(
            &output.stdout,
            MAX_EXACT_CURRENT_PATH_FACTS_V1.saturating_mul(4),
        )? {
            let path = std::str::from_utf8(&entry.path)
                .context("exact index path was not UTF-8")?
                .to_owned();
            if !requested.contains(path.as_str()) {
                bail!("git ls-files exact-path query returned unrequested path `{path}`");
            }
            by_path.entry(path).or_default().push(entry);
        }
    }
    Ok(by_path)
}

fn independent_exact_path_chunks<'a>(paths: &'a [&'a str]) -> Vec<Vec<&'a str>> {
    let mut chunks = Vec::<Vec<&str>>::new();
    for path in paths {
        let encoded_bytes = exact_pathspec_bytes(path);
        let destination = chunks.iter_mut().find(|chunk| {
            chunk.len() < MAX_GIT_LITERAL_PATHS_PER_DIFF
                && chunk
                    .iter()
                    .map(|existing| exact_pathspec_bytes(existing))
                    .sum::<usize>()
                    .saturating_add(encoded_bytes)
                    <= MAX_GIT_LITERAL_PATHSPEC_BYTES_PER_DIFF
                && chunk
                    .iter()
                    .all(|existing| !paths_overlap_by_prefix(existing, path))
        });
        if let Some(chunk) = destination {
            chunk.push(path);
        } else {
            chunks.push(vec![path]);
        }
    }
    chunks
}

fn exact_pathspec_bytes(path: &str) -> usize {
    ":(top,literal)"
        .len()
        .saturating_add(path.len())
        .saturating_add(1)
        .saturating_add(":(exclude,top,literal)".len())
        .saturating_add(path.len())
        .saturating_add(2)
}

fn paths_overlap_by_prefix(left: &str, right: &str) -> bool {
    path_is_ancestor(left, right) || path_is_ancestor(right, left)
}

fn path_is_ancestor(ancestor: &str, descendant: &str) -> bool {
    descendant
        .strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_exact_paths(paths: &[String]) -> Result<Vec<&str>> {
    if paths.len() > MAX_EXACT_CURRENT_PATH_FACTS_V1 {
        bail!(
            "exact-path observation received {} paths; version 1 permits at most {MAX_EXACT_CURRENT_PATH_FACTS_V1}",
            paths.len()
        );
    }
    let mut normalized = BTreeSet::new();
    for path in paths {
        validate_exact_path(path)?;
        if !normalized.insert(path.as_str()) {
            bail!("exact-path observation contains duplicate path `{path}`");
        }
    }
    Ok(normalized.into_iter().collect())
}

fn validate_exact_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("exact path must not be empty");
    }
    if path.len() > MAX_EXACT_CURRENT_PATH_BYTES_V1 {
        bail!("exact path exceeds the version 1 UTF-8 byte limit");
    }
    if path.starts_with('/') || path.split('/').any(has_windows_drive_prefix) {
        bail!("exact path must be repository-relative");
    }
    if path.contains('\0') {
        bail!("exact path must not contain a NUL byte");
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        bail!("exact path contains an empty, dot, or traversal component");
    }
    if matches!(path.split('/').next(), Some(".agent" | ".git")) {
        bail!("exact path targets protected repository authority");
    }
    Ok(())
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
