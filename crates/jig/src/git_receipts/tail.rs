use super::*;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct BoundedChangedPaths {
    pub(super) preview: Vec<String>,
    pub(super) total: usize,
    pub(super) truncated: bool,
    pub(super) digest: String,
}

pub(super) fn bounded_changed_paths(mut paths: Vec<String>) -> BoundedChangedPaths {
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

pub(super) fn changed_paths_digest(paths: &[String]) -> String {
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
