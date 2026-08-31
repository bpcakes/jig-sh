fn receipt_arg_strings<'a>(receipt: &'a ReceiptRecord, key: &str) -> impl Iterator<Item = &'a str> {
    receipt
        .args
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn ensure_receipt_scan_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

pub(crate) fn current_worktree_fingerprint(ctx: &RepoContext) -> CurrentWorktreeFingerprint {
    let result = if ctx.contract_version() >= 6 {
        repository_source_snapshot(ctx.root()).map(|snapshot| snapshot.worktree_fingerprint)
    } else {
        repo_worktree_fingerprint(ctx.root())
    };
    current_worktree_fingerprint_from_result(result)
        .expect("blocking worktree fingerprint collection cannot be cancelled")
}

pub(crate) fn current_worktree_fingerprint_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<CurrentWorktreeFingerprint> {
    let result = if ctx.contract_version() >= 6 {
        repository_source_snapshot_with_cancellation(ctx.root(), cancelled)
            .map(|snapshot| snapshot.worktree_fingerprint)
    } else {
        repo_worktree_fingerprint_with_cancellation(ctx.root(), cancelled)
    };
    current_worktree_fingerprint_from_result(result)
}

pub(crate) fn current_worktree_fingerprint_for_receipt_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> CurrentWorktreeFingerprint {
    let result = if ctx.contract_version() >= 6 {
        repository_source_snapshot_with_cancellation(ctx.root(), cancelled)
            .map(|snapshot| snapshot.worktree_fingerprint)
    } else {
        repo_worktree_fingerprint_with_cancellation(ctx.root(), cancelled)
    };
    current_worktree_fingerprint_from_result_for_receipt(result)
}

fn current_worktree_fingerprint_from_result(
    result: Result<String>,
) -> Result<CurrentWorktreeFingerprint> {
    match result {
        Ok(fingerprint) => Ok(CurrentWorktreeFingerprint {
            fingerprint: Some(fingerprint),
            error: None,
        }),
        Err(error) if is_git_receipt_collection_cancellation(&error) => {
            Err(status_collection_cancellation())
        }
        Err(error) => Ok(CurrentWorktreeFingerprint {
            fingerprint: None,
            error: Some(format!("{error:#}")),
        }),
    }
}

fn current_worktree_fingerprint_from_result_for_receipt(
    result: Result<String>,
) -> CurrentWorktreeFingerprint {
    match result {
        Ok(fingerprint) => CurrentWorktreeFingerprint {
            fingerprint: Some(fingerprint),
            error: None,
        },
        Err(error) => CurrentWorktreeFingerprint {
            fingerprint: None,
            error: Some(format!("{error:#}")),
        },
    }
}

pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    record_receipt_inner(ctx, input, None, None)
}

pub(crate) fn record_target_receipt(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    target: TargetReceiptMetadata,
) -> Result<String> {
    record_receipt_inner(ctx, input, Some(target), None)
}

pub(crate) fn record_receipt_with_cancellation(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    record_receipt_inner(ctx, input, None, Some(cancelled))
}

fn record_receipt_inner(
    ctx: &RepoContext,
    input: ReceiptInput<'_>,
    target: Option<TargetReceiptMetadata>,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<String> {
    let mut git_metadata = receipt_git_metadata(
        ctx,
        input.collect_git_metadata,
        input.collect_worktree_fingerprint,
        cancelled,
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
    let (run_id, target_id, config_digest, input_digest, findings) = target.map_or_else(
        || (None, None, None, None, Vec::new()),
        |metadata| {
            (
                Some(metadata.run_id),
                Some(metadata.target),
                Some(metadata.config_digest),
                Some(metadata.input_digest),
                metadata.findings,
            )
        },
    );
    let root_spellings = repository_root_spellings(ctx.root());
    let receipt = ReceiptRecord {
        id: new_id("receipt"),
        session_id: match input.session_override {
            Some(session_id) => Some(session_id),
            None => current_session(ctx)?,
        },
        plan_id: input.plan_id,
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
    with_receipt_journal_writer(ctx, |writer| writer.append(&receipt))?;
    Ok(receipt_id)
}

pub(super) fn record_successful_state_tool(
    ctx: &RepoContext,
    input: StateToolReceipt<'_>,
) -> Result<String> {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: input.tool_name,
            args: input.args,
            invoked_command_key: None,
            plan_id: input.plan_id,
            started_at_ms: input.started_at_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: input.session_override,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
}

fn receipt_matches_filters(receipt: &ReceiptRecord, filter: &ReceiptListFilter) -> bool {
    let session_matches = filter
        .session_id
        .as_ref()
        .is_none_or(|session_id| receipt.session_id.as_ref() == Some(session_id));
    let plan_matches = filter
        .plan_id
        .as_ref()
        .is_none_or(|plan_id| receipt.plan_id.as_ref() == Some(plan_id));
    let tool_matches = filter
        .tool_name
        .as_ref()
        .is_none_or(|tool_name| receipt.tool_name == *tool_name);
    let failure_matches = !filter.failed_only || receipt.exit_status != 0;

    session_matches && plan_matches && tool_matches && failure_matches
}

fn receipt_args_include_receipt_id(receipt: &ReceiptRecord, receipt_id: &str) -> bool {
    receipt
        .args
        .get("receipt_ids")
        .and_then(Value::as_array)
        .is_some_and(|receipt_ids| {
            receipt_ids
                .iter()
                .any(|candidate| candidate.as_str() == Some(receipt_id))
        })
}

fn receipt_args_has_receipt_ids(receipt: &ReceiptRecord) -> bool {
    receipt
        .args
        .get("receipt_ids")
        .and_then(Value::as_array)
        .is_some()
}

pub(super) fn receipt_list_value(receipt: ReceiptRecord) -> Result<Value> {
    let diff_summary = receipt_diff_summary(&receipt);
    let mut value = serde_json::to_value(receipt)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("diff_summary".to_string(), Value::String(diff_summary));
    }
    Ok(value)
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
    }
}

fn target_receipt_status(
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

fn work_review_receipt_status(receipt: &ReceiptRecord) -> WorkReviewReceiptStatus {
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

fn work_review_receipt_evidence(evidence: &Value) -> WorkReviewReceiptEvidence {
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

pub(super) fn receipt_diff_summary(receipt: &ReceiptRecord) -> String {
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
