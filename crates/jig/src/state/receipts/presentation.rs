use super::*;

pub(super) fn receipt_list_value(receipt: ReceiptRecord) -> Result<Value> {
    let diff_summary = receipt_diff_summary(&receipt);
    let mut value = serde_json::to_value(receipt)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("diff_summary".to_string(), Value::String(diff_summary));
    }
    Ok(value)
}

pub(super) fn tool_receipt_status(receipt: &ReceiptRecord) -> ToolReceiptStatus {
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
    }
}

pub(super) fn target_receipt_status(
    receipt: &ReceiptRecord,
    run_id: &str,
    target: &TargetId,
) -> TargetReceiptStatus {
    let tool = tool_receipt_status(receipt);
    TargetReceiptStatus {
        receipt_id: tool.receipt_id,
        run_id: run_id.to_owned(),
        target: target.clone(),
        config_digest: receipt.config_digest.clone(),
        input_digest: receipt.input_digest.clone(),
        exit_status: tool.exit_status,
        ended_at_ms: tool.ended_at_ms,
        changed_paths: tool.changed_paths,
        changed_path_count: tool.changed_path_count,
        changed_paths_truncated: tool.changed_paths_truncated,
        changed_paths_digest: tool.changed_paths_digest,
        diff_summary: tool.diff_summary,
        worktree_fingerprint: tool.worktree_fingerprint,
        worktree_fingerprint_error: tool.worktree_fingerprint_error,
    }
}

pub(super) fn work_review_receipt_status(receipt: &ReceiptRecord) -> WorkReviewReceiptStatus {
    let diff_summary = receipt_diff_summary(receipt);
    let changed_path_count = receipt
        .changed_path_count
        .unwrap_or(receipt.changed_paths.len());
    let changed_paths_truncated =
        receipt.changed_paths_truncated || changed_path_count > receipt.changed_paths.len();
    WorkReviewReceiptStatus {
        receipt_id: receipt.id.clone(),
        exit_status: receipt.exit_status,
        ended_at_ms: receipt.ended_at_ms,
        evidence: receipt.evidence.as_ref().map(work_review_receipt_evidence),
        changed_paths: receipt.changed_paths.clone(),
        changed_path_count,
        changed_paths_truncated,
        changed_paths_digest: receipt.changed_paths_digest.clone(),
        diff_summary,
        worktree_fingerprint: receipt.worktree_fingerprint.clone(),
        worktree_fingerprint_error: receipt.worktree_fingerprint_error.clone(),
    }
}

pub(super) fn work_review_receipt_evidence(evidence: &Value) -> WorkReviewReceiptEvidence {
    let retained_finding_count = evidence["findings"].as_array().map(Vec::len);
    let retained_actionable_count = evidence["actionable_findings"].as_array().map(Vec::len);
    let mut parse_error = evidence["parse_error"].as_str().map(str::to_string);
    if parse_error.is_none() && evidence["status"].as_str().is_none() {
        parse_error = Some("review evidence is missing status".into());
    }
    if parse_error.is_none()
        && evidence.get("findings").is_some()
        && retained_finding_count.is_none()
    {
        parse_error = Some("review evidence findings is not an array".into());
    }
    if parse_error.is_none()
        && evidence.get("actionable_findings").is_some()
        && retained_actionable_count.is_none()
    {
        parse_error = Some("review evidence actionable_findings is not an array".into());
    }
    WorkReviewReceiptEvidence {
        status: evidence["status"].as_str().map(str::to_string),
        finding_count: evidence["raw_finding_count"]
            .as_u64()
            .or_else(|| retained_finding_count.map(|count| count as u64)),
        actionable_count: evidence["raw_actionable_count"]
            .as_u64()
            .or_else(|| retained_actionable_count.map(|count| count as u64)),
        retained_finding_count,
        retained_actionable_count,
        findings_truncated: evidence["findings_truncated"].as_bool(),
        actionable_findings_truncated: evidence["actionable_findings_truncated"].as_bool(),
        threshold: evidence["threshold"].as_str().map(str::to_string),
        parse: parse_error.map_or(
            WorkReviewEvidenceParse::Valid,
            WorkReviewEvidenceParse::Invalid,
        ),
    }
}

pub(in crate::state) fn receipt_diff_summary(receipt: &ReceiptRecord) -> String {
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

pub(super) fn receipt_output_preview(value: &str, exit_status: i32) -> String {
    if exit_status != 0 {
        return truncate(value);
    }
    truncate_to_bytes(value, SUCCESSFUL_RECEIPT_PREVIEW_BYTES)
}

pub(super) fn truncate_to_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

pub(super) fn receipt_git_metadata(
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
