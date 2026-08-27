use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::command::WorkCheckRequest;
use crate::context::RepoContext;
use crate::execution::{ExecutionControl, PhasePosition};
use crate::state::{
    ReceiptInput, ReusableWorkCheckEvidence, ReusableWorkCheckQuery, WORK_CHECK_EVIDENCE_SCHEMA,
    WorkCheckBatchEvidence, WorkCheckGateEvidence,
    current_worktree_fingerprint_for_receipt_with_cancellation,
    current_worktree_fingerprint_with_cancellation, now_ms, record_receipt,
    record_receipt_with_cancellation, reusable_work_check_evidence_batch_with_cancellation,
};
use crate::tool_defs::tool;

use super::super::tool_execution::{ManifestToolExecutionOutcome, manifest_tool_result_failure};
use super::scope::{GateScopeEvaluation, PlanGateContext};
use super::tools::{SelectedCheck, selected_checks, validate_check_tool};

const EMPTY_CHECK_SELECTION_MESSAGE: &str = "No work checks are selected. Configure a check gate, select a gate with --gate, or select an execution tool with --tool.";

pub(super) fn check_with_observer(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    check_selected_with_observer(
        ctx,
        &opts.plan_id,
        selected_checks(ctx, &opts.gates, &opts.tools)?,
        observer,
    )
}

#[cfg(test)]
pub(in crate::runtime) fn check_tools_collect_failures_with_observer(
    ctx: &RepoContext,
    plan_id: &str,
    tools: Vec<String>,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    // Used by review refinement so failed verification checks are reported in
    // the refine result instead of aborting before all receipts are recorded.
    check_selected_with_failure_mode(
        ctx,
        plan_id,
        tools.into_iter().map(SelectedCheck::Tool).collect(),
        FailureMode::Collect,
        observer,
    )
}

pub(in crate::runtime) fn check_required_collect_failures_with_observer(
    ctx: &RepoContext,
    plan_id: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<Option<Value>> {
    let selected = selected_checks(ctx, &[], &[])?;
    if selected.is_empty() {
        return Ok(None);
    }
    check_selected_with_failure_mode(ctx, plan_id, selected, FailureMode::Collect, observer)
        .map(Some)
}

fn check_selected_with_observer(
    ctx: &RepoContext,
    plan_id: &str,
    selected: Vec<SelectedCheck>,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    check_selected_with_failure_mode(ctx, plan_id, selected, FailureMode::Abort, observer)
}

fn check_selected_with_failure_mode(
    ctx: &RepoContext,
    plan_id: &str,
    selected: Vec<SelectedCheck>,
    failure_mode: FailureMode,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    if selected.is_empty() {
        bail!(EMPTY_CHECK_SELECTION_MESSAGE);
    }
    let started = now_ms();
    let batch = prepare_check_batch(ctx, plan_id, selected, observer)?;
    let outcome = execute_check_batch(ctx, plan_id, &batch, failure_mode, observer)?;
    let receipt_result =
        record_check_batch_receipt(ctx, plan_id, started, &batch, &outcome, observer);

    if let Some(failure) = outcome.failure {
        return match receipt_result {
            Ok(_) => Err(failure.error),
            Err(receipt_error) => {
                bail!(
                    "{:#}\nwork check batch receipt recording also failed:\n{receipt_error:#}",
                    failure.error
                )
            }
        };
    }
    let receipt_id = receipt_result?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "checks": outcome.results,
        "change_evidence": batch.changes.to_value(),
        "gate_evidence": outcome.gate_evidence,
        "receipt_id": receipt_id,
    }))
}

#[derive(Clone, Copy)]
enum FailureMode {
    Abort,
    Collect,
}

impl FailureMode {
    fn aborts(self) -> bool {
        matches!(self, Self::Abort)
    }
}

struct PreparedCheckBatch {
    checks: Vec<PreparedCheck>,
    selected_tools: Vec<String>,
    selected_gate_ids: Vec<String>,
    initial_gate_scopes: Vec<(crate::context::WorkCheckGate, GateScopeEvaluation)>,
    runnable_count: usize,
    changes: BatchChanges,
    before_fingerprint: crate::state::CurrentWorktreeFingerprint,
}

