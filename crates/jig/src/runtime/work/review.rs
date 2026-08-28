use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::command::{WorkRefineRequest, WorkReviewRequest};
use crate::context::{RepoContext, WorkGate, WorkRefinementConfig, WorkReviewGate};
use crate::execution::{ExecutionControl, PhasePosition};
use crate::state::{
    ReceiptInput, current_worktree_fingerprint_for_receipt_with_cancellation,
    current_worktree_fingerprint_with_cancellation, now_ms, record_receipt_with_cancellation,
};
use crate::tool_defs::tool;

use super::checks::{
    check_configured_collect_failures_from_mcp_with_observer,
    check_configured_collect_failures_with_observer,
};

mod evidence;
mod process;
mod prompt;

use evidence::{
    REVIEW_SCHEMA_VERSION, actionable_findings, checks_passed, evidence_preview,
    finding_meets_threshold, hash_json, hash_text, normalize_findings, normalize_severity,
    parse_review_output, review_failed_gates, severity_rank, truncate_evidence_text,
};
use process::{run_codex_refine, run_codex_review};
use prompt::{refine_prompt, review_output_schema, review_prompt};

pub(super) fn review_with_observer(
    ctx: &RepoContext,
    opts: WorkReviewRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    let gates = selected_review_gates(ctx, &opts.gates)?;
    run_review_gates_with_observer(ctx, &opts.plan_id, &gates, observer)
}

pub(super) fn refine_with_observer(
    ctx: &RepoContext,
    opts: WorkRefineRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    refine_with_final_checks(ctx, opts, FinalCheckExecution::WaitForLease, observer)
}

pub(super) fn refine_from_mcp_with_observer(
    ctx: &RepoContext,
    opts: WorkRefineRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    refine_with_final_checks(ctx, opts, FinalCheckExecution::RejectContention, observer)
}

#[derive(Clone, Copy)]
enum FinalCheckExecution {
    WaitForLease,
    RejectContention,
}

fn refine_with_final_checks(
    ctx: &RepoContext,
    opts: WorkRefineRequest,
    check_execution: FinalCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    if opts.max_iterations == 0 {
        bail!("--max-iterations must be at least 1");
    }

    let gates = selected_review_gates(ctx, &opts.gates)?;
    let mut iterations = Vec::new();
    let mut review_result = run_review_gates_with_observer(ctx, &opts.plan_id, &gates, observer)?;
    let refinement = ctx.work_refinements().first();
    let mut fixer_failed = false;
    let mut refinement_required = false;

    for iteration in 1..=opts.max_iterations {
        let findings = actionable_findings(&review_result)?;
        if findings.is_empty() {
            break;
        }
        let Some(refinement) = refinement else {
            refinement_required = true;
            break;
        };

        let refine_receipt = run_fixer(
            ctx,
            RefinementIteration {
                plan_id: &opts.plan_id,
                iteration,
                gates: &gates,
                refinement: Some(refinement),
                findings: &findings,
                position: PhasePosition::new(iteration, opts.max_iterations)
                    .expect("review iteration is within the configured nonzero maximum"),
            },
            observer,
        )?;
        fixer_failed = refine_receipt["status"].as_str() == Some("failed");
        iterations.push(refine_receipt);
        if fixer_failed {
            // A failed fixer may have left partial edits behind, so refresh the
            // review evidence before reporting remaining findings.
            review_result = run_review_gates_with_observer(ctx, &opts.plan_id, &gates, observer)?;
            break;
        }
        review_result = run_review_gates_with_observer(ctx, &opts.plan_id, &gates, observer)?;
    }

    let remaining_findings = actionable_findings(&review_result)?;
    let has_check_gates = ctx
        .work_gates()
        .iter()
        .any(|gate| matches!(gate, WorkGate::Check(_) | WorkGate::Evidence(_)));
    let check_result = if !has_check_gates {
        None
    } else {
        // Refinement verifies the full configured check gate set, even when the
        // review gate subset was narrowed with --gate.
        Some(match check_execution {
            FinalCheckExecution::WaitForLease => {
                check_configured_collect_failures_with_observer(ctx, &opts.plan_id, observer)
            }
            FinalCheckExecution::RejectContention => {
                check_configured_collect_failures_from_mcp_with_observer(
                    ctx,
                    &opts.plan_id,
                    observer,
                )
            }
        }?)
    };
    let failed_review_gates = review_failed_gates(&review_result)?;
    let checks_ok = check_result
        .as_ref()
        .map(checks_passed)
        .transpose()?
        .unwrap_or(true);
    let status = if remaining_findings.is_empty()
        && failed_review_gates.is_empty()
        && checks_ok
        && !fixer_failed
        && !refinement_required
    {
        "passed"
    } else {
        "failed"
    };

    Ok(json!({
        "ok": true,
        "command": "work refine",
        "plan_id": opts.plan_id,
        "status": status,
        "iterations": iterations,
        "review": review_result,
        "checks": check_result,
        "fixer_failed": fixer_failed,
        "refinement_required": refinement_required,
        "failed_review_gates": failed_review_gates,
        "remaining_actionable_findings": remaining_findings,
    }))
}

