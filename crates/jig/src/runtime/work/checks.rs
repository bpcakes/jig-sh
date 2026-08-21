use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail};
use jig_contract::RunConclusion;
use serde_json::{Value, json};

use crate::command::WorkCheckRequest;
use crate::context::{RepoContext, WorkGate};
use crate::repository::{PlanRunRequest, RepositoryCatalog, plan_run, resolve_evidence_targets};
use crate::state::{ReceiptInput, current_worktree_fingerprint, now_ms, record_receipt};
use crate::tool_defs::tool;

use super::super::run_execution::{ExecuteCheckRunRequest, execute_check_run};
use super::super::tool_execution::{
    execute_manifest_tool_result_without_worktree_fingerprint, manifest_tool_result_failure,
};
use super::tools::{selected_tools, validate_check_tool};

pub(super) fn check(ctx: &RepoContext, opts: WorkCheckRequest) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    if opts.tools.is_empty() {
        check_configured_with_failure_mode(ctx, &opts.plan_id, true)
    } else {
        check_tools(ctx, &opts.plan_id, selected_tools(ctx, &opts.tools)?)
    }
}

pub(super) fn check_tools(ctx: &RepoContext, plan_id: &str, tools: Vec<String>) -> Result<Value> {
    check_tools_with_failure_mode(ctx, plan_id, tools, true)
}

pub(super) fn check_configured_collect_failures(ctx: &RepoContext, plan_id: &str) -> Result<Value> {
    check_configured_with_failure_mode(ctx, plan_id, false)
}

fn check_configured_with_failure_mode(
    ctx: &RepoContext,
    plan_id: &str,
    fail_on_check_error: bool,
) -> Result<Value> {
    let tools = ctx.work_check_tools();
    let catalog = RepositoryCatalog::from_context(ctx)?;
    let targets = configured_evidence_targets(ctx, &catalog)?;
    if tools.is_empty() && targets.is_empty() {
        bail!(
            "No work check gates configured. Add check or evidence work.gates to .jig.toml, or pass --tool."
        );
    }

    let mut result = if tools.is_empty() {
        json!({
            "ok": true,
            "plan_id": plan_id,
            "checks": [],
            "receipt_id": null,
        })
    } else {
        check_tools_with_failure_mode(ctx, plan_id, tools, fail_on_check_error)?
    };

    if targets.is_empty() {
        return Ok(result);
    }
    let plan = plan_run(
        ctx,
        &catalog,
        PlanRunRequest {
            selectors: targets.iter().map(ToString::to_string).collect(),
            profile: None,
            affected_base: None,
        },
    )?;
    let execution = execute_check_run(
        ctx,
        &catalog,
        plan.clone(),
        ExecuteCheckRunRequest {
            work_plan_id: Some(plan_id.to_owned()),
            record_receipts: true,
            fail_fast: false,
        },
        &|| false,
    )?;
    let evidence_ok = execution.run.result.conclusion == Some(RunConclusion::Success);
    let failed_target_labels = execution
        .failed_targets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let object = result
        .as_object_mut()
        .ok_or_else(|| anyhow!("work check result was not a JSON object"))?;
    let legacy_ok = object["checks"].as_array().is_none_or(|checks| {
        checks
            .iter()
            .all(|check| check["ok"].as_bool() != Some(false))
    });
    object.insert("ok".into(), json!(legacy_ok && evidence_ok));
    object.insert("plan".into(), json!(plan));
    object.insert("run".into(), json!(execution.run.result));
    object.insert("results".into(), json!(execution.results));
    object.insert("failed_targets".into(), json!(execution.failed_targets));

    if fail_on_check_error && !evidence_ok {
        bail!("Work evidence targets failed: [{failed_target_labels}]");
    }
    Ok(result)
}

fn configured_evidence_targets(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
) -> Result<BTreeSet<jig_contract::TargetId>> {
    let mut targets = BTreeSet::new();
    for gate in ctx.work_gates() {
        let WorkGate::Evidence(gate) = gate else {
            continue;
        };
        targets.extend(resolve_evidence_targets(catalog, &gate.selector)?);
    }
    Ok(targets)
}

fn check_tools_with_failure_mode(
    ctx: &RepoContext,
    plan_id: &str,
    tools: Vec<String>,
    fail_on_tool_error: bool,
) -> Result<Value> {
    let started = now_ms();
    let before_fingerprint = current_worktree_fingerprint(ctx);
    for name in &tools {
        validate_check_tool(ctx, name, "Work check")?;
    }

    let mut results = Vec::with_capacity(tools.len());
    let mut check_failure = None;
    for name in &tools {
        let result = match execute_manifest_tool_result_without_worktree_fingerprint(
            ctx,
            name,
            json!({}),
            Some(plan_id.to_string()),
        ) {
            Ok(result) => result,
            Err(error) if fail_on_tool_error => {
                check_failure = Some((1, error));
                break;
            }
            Err(error) => return Err(error),
        };
        let result_failure = fail_on_tool_error
            .then(|| manifest_tool_result_failure(&result))
            .transpose()?
            .flatten();
        results.push(result);
        if let Some((exit_status, message)) = result_failure {
            check_failure = Some((exit_status, anyhow!(message)));
            break;
        }
    }
    let receipt_ids = results
        .iter()
        .filter_map(|result| result["receipt_id"].as_str())
        .collect::<Vec<_>>();
    let after_fingerprint = current_worktree_fingerprint(ctx);
    let worktree_fingerprint_override =
        work_check_fingerprint_evidence(&before_fingerprint, &after_fingerprint);
    let receipt_result = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": plan_id,
                "tools": tools,
                "receipt_ids": receipt_ids,
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms: started,
            ended_at_ms: now_ms(),
            exit_status: check_failure
                .as_ref()
                .map_or(0, |(exit_status, _)| *exit_status),
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(worktree_fingerprint_override),
        },
    );

    if let Some((_, check_error)) = check_failure {
        return match receipt_result {
            Ok(_) => Err(check_error),
            Err(receipt_error) => {
                bail!(
                    "{check_error:#}\nwork check batch receipt recording also failed:\n{receipt_error:#}"
                )
            }
        };
    }
    let receipt_id = receipt_result?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "checks": results,
        "receipt_id": receipt_id,
    }))
}

fn work_check_fingerprint_evidence(
    before: &crate::state::CurrentWorktreeFingerprint,
    after: &crate::state::CurrentWorktreeFingerprint,
) -> std::result::Result<String, String> {
    let before = before
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("before work check", before.error.as_deref()))?;
    let after = after
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("after work check", after.error.as_deref()))?;

    if before == after {
        Ok(after.to_string())
    } else {
        Err(format!(
            "worktree changed during work check; before fingerprint {before}, after fingerprint {after}; rerun work check after generated changes settle"
        ))
    }
}

fn fingerprint_error(stage: &str, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("Failed to collect worktree fingerprint {stage}: {error}"),
        None => format!("Failed to collect worktree fingerprint {stage}"),
    }
}
