use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;

#[cfg(test)]
pub(crate) use execution_leases::acquire_repository_execution_lease;
pub(crate) use execution_leases::{
    RepositoryExecutionLease, acquire_repository_execution_lease_without_wait,
    try_acquire_repository_execution_lease,
};
#[cfg(test)]
use jsonl::append_jsonl;
#[cfg(test)]
use jsonl::read_jsonl;
pub(crate) use jsonl::{JsonlRecordTooLarge, RawJsonlRecord, scan_dashboard_jsonl_raw};
#[cfg(test)]
pub(crate) use jsonl::{dashboard_scan_count, reset_dashboard_scan_counts};
pub(crate) use receipts::WORK_CHECK_EVIDENCE_SCHEMA;
pub(crate) use receipts::WORK_CHECK_TARGETS_SCHEMA;
pub(crate) use receipts::receipt_append_may_have_landed;
#[cfg(test)]
pub(crate) use receipts::receipt_append_may_have_landed_for_test;
#[cfg(test)]
pub(crate) use receipts::record_receipt;
pub(crate) use receipts::{
    ReceiptInput, record_receipt_with_cancellation, record_receipt_with_cancellation_until,
};
pub(crate) use receipts::{StateArchiveRequest, receipts_archive, receipts_export};
pub(crate) use receipts::{receipt_record_id, with_receipt_journal_writer};
#[cfg(test)]
use records::ReceiptRecord;
pub(crate) use runs::{CompletedTargetEvent, RunHistoryEvent, run_history_event};
pub(crate) use runs::{
    DurableRun, RunEventCursor, RunLease, block_nonterminal_run, complete_run, mark_run_running,
    mark_target_started, reconcile_run_for_inspection, record_target_result, request_run_cancel,
    run_by_id, run_cancel_requested_since, start_run_with_event_cursor_and_execution_lease,
    start_run_with_execution_lease,
};
#[cfg(test)]
pub(crate) use runs::{start_run, start_run_with_event_cursor};
#[cfg(test)]
pub(crate) use summary::state_summary;
pub(crate) use summary::state_summary_with_cancellation;
#[cfg(test)]
use support::ensure_state_layout;
pub(crate) use support::now_ms;
#[cfg(test)]
pub(crate) use support::set_test_now_ms;
#[cfg(test)]
use support::truncate;

mod compression;
mod diagnostics;
mod execution_leases;
mod resource_leases;
pub(crate) use resource_leases::{ResourceClaim, ResourceClaimMode, ResourceLease};
mod json_scan;
mod jsonl;
mod maintenance;
mod privacy;
mod receipts;
mod records;
mod runs;
mod summary;
mod support;

pub(super) const MAINTENANCE_WRITER_COORDINATION_NOTE: &str = "Before applying a state rewrite, stop Jig processes launched with older runtimes that wrote through a pre-opened state-file handle. Current runtimes coordinate through the repository state lock.";

pub(crate) use diagnostics::state_diagnose;
pub(crate) use maintenance::restore_backup;

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

#[cfg(test)]
mod tests;
