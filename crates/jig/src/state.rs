use anyhow::Result;
use serde_json::Value;

use jig_context::RepoContext;

#[cfg(test)]
pub(crate) use execution_leases::acquire_repository_execution_lease;
#[cfg(test)]
pub(crate) use execution_leases::acquire_repository_execution_lease_without_wait;
pub(crate) use execution_leases::{
    RepositoryExecutionLease, try_acquire_repository_execution_lease,
};
#[cfg(test)]
use jsonl::append_jsonl;
#[cfg(test)]
use jsonl::read_jsonl;
pub(crate) use jsonl::{JsonlRecordTooLarge, RawJsonlRecord, scan_dashboard_jsonl_raw};
#[cfg(test)]
pub(crate) use jsonl::{dashboard_scan_count, reset_dashboard_scan_counts};
pub(crate) use runs::{CompletedTargetEvent, RunHistoryEvent, run_history_event};
pub(crate) use runs::{
    DurableRun, RunEventCursor, block_nonterminal_run, complete_run, mark_run_running,
    mark_target_started, reconcile_run_for_inspection, record_target_result, run_by_id,
    run_cancel_requested_since, start_run_with_event_cursor_and_execution_lease,
    start_run_with_execution_lease,
};
#[cfg(test)]
pub(crate) use runs::{RunLease, request_run_cancel, start_run};
#[cfg(test)]
pub(crate) use summary::state_summary;
pub(crate) use summary::state_summary_with_cancellation;
pub(crate) use support::now_ms;
#[cfg(test)]
pub(crate) use support::set_test_now_ms;

mod compression;
mod diagnostics;
mod execution_leases;
mod resource_leases;
pub(crate) use resource_leases::{ResourceClaim, ResourceClaimMode, ResourceLease};
mod jsonl;
mod maintenance;
mod privacy;
mod records;
mod runs;
mod summary;
mod support;

pub(super) const MAINTENANCE_WRITER_COORDINATION_NOTE: &str = "Before applying a state rewrite, stop Jig processes launched with older runtimes that wrote through a pre-opened state-file handle. Current runtimes coordinate through the repository state lock.";

pub(crate) use diagnostics::state_diagnose;
pub(crate) use maintenance::restore_backup;

/// Archives completed run histories, the only stream Jig still rewrites.
pub(crate) fn state_archive(
    ctx: &RepoContext,
    request: crate::command::StateArchiveRequest,
) -> Result<Value> {
    runs::runs_archive(ctx, &request.before, request.dry_run)
}

#[cfg(test)]
mod tests;
