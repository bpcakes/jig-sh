//! Scheduled and manual agent loops: workflow dispatch, occurrences and
//! their evidence, leases, Codex task workers, and the PR manager.

use anyhow::Result;
use jig_context::RepoContext;
use jig_execution::ExecutionControl;
use serde_json::Value;

mod authority;
mod codex_task;
mod dashboard;
mod engine;
mod evidence;
mod github;
mod managed_path;
mod noop;
mod occurrence;
mod pr_manager;
mod pre_execution;
mod renewal;
mod schedule;
mod show;
mod state;
mod worker_runner;
mod workflow;
mod workflow_state;

mod request;
pub use request::{
    LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopCommand, LoopDispatchRequest,
    LoopRunRequest, LoopShowRequest, LoopStatusRequest, LoopTickRequest,
};

#[cfg(any(test, feature = "test-support"))]
pub use schedule::dispatch_due_at;

#[cfg(any(test, feature = "test-support"))]
pub fn revoke_lease_for_test(ctx: &RepoContext, key: &str) -> Result<()> {
    if key.starts_with("branch:") {
        state::LeaseStore::new_repository(ctx).revoke_for_test(key)
    } else {
        state::LeaseStore::new(ctx).revoke_for_test(key)
    }
}

/// Where loop occurrence evidence is recorded for this repository.
#[cfg(any(test, feature = "test-support"))]
pub fn evidence_directory_for_test(ctx: &RepoContext) -> std::path::PathBuf {
    evidence::directory_for_test(ctx).unwrap()
}

pub fn dispatch_with_observer(
    ctx: &RepoContext,
    command: LoopCommand,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    match command {
        LoopCommand::Tick(request) => engine::tick_with_observer(ctx, request, observer),
        LoopCommand::Dispatch(request) => {
            schedule::dispatch_due_with_observer(ctx, request, observer)
        }
        LoopCommand::Status(request) => {
            engine::status_with_cancellation(ctx, request, &|| observer.cancelled())
        }
        LoopCommand::Show(request) => show::show_occurrence(ctx, request, &|| observer.cancelled()),
        LoopCommand::Run(request) => schedule::run_until_with_observer(ctx, request, observer),
        LoopCommand::ClearAttempt(request) => engine::clear_attempt(ctx, request, observer),
        LoopCommand::AcknowledgeOccurrence(request) => {
            engine::acknowledge_occurrence(ctx, request, observer)
        }
    }
}

pub fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    engine::status_with_cancellation(ctx, request, cancelled)
}

pub fn typed_status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<jig_dashboard::StatusLoopObservation> {
    engine::typed_status_with_cancellation(ctx, request, cancelled)
}

#[cfg(test)]
use jig_context::test_support as test_env;
