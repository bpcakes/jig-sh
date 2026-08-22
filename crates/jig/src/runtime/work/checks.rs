use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::command::WorkCheckRequest;
use crate::context::RepoContext;
use crate::execution::{ExecutionControl, PhasePosition};
use crate::state::{ReceiptInput, current_worktree_fingerprint, now_ms, record_receipt};
use crate::tool_defs::tool;

use super::super::tool_execution::{ManifestToolExecutionOutcome, manifest_tool_result_failure};
use super::tools::{selected_tools, validate_check_tool};

pub(super) fn check_with_observer(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    check_tools_with_observer(
        ctx,
        &opts.plan_id,
        selected_tools(ctx, &opts.tools)?,
        observer,
    )
}

pub(super) fn check_tools_with_observer(
    ctx: &RepoContext,
    plan_id: &str,
    tools: Vec<String>,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    check_tools_with_failure_mode(ctx, plan_id, tools, true, observer)
}

pub(in crate::runtime) fn check_tools_collect_failures_with_observer(
    ctx: &RepoContext,
    plan_id: &str,
    tools: Vec<String>,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    // Used by review refinement so failed verification checks are reported in
    // the refine result instead of aborting before all receipts are recorded.
    check_tools_with_failure_mode(ctx, plan_id, tools, false, observer)
}

fn check_tools_with_failure_mode(
    ctx: &RepoContext,
    plan_id: &str,
    tools: Vec<String>,
    fail_on_tool_error: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let before_fingerprint = current_worktree_fingerprint(ctx);
    for name in &tools {
        validate_check_tool(ctx, name, "Work check")?;
    }

    let mut results = Vec::with_capacity(tools.len());
    let mut check_failure = None;
    for (index, name) in tools.iter().enumerate() {
        if observer.cancelled() {
            check_failure = Some((1, anyhow!("Work check was cancelled before {name} started")));
            break;
        }
        let position = PhasePosition::new(index + 1, tools.len())
            .expect("work checks are enumerated within a nonempty tool list");
        let result =
            match super::super::tool_execution::execute_manifest_tool_with_options_for_work_check(
                ctx,
                name,
                json!({}),
                Some(plan_id.to_string()),
                position,
                observer,
            ) {
                Ok(ManifestToolExecutionOutcome::Completed(result)) => result,
                Ok(ManifestToolExecutionOutcome::Cancelled(result)) => {
                    let (_, message) = manifest_tool_result_failure(&result)?
                        .ok_or_else(|| anyhow!("Cancelled tool returned a successful result"))?;
                    results.push(result);
                    check_failure = Some((1, anyhow!(message)));
                    break;
                }
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
    let receipt_stderr = check_failure
        .as_ref()
        .map(|(_, error)| format!("{error:#}"))
        .unwrap_or_default();
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
            stderr: &receipt_stderr,
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
