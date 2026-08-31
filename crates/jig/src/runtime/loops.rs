use anyhow::Result;
use serde_json::Value;

use crate::command::{LoopCommand, LoopStatusRequest};
use crate::context::RepoContext;
use crate::execution::ExecutionControl;

mod authority;
mod codex_task;
mod engine;
mod github;
mod managed_path;
mod noop;
mod occurrence;
mod pr_manager;
mod pre_execution;
mod renewal;
mod schedule;
mod state;
mod workflow;
mod workflow_state;

#[cfg(test)]
pub(in crate::runtime) use schedule::dispatch_due_at;

#[cfg(test)]
pub(in crate::runtime) fn revoke_lease_for_test(ctx: &RepoContext, key: &str) -> Result<()> {
    state::LeaseStore::new(ctx).revoke_for_test(key)
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
