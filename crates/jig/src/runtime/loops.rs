use anyhow::Result;
use serde_json::Value;

use crate::command::{LoopCommand, LoopStatusRequest};
use crate::context::RepoContext;

mod engine;
mod github;
mod noop;
mod pr_manager;
mod schedule;
mod state;
mod workflow;

pub(super) fn dispatch(ctx: &RepoContext, command: LoopCommand) -> Result<Value> {
    match command {
        LoopCommand::Tick(request) => engine::tick(ctx, request),
        LoopCommand::Status(request) => engine::status(ctx, request),
        LoopCommand::Run(request) => schedule::run_until(ctx, request),
        LoopCommand::ClearAttempt(request) => engine::clear_attempt(ctx, request),
    }
}

pub(super) fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    engine::status_with_cancellation(ctx, request, cancelled)
}