#[derive(Default)]
struct BatchChanges {
    paths: Vec<String>,
    path_count: usize,
    paths_truncated: bool,
    paths_digest: Option<String>,
}

impl BatchChanges {
    fn from_checks(checks: &[PreparedCheck]) -> Self {
        checks
            .iter()
            .find_map(|check| match check {
                PreparedCheck::Gate { scope, .. } if scope.changed_paths_digest().is_some() => {
                    Some(Self {
                        paths: scope.changed_paths().to_vec(),
                        path_count: scope.changed_path_count(),
                        paths_truncated: scope.changed_paths_truncated(),
                        paths_digest: scope.changed_paths_digest().map(str::to_string),
                    })
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    fn to_value(&self) -> Value {
        json!({
            "changed_paths": self.paths,
            "changed_path_count": self.path_count,
            "changed_paths_truncated": self.paths_truncated,
            "changed_paths_digest": self.paths_digest,
        })
    }
}

struct CheckFailure {
    exit_status: i32,
    error: anyhow::Error,
}

struct BatchExecutionOutcome {
    results: Vec<Value>,
    gate_evidence: Vec<WorkCheckGateEvidence>,
    failure: Option<CheckFailure>,
}

#[derive(Clone, Copy)]
struct RunnableCheck<'a> {
    name: &'a str,
    gate_id: Option<&'a str>,
    gate: Option<GateRun<'a>>,
}

#[derive(Clone, Copy)]
struct GateRun<'a> {
    gate: &'a crate::context::WorkCheckGate,
    forced: bool,
    scope: &'a GateScopeEvaluation,
}

enum PreparedCheckAction<'a> {
    Evidence(WorkCheckGateEvidence),
    Abort {
        evidence: WorkCheckGateEvidence,
        failure: CheckFailure,
    },
    Run(RunnableCheck<'a>),
}

struct CheckRunOutcome {
    result: Option<Value>,
    gate_evidence: Option<WorkCheckGateEvidence>,
    failure: Option<CheckFailure>,
}

fn prepare_check_batch(
    ctx: &RepoContext,
    plan_id: &str,
    selected: Vec<SelectedCheck>,
    observer: &dyn ExecutionControl,
) -> Result<PreparedCheckBatch> {
    for selected in &selected {
        validate_check_tool(ctx, selected.tool(), "Work check")?;
    }
    let before_fingerprint =
        current_worktree_fingerprint_with_cancellation(ctx, &|| observer.cancelled())?;
    let plan_scope =
        PlanGateContext::load_with_cancellation(ctx, plan_id, &|| observer.cancelled())?;
    plan_scope.seed_legacy_fingerprint(before_fingerprint.clone());
    let selected_tools = selected
        .iter()
        .map(|selected| selected.tool().to_string())
        .collect::<Vec<_>>();
    let mut prepared = selected
        .into_iter()
        .map(|selected| -> Result<PreparedCheck> {
            Ok(match selected {
                SelectedCheck::Gate { gate, force } => {
                    let scope =
                        plan_scope.evaluate_with_cancellation(ctx, &gate, &|| observer.cancelled());
                    PreparedCheck::Gate {
                        gate,
                        force,
                        scope: Box::new(scope),
                        reusable: None,
                    }
                }
                SelectedCheck::Tool(tool) => PreparedCheck::Tool(tool),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let selected_gate_ids = prepared
        .iter()
        .filter_map(|check| match check {
            PreparedCheck::Gate { gate, .. } => Some(gate.id.clone()),
            PreparedCheck::Tool(_) => None,
        })
        .collect::<Vec<_>>();
    let reuse_queries = prepared
        .iter()
        .filter_map(|check| match check {
            PreparedCheck::Gate {
                gate,
                force: false,
                scope,
                ..
            } if gate.reuse && scope.is_known_applicable() => {
                scope
                    .scope_fingerprint()
                    .map(|scope_fingerprint| ReusableWorkCheckQuery {
                        gate_id: gate.id.clone(),
                        tool: gate.tool.clone(),
                        gate_signature: scope.gate_signature().to_string(),
                        scope_fingerprint: scope_fingerprint.to_string(),
                    })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut reusable = reusable_work_check_evidence_batch_with_cancellation(
        ctx,
        plan_id,
        &reuse_queries,
        &|| observer.cancelled(),
    )?;
    for check in &mut prepared {
        if let PreparedCheck::Gate {
            gate,
            reusable: selected_reuse,
            ..
        } = check
        {
            *selected_reuse = reusable.remove(&gate.id);
        }
    }
    let initial_gate_scopes = prepared
        .iter()
        .filter_map(|check| match check {
            PreparedCheck::Gate { gate, scope, .. } => Some((gate.clone(), scope.as_ref().clone())),
            PreparedCheck::Tool(_) => None,
        })
        .collect::<Vec<_>>();
    let runnable_count = prepared.iter().filter(|check| check.should_run()).count();
    let changes = BatchChanges::from_checks(&prepared);

    Ok(PreparedCheckBatch {
        checks: prepared,
        selected_tools,
        selected_gate_ids,
        initial_gate_scopes,
        runnable_count,
        changes,
        before_fingerprint,
    })
}

fn execute_check_batch(
    ctx: &RepoContext,
    plan_id: &str,
    batch: &PreparedCheckBatch,
    failure_mode: FailureMode,
    observer: &mut dyn ExecutionControl,
) -> Result<BatchExecutionOutcome> {
    let mut results = Vec::with_capacity(batch.runnable_count);
    let mut gate_evidence = Vec::new();
    let mut failure = None;
    let mut runnable_index = 0;
    for check in &batch.checks {
        let runnable = match classify_prepared_check(check, failure_mode) {
            PreparedCheckAction::Evidence(evidence) => {
                gate_evidence.push(evidence);
                continue;
            }
            PreparedCheckAction::Abort {
                evidence,
                failure: check_failure,
            } => {
                gate_evidence.push(evidence);
                failure = Some(check_failure);
                break;
            }
            PreparedCheckAction::Run(runnable) => runnable,
        };
        if observer.cancelled() {
            failure = Some(CheckFailure {
                exit_status: 1,
                error: anyhow!("Work check was cancelled before {} started", runnable.name),
            });
            break;
        }
        runnable_index += 1;
        let position = PhasePosition::new(runnable_index, batch.runnable_count)
            .expect("work checks are enumerated within a nonempty tool list");
        let check_outcome = run_check(ctx, plan_id, runnable, position, failure_mode, observer)?;
        if let Some(evidence) = check_outcome.gate_evidence {
            gate_evidence.push(evidence);
        }
        if let Some(result) = check_outcome.result {
            results.push(result);
        }
        if check_outcome.failure.is_some() {
            failure = check_outcome.failure;
            break;
        }
    }
    if let Some(failure) = failure.as_ref() {
        let interruption = format!(
            "gate did not complete because the work-check batch stopped: {:#}",
            failure.error
        );
        for check in &batch.checks {
            let PreparedCheck::Gate {
                gate, force, scope, ..
            } = check
            else {
                continue;
            };
            if gate_evidence
                .iter()
                .any(|evidence| evidence.gate_id == gate.id)
            {
                continue;
            }
            gate_evidence.push(gate_interruption_evidence(
                gate,
                "unknown",
                scope,
                None,
                failure.exit_status,
                *force,
                &interruption,
            ));
        }
    }

    Ok(BatchExecutionOutcome {
        results,
        gate_evidence,
        failure,
    })
}

fn classify_prepared_check(
    check: &PreparedCheck,
    failure_mode: FailureMode,
) -> PreparedCheckAction<'_> {
    let (gate, force, scope, reusable) = match check {
        PreparedCheck::Gate {
            gate,
            force,
            scope,
            reusable,
        } => (gate, force, scope, reusable),
        PreparedCheck::Tool(tool) => {
            return PreparedCheckAction::Run(RunnableCheck {
                name: tool,
                gate_id: None,
                gate: None,
            });
        }
    };

    if let Some(error) = scope.error()
        && !*force
    {
        let evidence = gate_evidence_from_scope(gate, "unknown", scope, None, None, *force, None);
        return if failure_mode.aborts() {
            PreparedCheckAction::Abort {
                evidence,
                failure: CheckFailure {
                    exit_status: 1,
                    error: anyhow!("Work gate '{}' applicability is unknown: {error}", gate.id),
                },
            }
        } else {
            PreparedCheckAction::Evidence(evidence)
        };
    }
    if !*force
        && scope.applicability() == Some(crate::git_receipts::GateApplicability::NotApplicable)
    {
        return PreparedCheckAction::Evidence(gate_evidence_from_scope(
            gate,
            "not_applicable",
            scope,
            None,
            None,
            false,
            None,
        ));
    }
    if let Some(reusable) = reusable {
        return PreparedCheckAction::Evidence(gate_evidence_from_scope(
            gate,
            "reused",
            scope,
            None,
            None,
            false,
            Some(reusable),
        ));
    }
    PreparedCheckAction::Run(RunnableCheck {
        name: &gate.tool,
        gate_id: Some(&gate.id),
        gate: Some(GateRun {
            gate,
            forced: *force,
            scope,
        }),
    })
}

fn run_check(
    ctx: &RepoContext,
    plan_id: &str,
    runnable: RunnableCheck<'_>,
    position: PhasePosition,
    failure_mode: FailureMode,
    observer: &mut dyn ExecutionControl,
) -> Result<CheckRunOutcome> {
    let execution = super::super::tool_execution::execute_manifest_tool_with_options_for_work_check(
        ctx,
        runnable.name,
        json!({}),
        Some(plan_id.to_string()),
        position,
        observer,
    );
    let (mut result, was_cancelled) = match execution {
        Ok(ManifestToolExecutionOutcome::Completed(result)) => (result, false),
        Ok(ManifestToolExecutionOutcome::Cancelled(result)) => (result, true),
        Err(error) => {
            let gate_evidence = runnable.gate.map(|gate| {
                gate_interruption_evidence(
                    gate.gate,
                    "failed",
                    gate.scope,
                    None,
                    1,
                    gate.forced,
                    &format!("gate execution could not start: {error:#}"),
                )
            });
            return Ok(CheckRunOutcome {
                result: None,
                gate_evidence,
                failure: Some(CheckFailure {
                    exit_status: 1,
                    error,
                }),
            });
        }
    };
    if let Some(gate_id) = runnable.gate_id
        && let Some(result) = result.as_object_mut()
    {
        result.insert("gate_id".into(), json!(gate_id));
    }
    let manifest_failure = manifest_tool_result_failure(&result)?;
    if was_cancelled && manifest_failure.is_none() {
        bail!("Cancelled tool returned a successful result");
    }
    let tool_receipt_id = result["receipt_id"].as_str().map(str::to_string);
    let gate_evidence = runnable.gate.map(|gate| {
        let status = if was_cancelled {
            "cancelled"
        } else if manifest_failure.is_some() {
            "failed"
        } else {
            "executed"
        };
        let exit_status = manifest_failure
            .as_ref()
            .map_or(0, |(exit_status, _)| *exit_status);
        if was_cancelled {
            let message = manifest_failure
                .as_ref()
                .map_or("gate execution was cancelled", |(_, message)| {
                    message.as_str()
                });
            gate_interruption_evidence(
                gate.gate,
                status,
                gate.scope,
                tool_receipt_id,
                exit_status,
                gate.forced,
                message,
            )
        } else {
            gate_evidence_from_scope(
                gate.gate,
                status,
                gate.scope,
                tool_receipt_id,
                Some(exit_status),
                gate.forced,
                None,
            )
        }
    });
    let failure = if was_cancelled {
        let (exit_status, message) =
            manifest_failure.expect("cancelled manifest tool outcome has a failure result");
        Some(CheckFailure {
            exit_status,
            error: anyhow!(message),
        })
    } else if failure_mode.aborts() {
        manifest_failure.map(|(exit_status, message)| CheckFailure {
            exit_status,
            error: anyhow!(message),
        })
    } else {
        None
    };

    Ok(CheckRunOutcome {
        result: Some(result),
        gate_evidence,
        failure,
    })
}

fn record_check_batch_receipt(
    ctx: &RepoContext,
    plan_id: &str,
    started: u64,
    batch: &PreparedCheckBatch,
    outcome: &BatchExecutionOutcome,
    observer: &dyn ExecutionControl,
) -> Result<String> {
    let receipt_ids = outcome
        .results
        .iter()
        .filter_map(|result| result["receipt_id"].as_str())
        .collect::<Vec<_>>();
    let scope_stability = revalidate_gate_scopes(ctx, plan_id, &batch.initial_gate_scopes, &|| {
        observer.cancelled()
    });
    let after_fingerprint =
        current_worktree_fingerprint_for_receipt_with_cancellation(ctx, &|| observer.cancelled());
    let worktree_fingerprint_override = Some(
        work_check_fingerprint_evidence(&batch.before_fingerprint, &after_fingerprint)
            .and_then(|fingerprint| scope_stability.map(|()| fingerprint)),
    );
    let receipt_stderr = outcome
        .failure
        .as_ref()
        .map(|failure| format!("{:#}", failure.error))
        .unwrap_or_default();
    let cancellation_active = observer.cancelled();
    let receipt_input = ReceiptInput {
        tool_name: tool::WORK_CHECK,
        args: json!({
            "plan_id": plan_id,
            "gates": batch.selected_gate_ids,
            "tools": batch.selected_tools,
            "receipt_ids": receipt_ids,
        }),
        invoked_command_key: None,
        plan_id: Some(plan_id.to_string()),
        started_at_ms: started,
        ended_at_ms: now_ms(),
        exit_status: outcome
            .failure
            .as_ref()
            .map_or(0, |failure| failure.exit_status),
        stdout: "",
        stderr: &receipt_stderr,
        evidence: Some(serde_json::to_value(WorkCheckBatchEvidence {
            schema: WORK_CHECK_EVIDENCE_SCHEMA.into(),
            changed_paths: batch.changes.paths.clone(),
            changed_path_count: batch.changes.path_count,
            changed_paths_truncated: batch.changes.paths_truncated,
            changed_paths_digest: batch.changes.paths_digest.clone(),
            gates: outcome.gate_evidence.clone(),
        })?),
        session_override: None,
        collect_git_metadata: !cancellation_active,
        collect_worktree_fingerprint: false,
        worktree_fingerprint_override,
    };
    if cancellation_active {
        // Cancellation is already authoritative, but its batch evidence still
        // has to supersede older passes. Append the small cleanup record
        // without starting fresh Git metadata collection.
        record_receipt(ctx, receipt_input)
    } else {
        record_receipt_with_cancellation(ctx, receipt_input, &|| observer.cancelled())
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::EMPTY_CHECK_SELECTION_MESSAGE;

    #[test]
    fn empty_selection_guidance_is_truthful_for_legacy_and_current_contracts() {
        assert!(EMPTY_CHECK_SELECTION_MESSAGE.contains("check gate"));
        assert!(EMPTY_CHECK_SELECTION_MESSAGE.contains("--gate"));
        assert!(EMPTY_CHECK_SELECTION_MESSAGE.contains("--tool"));
        assert!(!EMPTY_CHECK_SELECTION_MESSAGE.contains("required"));
        assert!(!EMPTY_CHECK_SELECTION_MESSAGE.contains("optional"));
    }
}

fn revalidate_gate_scopes(
    ctx: &RepoContext,
    plan_id: &str,
    initial: &[(crate::context::WorkCheckGate, GateScopeEvaluation)],
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<(), String> {
    if initial.is_empty() {
        return Ok(());
    }
    let final_context = PlanGateContext::load_with_cancellation(ctx, plan_id, cancelled)
        .map_err(|error| format!("Failed to reload work gate scopes after checks: {error:#}"))?;
    for (gate, before) in initial {
        let after = final_context.evaluate_with_cancellation(ctx, gate, cancelled);
        if &after != before {
            return Err(format!(
                "work gate '{}' scope changed during work check; rerun after repository inputs settle",
                gate.id
            ));
        }
    }
    Ok(())
}

enum PreparedCheck {
    Gate {
        gate: crate::context::WorkCheckGate,
        force: bool,
        scope: Box<GateScopeEvaluation>,
        reusable: Option<ReusableWorkCheckEvidence>,
    },
    Tool(String),
}

include!("checks/tail.rs");
