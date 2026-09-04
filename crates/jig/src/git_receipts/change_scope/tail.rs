fn scope_git_output(
    root: &Path,
    args: &[&str],
    label: &str,
    collection: GitReceiptCollection<'_>,
) -> Result<Output> {
    collection.git_bounded_output(
        root,
        args,
        label,
        gate_scope_diff_output_limit(),
        "comparison-scope",
    )
}

fn scope_rename_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = SCOPE_RENAME_LIMIT_OVERRIDE.get() {
        return limit;
    }
    MAX_SCOPE_RENAME_CANDIDATES_V1
}

#[cfg(test)]
pub(super) fn with_scope_rename_limit<T>(limit: usize, operation: impl FnOnce() -> T) -> T {
    SCOPE_RENAME_LIMIT_OVERRIDE.with(|override_limit| {
        let previous = override_limit.replace(Some(limit));
        let result = operation();
        override_limit.set(previous);
        result
    })
}