pub(super) fn run_review_gates_with_observer(
    ctx: &RepoContext,
    plan_id: &str,
    gates: &[WorkReviewGate],
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let mut reviews = Vec::with_capacity(gates.len());
    let mut failed = Vec::new();
    for (index, gate) in gates.iter().enumerate() {
        let position = PhasePosition::new(index + 1, gates.len())
            .expect("review gates are enumerated within a nonempty list");
        let review = run_review_gate(ctx, plan_id, gate, position, observer)?;
        if review["status"].as_str() != Some("passed") {
            failed.push(gate.id.clone());
        }
        reviews.push(review);
    }

    Ok(json!({
        "ok": true,
        "command": "work review",
        "plan_id": plan_id,
        "status": if failed.is_empty() { "passed" } else { "failed" },
        "failed_gates": failed,
        "reviews": reviews,
    }))
}

fn run_review_gate(
    ctx: &RepoContext,
    plan_id: &str,
    gate: &WorkReviewGate,
    position: PhasePosition,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let skill = gate.skill.as_str();
    let threshold = gate.threshold;
    let schema = review_output_schema();
    let prompt = review_prompt(ctx, plan_id, gate);
    let schema_hash = hash_json(&schema)?;
    let prompt_hash = hash_text(&prompt);
    let started = now_ms();
    let before_fingerprint =
        current_worktree_fingerprint_with_cancellation(ctx, &|| observer.cancelled())?;
    let command_output =
        run_codex_review(ctx, plan_id, gate, &prompt, &schema, position, observer)?;
    let output = command_output.output;
    let ended = now_ms();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let parsed = match parse_review_output(&output, &stdout) {
        Ok(parsed) => parsed,
        Err(error) => {
            let parse_error = format!("{error:#}");
            return record_invalid_review_output(
                InvalidReviewOutputContext {
                    ctx,
                    plan_id,
                    gate,
                    started_at_ms: started,
                    ended_at_ms: ended,
                    stdout: &stdout,
                    stderr: &stderr,
                    prompt_hash: &prompt_hash,
                    schema_hash: &schema_hash,
                    codex_exit_status: output.status.code().unwrap_or(1),
                    worker_receipt_id: &command_output.worker_receipt_id,
                },
                &parse_error,
                observer,
            );
        }
    };
    let raw_finding_count = parsed.findings.len();
    let raw_actionable_count = parsed
        .findings
        .iter()
        .filter(|finding| {
            severity_rank(normalize_severity(&finding.severity)) >= severity_rank(threshold)
        })
        .count();
    let findings = normalize_findings(parsed.findings, &gate.id);
    let findings_truncated = raw_finding_count > findings.len();
    let actionable = findings
        .iter()
        .filter(|finding| finding_meets_threshold(finding, threshold))
        .cloned()
        .collect::<Vec<_>>();
    let actionable_findings_truncated = raw_actionable_count > actionable.len();
    let status = if raw_actionable_count == 0 && output.status.success() {
        "passed"
    } else {
        "failed"
    };
    let exit_status = if status == "passed" { 0 } else { 1 };
    let after_fingerprint =
        current_worktree_fingerprint_for_receipt_with_cancellation(ctx, &|| observer.cancelled());
    let evidence = json!({
        "kind": "codex_review",
        "schema_version": REVIEW_SCHEMA_VERSION,
        "plan_id": plan_id,
        "gate_id": gate.id,
        "skill": skill,
        "provider": "codex",
        "model": gate.model.as_deref(),
        "scope": gate.scope.as_str(),
        "threshold": threshold,
        "status": status,
        "worker_receipt_id": command_output.worker_receipt_id,
        "codex_exit_status": output.status.code().unwrap_or(1),
        "codex_stdout_preview": evidence_preview(&command_output.codex_stdout),
        "codex_stderr_preview": evidence_preview(&stderr),
        "summary": truncate_evidence_text(&parsed.summary),
        "prompt_hash": prompt_hash,
        "schema_hash": schema_hash,
        "worktree_fingerprint_before": before_fingerprint.fingerprint,
        "worktree_fingerprint_before_error": before_fingerprint.error,
        "worktree_fingerprint_after": after_fingerprint.fingerprint,
        "worktree_fingerprint_after_error": after_fingerprint.error,
        "raw_finding_count": raw_finding_count,
        "raw_actionable_count": raw_actionable_count,
        "findings_truncated": findings_truncated,
        "actionable_findings_truncated": actionable_findings_truncated,
        "findings": findings,
        "actionable_findings": actionable,
    });
    let receipt_id = record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_REVIEW,
            args: json!({
                "plan_id": plan_id,
                "gate_id": gate.id,
                "skill": skill,
                "threshold": threshold,
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status,
            stdout: &stdout,
            stderr: &stderr,
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    )?;

    Ok(json!({
        "gate_id": gate.id,
        "skill": skill,
        "status": status,
        "threshold": threshold,
        "receipt_id": receipt_id,
        "worker_receipt_id": evidence["worker_receipt_id"],
        "finding_count": raw_finding_count,
        "actionable_count": raw_actionable_count,
        "retained_finding_count": evidence["findings"].as_array().map(Vec::len).unwrap_or(0),
        "retained_actionable_count": evidence["actionable_findings"].as_array().map(Vec::len).unwrap_or(0),
        "findings_truncated": findings_truncated,
        "actionable_findings_truncated": actionable_findings_truncated,
        "findings": evidence["findings"],
        "actionable_findings": evidence["actionable_findings"],
    }))
}

struct InvalidReviewOutputContext<'a> {
    ctx: &'a RepoContext,
    plan_id: &'a str,
    gate: &'a WorkReviewGate,
    started_at_ms: u64,
    ended_at_ms: u64,
    stdout: &'a str,
    stderr: &'a str,
    prompt_hash: &'a str,
    schema_hash: &'a str,
    codex_exit_status: i32,
    worker_receipt_id: &'a str,
}

