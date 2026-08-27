use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::tool;

pub(crate) use execution_leases::{
    RepositoryExecutionLease, acquire_repository_execution_lease,
    acquire_repository_execution_lease_without_wait, try_acquire_repository_execution_lease,
};
use jsonl::append_jsonl;
#[cfg(test)]
use jsonl::read_jsonl;
pub(crate) use plans::{
    PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, PlanStatus, ensure_plan_exists,
    ensure_plan_exists_with_cancellation, ensure_plan_is_open, open_plan_summaries,
    open_plan_summaries_with_cancellation, plan_baseline, plan_baseline_with_cancellation,
    plan_baselines_with_cancellation, plan_status, plan_status_with_cancellation, plans_append,
    plans_close, plans_open_prepared, prepare_plan_open,
};
#[cfg(test)]
pub(crate) use plans::{plans_open, seed_open_plan_for_test};
pub(crate) use receipts::{
    CurrentWorktreeFingerprint, ReusableWorkCheckEvidence, ReusableWorkCheckQuery,
    TargetReceiptStatus, ToolReceiptStatus, WORK_CHECK_EVIDENCE_SCHEMA, WorkCheckBatchEvidence,
    WorkCheckGateEvidence, WorkCheckGateReceiptStatus, WorkGateReceiptIndex,
    WorkReviewReceiptEvidence, WorkReviewReceiptStatus, current_worktree_fingerprint,
    current_worktree_fingerprint_for_receipt_with_cancellation,
    current_worktree_fingerprint_with_cancellation,
    reusable_work_check_evidence_batch_with_cancellation, work_gate_receipt_index,
    work_gate_receipt_index_with_cancellation, work_gate_receipt_indexes_with_cancellation,
};
pub(crate) use receipts::{
    ReceiptInput, ReceiptListFilter, receipts_list, record_receipt,
    record_receipt_with_cancellation,
};
pub(crate) use receipts::{StateArchiveRequest, receipts_archive, receipts_export};
use receipts::{StateToolReceipt, record_successful_state_tool};
pub(crate) use receipts::{TargetReceiptMetadata, record_target_receipt};
#[cfg(test)]
pub(crate) use receipts::{
    reset_work_gate_receipt_index_scan_count, work_gate_receipt_index_scan_count,
};
use records::DecisionRecord;
pub(crate) use records::PlanBaseline;
#[cfg(test)]
use records::{PlanEvent, ReceiptRecord};
pub(crate) use runs::{
    DurableRun, RunEventCursor, RunLease, block_nonterminal_run, complete_run, mark_run_running,
    mark_target_started, reconcile_run_for_inspection, record_target_result, request_run_cancel,
    run_by_id, run_cancel_requested_since, start_run_with_event_cursor_and_execution_lease,
    start_run_with_execution_lease,
};
#[cfg(test)]
pub(crate) use runs::{start_run, start_run_with_event_cursor};
#[cfg(test)]
use sessions::build_summary;
pub(crate) use sessions::current_session;
pub(crate) use sessions::{
    SessionEndRequest, session_end, session_start, state_summary, state_summary_with_cancellation,
};
pub(crate) use support::now_ms;
#[cfg(test)]
use support::truncate;
use support::{ensure_state_layout, new_id};
pub(crate) use timeline::{
    DecisionStreamRecord, PlanStreamEvent, ReceiptStreamRecord, StateStreams, plan_detail_streams,
    plan_receipts, state_streams,
};

mod compression;
mod diagnostics;
mod execution_leases;
mod jsonl;
mod maintenance;
mod plans;
mod privacy;
mod receipts;
mod records;
mod runs;
mod session_compaction;
mod sessions;
mod support;
mod timeline;

pub(super) const MAINTENANCE_WRITER_COORDINATION_NOTE: &str = "Before applying a state rewrite, stop Jig processes launched with older runtimes that wrote through a pre-opened state-file handle. Current runtimes coordinate through the repository state lock.";

pub(crate) use diagnostics::state_diagnose;
pub(crate) use maintenance::{compact_sessions, restore_backup};

pub(crate) fn state_archive(
    ctx: &RepoContext,
    request: crate::command::StateArchiveRequest,
) -> Result<Value> {
    // Validate receipts before an applying invocation rewrites the run stream.
    // The run apply performs its own lifecycle validation under its write lock.
    if request.include_runs && !request.dry_run {
        receipts_archive(
            ctx,
            StateArchiveRequest {
                before: request.before.clone(),
                dry_run: true,
            },
        )?;
    }

    // Apply the harder run-journal invariant first. A later receipt failure is
    // still recoverable per stream, and the decorated error below preserves
    // the already-completed run backup/artifact paths for the operator.
    let runs = request
        .include_runs
        .then(|| runs::runs_archive(ctx, &request.before, request.dry_run))
        .transpose()?;
    let mut output = receipts_archive(
        ctx,
        StateArchiveRequest {
            before: request.before.clone(),
            dry_run: request.dry_run,
        },
    )
    .map_err(|error| decorate_receipt_archive_failure(error, runs.as_ref(), request.dry_run))?;
    let output_object = output
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("receipt archive result was not an object"))?;
    output_object.insert("runs_included".into(), json!(request.include_runs));
    if let Some(runs) = runs {
        let runs_object = runs
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("run archive result was not an object"))?;
        output_object.extend(runs_object.clone());
    }
    Ok(output)
}

fn decorate_receipt_archive_failure(
    error: anyhow::Error,
    runs: Option<&Value>,
    dry_run: bool,
) -> anyhow::Error {
    let Some(runs) = runs.filter(|_| !dry_run) else {
        return error;
    };
    let archived = runs["runs_archived"].as_u64().unwrap_or(0);
    if archived == 0 {
        return error;
    }
    let backup = runs["runs_recovery_backup_path"]
        .as_str()
        .unwrap_or("<missing run recovery backup path>");
    let archive = runs["runs_archive_path"]
        .as_str()
        .unwrap_or("<missing run-event archive path>");
    anyhow::anyhow!(
        "{error:#}\nRun archival completed before receipt archival failed: {archived} run(s) were archived; exact run recovery backup: {backup}; run-event archive: {archive}"
    )
}

#[derive(Debug, Deserialize)]
pub(crate) struct DecisionAddRequest {
    pub(crate) title: String,
    pub(crate) selected_option: String,
    pub(crate) rationale: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) alternatives: Vec<String>,
    pub(crate) plan_id: Option<String>,
}

pub(crate) fn decisions_add(ctx: &RepoContext, request: DecisionAddRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let record = DecisionRecord {
        id: new_id("decision"),
        session_id: current_session(ctx)?,
        plan_id: request.plan_id.clone(),
        title: request.title.clone(),
        selected_option: request.selected_option.clone(),
        rationale: request.rationale.clone(),
        alternatives: request.alternatives.clone(),
        timestamp_ms: now_ms(),
    };
    append_jsonl(&ctx.state_file("decisions.jsonl"), &record)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::DECISIONS_ADD,
            args: json!({
                "title": request.title,
                "selected_option": request.selected_option,
                "plan_id": request.plan_id,
            }),
            started_at_ms: record.timestamp_ms,
            plan_id: record.plan_id.clone(),
            session_override: record.session_id.clone(),
        },
    )?;

    Ok(json!({
        "ok": true,
        "decision_id": record.id,
        "receipt_id": receipt_id,
    }))
}

#[cfg(test)]
mod tests;
