use anyhow::Result;
use jig_context::RepoContext;
use jig_execution::ExecutionControl;
use serde_json::Value;

use crate::command::{LoopCommand, LoopStatusRequest};

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
mod workflow;
mod workflow_state;

#[cfg(test)]
pub(in crate::runtime) use schedule::dispatch_due_at;

#[cfg(test)]
pub(in crate::runtime) fn revoke_lease_for_test(ctx: &RepoContext, key: &str) -> Result<()> {
    if key.starts_with("branch:") {
        state::LeaseStore::new_repository(ctx).revoke_for_test(key)
    } else {
        state::LeaseStore::new(ctx).revoke_for_test(key)
    }
}

/// Where loop occurrence evidence is recorded for this repository.
#[cfg(test)]
pub(in crate::runtime) fn evidence_directory_for_test(ctx: &RepoContext) -> std::path::PathBuf {
    evidence::directory_for_test(ctx).unwrap()
}

pub(super) fn dispatch_with_observer(
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

pub(super) fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    engine::status_with_cancellation(ctx, request, cancelled)
}

pub(super) fn typed_status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<jig_ui::dashboard::StatusLoopObservation> {
    engine::typed_status_with_cancellation(ctx, request, cancelled)
}