fn record_invalid_review_output(
    context: InvalidReviewOutputContext<'_>,
    parse_error: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let InvalidReviewOutputContext {
        ctx,
        plan_id,
        gate,
        started_at_ms,
        ended_at_ms,
        stdout,
        stderr,
        prompt_hash,
        schema_hash,
        codex_exit_status,
        worker_receipt_id,
    } = context;
    let skill = gate.skill.as_str();
    let threshold = gate.threshold;
    let receipt_stderr = if stderr.is_empty() {
        parse_error.to_string()
    } else {
        format!("{stderr}\n{parse_error}")
    };
    let evidence = json!({
        "kind": "codex_review",
        "schema_version": REVIEW_SCHEMA_VERSION,
        "plan_id": plan_id,
        "gate_id": gate.id,
        "skill": skill,
        "provider": "codex",
        "model": gate.model.as_deref(),
        "scope": gate.scope.as_str(),
        "threshold": threshold,
        "status": "invalid_output",
        "worker_receipt_id": worker_receipt_id,
        "codex_exit_status": codex_exit_status,
        "codex_stdout_preview": evidence_preview(stdout),
        "codex_stderr_preview": evidence_preview(stderr),
        "prompt_hash": prompt_hash,
        "schema_hash": schema_hash,
        "parse_error": evidence_preview(parse_error),
        "findings": [],
        "actionable_findings": [],
    });
    let receipt_id = record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_REVIEW,
            args: json!({
                "plan_id": plan_id,
                "gate_id": gate.id,
                "skill": skill,
                "threshold": threshold,
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms,
            ended_at_ms,
            exit_status: 2,
            stdout,
            stderr: &receipt_stderr,
            evidence: Some(evidence),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    )?;

    Ok(json!({
        "gate_id": gate.id,
        "skill": skill,
        "status": "invalid_output",
        "threshold": threshold,
        "receipt_id": receipt_id,
        "worker_receipt_id": worker_receipt_id,
        "finding_count": 0,
        "actionable_count": 0,
        "retained_finding_count": 0,
        "retained_actionable_count": 0,
        "findings_truncated": false,
        "actionable_findings_truncated": false,
        "findings": [],
        "actionable_findings": [],
        "parse_error": evidence_preview(parse_error),
    }))
}

struct RefinementIteration<'a> {
    plan_id: &'a str,
    iteration: usize,
    gates: &'a [WorkReviewGate],
    refinement: Option<&'a WorkRefinementConfig>,
    findings: &'a [Value],
    position: PhasePosition,
}

