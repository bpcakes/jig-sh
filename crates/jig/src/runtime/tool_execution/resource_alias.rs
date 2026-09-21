//! Compatibility aliases share prepared-target resource admission, dependency
//! execution and receipt publication. This adapter owns only the old envelope.

use std::collections::BTreeMap;

use super::*;
use crate::repository::{PlanRunRequest, plan_action_run_with_cancellation};
use crate::runtime::run_execution::{
    ExecuteCheckRunRequest, ExecutionAliasOverride, execute_freshly_planned_check_run_with_lease,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    ctx: &RepoContext,
    tool: &ManifestTool,
    action: ActionSpec,
    args: Value,
    plan_id: Option<String>,
    options: ManifestToolExecutionOptions,
    observer: &mut dyn ExecutionControl,
    repository_execution: crate::state::RepositoryExecutionLease,
) -> Result<ManifestToolExecutionOutcome> {
    let catalog = RepositoryCatalog::from_context(ctx)?;
    let target = action.target;
    let plan = plan_action_run_with_cancellation(
        ctx,
        &catalog,
        PlanRunRequest {
            selectors: vec![target.to_string()],
            work_plan_id: plan_id.clone(),
            ..PlanRunRequest::default()
        },
        BTreeMap::from([(target.clone(), serde_json::from_value(args.clone())?)]),
        &|| observer.cancelled(),
    )?;
    if plan.targets.iter().any(|planned| {
        !planned
            .effects
            .contains(&jig_contract::ActionEffect::ReadOnly)
            || planned.effects.iter().any(|effect| {
                !matches!(
                    effect,
                    jig_contract::ActionEffect::ReadOnly | jig_contract::ActionEffect::Process
                )
            })
    }) || !repository_execution.permits(&plan.effects)
    {
        // An alias for a read-only action does not authorize its graph to
        // mutate the checkout or external systems, even under a stronger lock.
        bail!(
            "resource-bearing alias prerequisites require effects beyond read-only process checks; inspect the canonical target with `jig run {} --explain` and explicitly authorize its effects before executing it",
            target
        );
    }
    let execution = execute_freshly_planned_check_run_with_lease(
        ctx,
        &catalog,
        plan,
        ExecuteCheckRunRequest {
            work_plan_id: plan_id,
            record_receipts: options.record_receipt,
            fail_fast: true,
            reuse_after_resource_wait: false,
            alias_override: Some(ExecutionAliasOverride {
                target: target.clone(),
                tool_name: tool.name.clone(),
                args: args.clone(),
            }),
        },
        observer,
        repository_execution,
        true,
    )?;
    let result = execution
        .run
        .result
        .targets
        .iter()
        .find(|result| result.target == target)
        .ok_or_else(|| anyhow!("resource-bearing alias execution omitted its selected target"))?;
    let command_key = match &action.runner {
        ActionRunner::Command { command, .. } | ActionRunner::Shell { command, .. } => {
            Some(command.as_str())
        }
        _ => None,
    };
    let mut response = execution
        .results
        .iter()
        .find(|entry| entry.get("target") == Some(&json!(target)))
        .and_then(|entry| entry.get("response"))
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| {
            tool_response_value(ToolExecutionResponse {
                ok: true,
                tool: &tool.name,
                command_key,
                args,
                result: ToolProcessResult {
                    exit_status: result.exit_code.unwrap_or(1),
                    stdout: String::new(),
                    stderr: result
                        .findings
                        .iter()
                        .map(|finding| finding.message.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                },
                receipt_id: result.receipt_id.clone(),
            })
        })?;
    // The compatibility envelope indicates successful dispatch. Its process
    // status remains the source of execution failure, as for ordinary aliases.
    response["ok"] = json!(true);
    if result.conclusion == Some(RunConclusion::Cancelled)
        || execution.run.result.conclusion == Some(RunConclusion::Cancelled)
    {
        return Ok(ManifestToolExecutionOutcome::Cancelled(response));
    }
    let failure = manifest_tool_result_failure(&response)?.map(|(_, message)| message);
    receipt_id_for_failure_mode(options.failure_mode, failure, Ok(result.receipt_id.clone()))?;
    Ok(ManifestToolExecutionOutcome::Completed(response))
}

#[cfg(test)]
mod tests;
