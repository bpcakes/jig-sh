use super::*;

#[cfg(test)]
pub fn repo_worktree_fingerprint(root: &Path) -> Result<String> {
    repo_worktree_fingerprint_inner(root, GitCollection::Blocking)
}

#[cfg(test)]
pub fn repo_worktree_fingerprint_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    repo_worktree_fingerprint_inner(root, GitCollection::Cancellable(cancelled))
}

pub fn is_git_collection_cancellation(error: &anyhow::Error) -> bool {
    error.is::<GitCollectionCancelled>()
}
