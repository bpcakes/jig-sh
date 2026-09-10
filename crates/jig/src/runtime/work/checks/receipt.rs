use super::*;

pub(super) fn record_check_batch_receipt(
    ctx: &RepoContext,
    plan_id: &str,
    started: u64,
    batch: &PreparedCheckBatch,
    outcome: &BatchExecutionOutcome,
    observer: &dyn ExecutionControl,
) -> Result<String> {
    let receipt_ids = outcome
        .results
        .iter()
        .filter_map(|result| result["receipt_id"].as_str())
        .collect::<Vec<_>>();
    let scope_stability = revalidate_gate_scopes(ctx, plan_id, &batch.initial_gate_scopes, &|| {
        observer.cancelled()
    });
    let after_fingerprint =
        current_worktree_fingerprint_for_receipt_with_cancellation(ctx, &|| observer.cancelled());
    let worktree_fingerprint_override = Some(
        work_check_fingerprint_evidence(&batch.before_fingerprint, &after_fingerprint)
            .and_then(|fingerprint| scope_stability.map(|()| fingerprint)),
    );
    let receipt_stderr = outcome
        .failure
        .as_ref()
        .map(|failure| format!("{:#}", failure.error))
        .unwrap_or_default();
    let cancellation_active = observer.cancelled();
    let valid_until_ms = outcome
        .gate_evidence
        .iter()
        .filter_map(|gate| gate.valid_until_ms)
        .min();
    let requires_time_validity = outcome
        .gate_evidence
        .iter()
        .any(|gate| gate.requires_time_validity);
    let receipt_input = ReceiptInput {
        tool_name: tool::WORK_CHECK,
        args: json!({
            "plan_id": plan_id,
            "gates": batch.selected_gate_ids,
            "tools": batch.selected_tools,
            "receipt_ids": receipt_ids,
        }),
        invoked_command_key: None,
        plan_id: Some(plan_id.to_string()),
        started_at_ms: started,
        ended_at_ms: now_ms(),
        exit_status: outcome
            .failure
            .as_ref()
            .map_or(0, |failure| failure.exit_status),
        stdout: "",
        stderr: &receipt_stderr,
        evidence: Some(serde_json::to_value(WorkCheckBatchEvidence {
            effective_time: batch_effective_time(ctx, outcome),
            schema: WORK_CHECK_EVIDENCE_SCHEMA.into(),
            changed_paths: batch.changes.paths.clone(),
            changed_path_count: batch.changes.path_count,
            changed_paths_truncated: batch.changes.paths_truncated,
            changed_paths_digest: batch.changes.paths_digest.clone(),
            valid_until_ms,
            requires_time_validity,
            gates: outcome.gate_evidence.clone(),
        })?),
        session_override: None,
        collect_git_metadata: !cancellation_active,
        collect_worktree_fingerprint: false,
        worktree_fingerprint_override,
    };
    if cancellation_active {
        // Cancellation is already authoritative, but its batch evidence still
        // has to supersede older passes. Append the small cleanup record
        // without starting fresh Git metadata collection.
        record_receipt(ctx, receipt_input)
    } else {
        record_receipt_with_cancellation(ctx, receipt_input, &|| observer.cancelled())
    }
}
