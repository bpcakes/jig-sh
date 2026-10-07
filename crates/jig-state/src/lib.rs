//! Append-only repository state under `.agent/state`: run history, execution
//! and resource leases, and the summary, diagnosis, archive, and restore
//! maintenance over them.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::Value;

use jig_context::RepoContext;

#[cfg(any(test, feature = "test-support"))]
pub use execution_leases::acquire_repository_execution_lease;
#[cfg(any(test, feature = "test-support"))]
pub use execution_leases::acquire_repository_execution_lease_without_wait;
pub use execution_leases::{RepositoryExecutionLease, try_acquire_repository_execution_lease};
#[cfg(test)]
use jsonl::append_jsonl;
#[cfg(test)]
use jsonl::read_jsonl;
pub use jsonl::{JsonlRecordTooLarge, RawJsonlRecord, scan_dashboard_jsonl_raw};
#[cfg(any(test, feature = "test-support"))]
pub use jsonl::{dashboard_scan_count, reset_dashboard_scan_counts};
pub use runs::{CompletedTargetEvent, RunHistoryEvent, run_history_event};
pub use runs::{
    DurableRun, RunEventCursor, block_nonterminal_run, complete_run, mark_run_running,
    mark_target_started, reconcile_run_for_inspection, record_target_result, run_by_id,
    run_cancel_requested_since, start_run_with_event_cursor_and_execution_lease,
    start_run_with_execution_lease,
};
#[cfg(any(test, feature = "test-support"))]
pub use runs::{RunLease, request_run_cancel, start_run};
#[cfg(test)]
pub use summary::state_summary;
pub use summary::state_summary_with_cancellation;
pub use support::now_ms;
#[cfg(any(test, feature = "test-support"))]
pub use support::set_test_now_ms;

pub mod cancellation;
mod compression;
mod diagnostics;
mod execution_leases;
mod resource_leases;
pub use resource_leases::{ResourceClaim, ResourceClaimMode, ResourceLease};
mod jsonl;
mod maintenance;
mod privacy;
mod records;
mod runs;
mod summary;
mod support;

pub(crate) const MAINTENANCE_WRITER_COORDINATION_NOTE: &str = "Before applying a state rewrite, stop Jig processes launched with older runtimes that wrote through a pre-opened state-file handle. Current runtimes coordinate through the repository state lock.";

pub use diagnostics::state_diagnose;
pub use maintenance::restore_backup;

#[derive(Debug)]
pub struct StateRestoreRequest {
    pub backup: PathBuf,
}

#[derive(Debug)]
pub struct StateArchiveRequest {
    pub before: String,
    pub dry_run: bool,
}

/// Archives completed run histories, the only stream Jig still rewrites.
pub fn state_archive(ctx: &RepoContext, request: StateArchiveRequest) -> Result<Value> {
    runs::runs_archive(ctx, &request.before, request.dry_run)
}

#[cfg(test)]
mod tests;
