use anyhow::Result;
use serde_json::json;

use crate::context::RepoContext;
use crate::state::open_plan_summaries;

use super::workflow::WorkflowTick;

pub(super) fn noop_status_tick(ctx: &RepoContext) -> Result<WorkflowTick> {
    let open_plans = open_plan_summaries(ctx)?;
    Ok(WorkflowTick {
        observed: json!({
            "repo": {
                "name": ctx.repo_name(),
                "default_branch": ctx.default_branch(),
            },
            "open_plan_count": open_plans.len(),
            "open_plans": open_plans,
            "work_gate_count": ctx.work_gates().len(),
        }),
        actions: Vec::new(),
    })
}
