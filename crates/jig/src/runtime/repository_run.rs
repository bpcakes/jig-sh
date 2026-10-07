use anyhow::{Result, bail};
use jig_context::RepoContext;
use jig_contract::ActionEffect;
use jig_execution::ExecutionControl;
use jig_repository::{PlanRunRequest, RepositoryCatalog};
use serde_json::{Value, json};

use crate::command::RepositoryRunRequest;

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
        && current.contract_version() < jig_repository::FILE_BUDGET_CONTRACT_VERSION
    {
        bail!("explicit run comparison authority requires repository contract version 7 or later");
    }
    let plan = jig_repository::plan_action_run_with_cancellation(
        &current,
        &catalog,
        PlanRunRequest {
            selectors: request.selectors,
            profile: request.profile,
            affected_base: request.affected_base,
            comparison: request.comparison,
        },
        request.arguments,
        &|| observer.cancelled(),
    )?;
    if request.explain {
        return Ok(json!({"ok": true, "command": "run plan", "executed": false, "plan": plan}));
    }
    validate_effect_approval("jig run", &plan.effects, &request.approved_effects)?;
    let execution = execute_foreground_action_run(
        &current,
        &catalog,
        plan.clone(),
        ExecuteCheckRunRequest {
            alias_override: None,
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
