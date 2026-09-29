#[cfg(test)]
pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    record_receipt_inner(ctx, input, None, ReceiptPublication::Finalize)
}

/// Cancellation stops optional enrichment; publication still gets a bounded
/// chance to persist the outcome, including the cancellation itself.
pub(crate) fn record_receipt_with_cancellation(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    record_receipt_inner(
        ctx,
        input,
        Some(cancelled),
        ReceiptPublication::Finalize,
    )
}

/// For rollback-sensitive operations, cancellation aborts publication too.
pub(crate) fn record_receipt_with_cancellation_until(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    cancelled: &dyn Fn() -> bool,
    deadline: std::time::Instant,
) -> Result<String> {
    record_receipt_inner(
        ctx,
        input,
        Some(cancelled),
        ReceiptPublication::Cancellable {
            deadline,
            cancelled,
        },
    )
}

enum ReceiptPublication<'a> {
    Finalize,
    Cancellable {
        deadline: std::time::Instant,
        cancelled: &'a dyn Fn() -> bool,
    },
}

impl ReceiptPublication<'_> {
    fn lock_budget(&self) -> (std::time::Instant, &dyn Fn() -> bool) {
        match self {
            Self::Finalize => (
                std::time::Instant::now() + journal::RECEIPT_LOCK_TIMEOUT,
                &|| false,
            ),
            Self::Cancellable {
                deadline,
                cancelled,
            } => (*deadline, *cancelled),
        }
    }
}

fn record_receipt_inner(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    enrichment_cancelled: Option<&dyn Fn() -> bool>,
    publication: ReceiptPublication<'_>,
) -> Result<String> {
    let git_metadata = receipt_git_metadata(
        ctx,
        input.collect_git_metadata,
        input.collect_worktree_fingerprint,
        enrichment_cancelled,
    );
    let root_spellings = repository_root_spellings(ctx.root());
    // All publication locks share one budget. Ordinary finalization starts it
    // after optional enrichment; transactions retain their caller's deadline.
    let (deadline, lock_cancelled) = publication.lock_budget();
    let receipt = ReceiptRecord {
        id: new_id("receipt"),
        // Work sessions were removed; the field remains for existing records.
        session_id: None,
        plan_id: None,
        tool_name: input.tool_name.to_string(),
        args: redact_repository_root_in_value(input.args, &root_spellings),
        invoked_command_key: input.invoked_command_key,
        started_at_ms: input.started_at_ms,
        ended_at_ms: input.ended_at_ms,
        exit_status: input.exit_status,
        stdout_preview: receipt_output_preview(
            &redact_repository_root(input.stdout, &root_spellings),
            input.exit_status,
        ),
        stderr_preview: receipt_output_preview(
            &redact_repository_root(input.stderr, &root_spellings),
            input.exit_status,
        ),
        evidence: input
            .evidence
            .map(|value| redact_repository_root_in_value(value, &root_spellings)),
        run_id: None,
        changed_paths: git_metadata.changed_paths,
        changed_path_count: git_metadata.changed_path_count,
        changed_paths_truncated: git_metadata.changed_paths_truncated,
        changed_paths_digest: git_metadata.changed_paths_digest,
        diff_stat: git_metadata.diff_stat,
        git_status_error: git_metadata
            .git_status_error
            .map(|value| redact_repository_root(&value, &root_spellings)),
        git_diff_stat_error: git_metadata
            .git_diff_stat_error
            .map(|value| redact_repository_root(&value, &root_spellings)),
        worktree_fingerprint: git_metadata.worktree_fingerprint,
        worktree_fingerprint_error: git_metadata
            .worktree_fingerprint_error
            .map(|value| redact_repository_root(&value, &root_spellings)),
    };
    let receipt_id = receipt.id.clone();
    with_receipt_journal_writer_until(ctx, deadline, lock_cancelled, |writer| {
        writer.append(&receipt)
    })?;
    Ok(receipt_id)
}

fn receipt_output_preview(value: &str, exit_status: i32) -> String {
    if exit_status != 0 {
        return truncate(value);
    }
    truncate_to_bytes(value, SUCCESSFUL_RECEIPT_PREVIEW_BYTES)
}

fn truncate_to_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn receipt_git_metadata(
    ctx: &RepoContext,
    collect_git_metadata: bool,
    collect_worktree_fingerprint: bool,
    cancelled: Option<&dyn Fn() -> bool>,
) -> GitReceiptMetadata {
    if !collect_git_metadata {
        return GitReceiptMetadata::default();
    }

    match (collect_worktree_fingerprint, cancelled) {
        (true, Some(cancelled)) => {
            collect_git_receipt_metadata_with_cancellation(ctx.root(), cancelled)
        }
        (false, Some(cancelled)) => {
            collect_git_receipt_metadata_without_worktree_fingerprint_with_cancellation(
                ctx.root(),
                cancelled,
            )
        }
        (true, None) => collect_git_receipt_metadata(ctx.root()),
        (false, None) => collect_git_receipt_metadata_without_worktree_fingerprint(ctx.root()),
    }
}
