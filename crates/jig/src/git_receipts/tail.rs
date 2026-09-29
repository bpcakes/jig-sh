use super::*;

#[cfg(test)]
pub(crate) fn repo_worktree_fingerprint(root: &Path) -> Result<String> {
    repo_worktree_fingerprint_inner(root, GitReceiptCollection::Blocking)
}

#[cfg(test)]
pub(crate) fn repo_worktree_fingerprint_with_cancellation(
    root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    repo_worktree_fingerprint_inner(root, GitReceiptCollection::Cancellable(cancelled))
}

pub(crate) fn is_git_receipt_collection_cancellation(error: &anyhow::Error) -> bool {
    error.is::<GitReceiptCollectionCancelled>()
}
