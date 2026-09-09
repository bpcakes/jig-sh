use anyhow::{Result, bail};
use jig_contract::ActionEffect;
use serde_json::{Value, json};

use crate::command::RepositoryRunRequest;
use crate::context::RepoContext;
use crate::execution::ExecutionControl;
use crate::repository::{PlanRunRequest, RepositoryCatalog};

use super::run_execution::{ExecuteCheckRunRequest, execute_foreground_action_run};

pub(super) fn dispatch(
    ctx: &RepoContext,
    request: RepositoryRunRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let current = super::refreshed_repository_context(ctx)?;
    if current.contract_version() < 6 {
        bail!(
            "jig run requires repository contract version 6 or later; migrate the repository with jig update"
        );
    }
    let catalog = RepositoryCatalog::from_context(&current)?;
    if request.comparison.is_some()
        && current.contract_version() < crate::repository::FILE_BUDGET_CONTRACT_VERSION
    {
        bail!("explicit run comparison authority requires repository contract version 7 or later");
    }
    let (work_plan_id, record_receipts) = request.tool.into_parts();
    let plan = crate::repository::plan_action_run(
        &current,
        &catalog,
        PlanRunRequest {
            selectors: request.selectors,
            profile: request.profile,
            affected_base: request.affected_base,
            comparison: request.comparison,
            work_plan_id: work_plan_id.clone(),
        },
        request.arguments,
    )?;
    if request.explain {
        return Ok(json!({"ok": true, "command": "run plan", "executed": false, "plan": plan}));
    }
    validate_effect_approval("jig run", &plan.effects, &request.approved_effects)?;
    if let Some(id) = work_plan_id.as_deref() {
        crate::state::ensure_plan_is_open(&current, id)?;
    }
    let execution = execute_foreground_action_run(
        &current,
        &catalog,
        plan.clone(),
        ExecuteCheckRunRequest {
            work_plan_id,
            record_receipts,
            fail_fast: request.fail_fast,
        },
        observer,
    )?;
    Ok(json!({
        "ok": execution.run.result.conclusion == Some(jig_contract::RunConclusion::Success),
        "command": "run", "executed": true, "plan": plan,
        "run": execution.run.result, "results": execution.results,
        "failed_targets": execution.failed_targets, "source_observations": execution.source_observations,
    }))
}

pub(super) fn validate_effect_approval(
    caller: &str,
    planned: &[ActionEffect],
    approved: &[ActionEffect],
) -> Result<()> {
    let requires_approval = planned
        .iter()
        .copied()
        .filter(|effect| matches!(effect, ActionEffect::Worktree | ActionEffect::External))
        .collect::<std::collections::BTreeSet<_>>();
    let approved = approved
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if approved != requires_approval {
        bail!(
            "{caller} requires approved_effects {:?} for this exact plan",
            requires_approval
        );
    }
    Ok(())
}
