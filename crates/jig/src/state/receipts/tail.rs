pub(crate) fn record_target_receipt(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    target: TargetReceiptMetadata,
) -> Result<String> {
    record_receipt_inner(ctx, input, Some(target), None, ReceiptPublication::Finalize)
}

#[cfg(test)]
pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    record_receipt_inner(ctx, input, None, None, ReceiptPublication::Finalize)
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
        None,
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
        None,
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
    target: Option<TargetReceiptMetadata>,
    enrichment_cancelled: Option<&dyn Fn() -> bool>,
    publication: ReceiptPublication<'_>,
) -> Result<String> {
    let mut git_metadata = receipt_git_metadata(
        ctx,
        input.collect_git_metadata,
        input.collect_worktree_fingerprint,
        enrichment_cancelled,
    );
    if let Some(override_result) = input.worktree_fingerprint_override {
        match override_result {
            Ok(fingerprint) => {
                git_metadata.worktree_fingerprint = Some(fingerprint);
                git_metadata.worktree_fingerprint_error = None;
            }
            Err(error) => {
                git_metadata.worktree_fingerprint = None;
                git_metadata.worktree_fingerprint_error = Some(error);
            }
        }
    }
    let evidence_valid_until_ms = input
        .evidence
        .as_ref()
        .and_then(|evidence| evidence.get("valid_until_ms"))
        .and_then(Value::as_u64);
    let target_freshness = target
        .as_ref()
        .and_then(|metadata| metadata.target_freshness.clone());
    let (
        run_id,
        target_id,
        config_digest,
        input_digest,
        findings,
        finding_count,
        findings_truncated,
        findings_digest,
        evaluated_at_ms,
        target_valid_until_ms,
    ) = target.map_or_else(
        || {
            (
                None,
                None,
                None,
                None,
                Vec::new(),
                None,
                false,
                None,
                None,
                None,
            )
        },
        |metadata| {
            (
                Some(metadata.run_id),
                Some(metadata.target),
                Some(metadata.config_digest),
                Some(metadata.input_digest),
                metadata.findings,
                metadata.finding_count,
                metadata.findings_truncated,
                metadata.findings_digest,
                metadata.evaluated_at_ms,
                metadata.valid_until_ms,
            )
        },
    );
    let root_spellings = repository_root_spellings(ctx.root());
    // All publication locks share one budget. Ordinary finalization starts it
    // after optional enrichment; transactions retain their caller's deadline.
    let (deadline, lock_cancelled) = publication.lock_budget();
    let receipt = ReceiptRecord {
        target_freshness,
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
        run_id,
        target: target_id,
        config_digest,
        input_digest,
        findings,
        finding_count,
        findings_truncated,
        findings_digest,
        evaluated_at_ms,
        valid_until_ms: target_valid_until_ms.or(evidence_valid_until_ms),
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

fn tool_receipt_status(receipt: &ReceiptRecord) -> ToolReceiptStatus {
    let diff_summary = receipt_diff_summary(receipt);
    let changed_path_count = receipt
        .changed_path_count
        .unwrap_or(receipt.changed_paths.len());
    let changed_paths_truncated =
        receipt.changed_paths_truncated || changed_path_count > receipt.changed_paths.len();
    ToolReceiptStatus {
        receipt_id: receipt.id.clone(),
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        changed_paths: receipt.changed_paths.clone(),
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest: receipt.changed_paths_digest.clone(),
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint.clone(),
        worktree_fingerprint_error: receipt.worktree_fingerprint_error.clone(),
        valid_until_ms: receipt.valid_until_ms,
        requires_time_validity: receipt
            .evidence
            .as_ref()
            .is_some_and(evidence_requires_time_validity),
    }
}

fn target_receipt_status(receipt: &ReceiptRecord, target: &TargetId) -> TargetReceiptStatus {
    let tool = tool_receipt_status(receipt);
    TargetReceiptStatus {
        receipt_id: tool.receipt_id,
        run_id: receipt.run_id.clone(),
        plan_id: receipt.plan_id.clone(),
        target_freshness: receipt.target_freshness.clone(),
        target: target.clone(),
        config_digest: receipt.config_digest.clone(),
        input_digest: receipt.input_digest.clone(),
        exit_status: tool.exit_status,
        started_at_ms: receipt.started_at_ms,
        ended_at_ms: tool.ended_at_ms,
        changed_paths: tool.changed_paths,
        changed_path_count: tool.changed_path_count,
        changed_paths_truncated: tool.changed_paths_truncated,
        changed_paths_digest: tool.changed_paths_digest,
        diff_summary: tool.diff_summary,
        worktree_fingerprint: tool.worktree_fingerprint,
        worktree_fingerprint_error: tool.worktree_fingerprint_error,
        valid_until_ms: tool.valid_until_ms,
        requires_time_validity: tool.requires_time_validity,
    }
}

pub(crate) fn evidence_requires_time_validity(evidence: &Value) -> bool {
    evidence
        .get("requires_time_validity")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || evidence
            .get("file_budget")
            .and_then(|value| value.get("active_waiver_count"))
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0)
}

pub(crate) fn receipt_diff_summary(receipt: &ReceiptRecord) -> String {
    if receipt.git_status_error.is_some() || receipt.git_diff_stat_error.is_some() {
        return "git metadata unavailable".to_string();
    }

    let stat = &receipt.diff_stat;
    if stat.files == 0 && stat.insertions == 0 && stat.deletions == 0 {
        "no changes".to_string()
    } else {
        let file_count = if stat.files == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", stat.files)
        };
        format!("{file_count}, +{} -{}", stat.insertions, stat.deletions)
    }
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