fn run_fixer(
    ctx: &RepoContext,
    request: RefinementIteration<'_>,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let RefinementIteration {
        plan_id,
        iteration,
        gates,
        refinement,
        findings,
        position,
    } = request;
    let started = now_ms();
    let prompt = refine_prompt(plan_id, iteration, gates, refinement, findings);
    let phase_label = format!("refinement iteration {iteration}");
    let before_fingerprint =
        current_worktree_fingerprint_with_cancellation(ctx, &|| observer.cancelled())?;
    let command_output = run_codex_refine(
        ctx,
        plan_id,
        &prompt,
        refinement
            .and_then(|refinement| refinement.model.as_deref())
            .or_else(|| gates.first().and_then(|gate| gate.model.as_deref())),
        &phase_label,
        position,
        observer,
    )
    .context("Failed to run Codex refinement")?;
    let output = command_output.output;
    let ended = now_ms();
    let after_fingerprint =
        current_worktree_fingerprint_for_receipt_with_cancellation(ctx, &|| observer.cancelled());
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let evidence = json!({
        "kind": "codex_refine",
        "schema_version": REVIEW_SCHEMA_VERSION,
        "plan_id": plan_id,
        "iteration": iteration,
        "provider": "codex",
        "worker_receipt_id": command_output.worker_receipt_id,
        "prompt_hash": hash_text(&prompt),
        "refinement_id": refinement.map(|refinement| refinement.id.as_str()),
        "refinement_skill": refinement.and_then(|refinement| refinement.skill.as_deref()),
        "refinement_mode": refinement.and_then(|refinement| refinement.mode.as_deref()),
        "gate_ids": gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>(),
        "worktree_fingerprint_before": before_fingerprint.fingerprint,
        "worktree_fingerprint_before_error": before_fingerprint.error,
        "worktree_fingerprint_after": after_fingerprint.fingerprint,
        "worktree_fingerprint_after_error": after_fingerprint.error,
        "finding_fingerprints": findings
            .iter()
            .filter_map(|finding| finding.get("fingerprint").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        "finding_count": findings.len(),
    });
    let receipt_id = record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_REFINE,
            args: json!({
                "plan_id": plan_id,
                "iteration": iteration,
                "gates": gates.iter().map(|gate| gate.id.as_str()).collect::<Vec<_>>(),
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status,
            stdout: &stdout,
            stderr: &stderr,
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    )?;

    Ok(json!({
        "iteration": iteration,
        "status": if output.status.success() { "passed" } else { "failed" },
        "receipt_id": receipt_id,
        "worker_receipt_id": evidence["worker_receipt_id"],
        "exit_status": exit_status,
        "finding_count": findings.len(),
    }))
}

fn selected_review_gates(
    ctx: &RepoContext,
    explicit_gates: &[String],
) -> Result<Vec<WorkReviewGate>> {
    let gates = ctx
        .work_gates()
        .into_iter()
        .filter_map(|gate| match gate {
            WorkGate::CodexReview(gate) => Some(gate),
            WorkGate::Evidence(_) => None,
            _ => None,
        })
        .collect::<Vec<_>>();
    if gates.is_empty() {
        bail!(
            "No codex_review work gates configured. Add [[work.gates]] entries with kind = \"codex_review\"."
        );
    }
    if explicit_gates.is_empty() {
        return Ok(gates);
    }

    let mut selected = Vec::with_capacity(explicit_gates.len());
    for id in explicit_gates {
        if selected.iter().any(|gate: &WorkReviewGate| gate.id == *id) {
            bail!("Duplicate codex_review work gate requested with id '{id}'");
        }
        let gate = gates
            .iter()
            .find(|gate| gate.id == *id)
            .ok_or_else(|| anyhow!("No codex_review work gate configured with id '{id}'"))?;
        selected.push(gate.clone());
    }
    Ok(selected)
}
