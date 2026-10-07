use anyhow::Result;
use serde_json::json;

use jig_context::RepoContext;

use super::workflow::WorkflowTick;

pub(super) fn noop_status_tick(ctx: &RepoContext) -> Result<WorkflowTick> {
    Ok(WorkflowTick::from_actions(
        json!({
            "repo": {
                "name": ctx.repo_name(),
                "default_branch": ctx.default_branch(),
            },
        }),
        Vec::new(),
    ))
}
