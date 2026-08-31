use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::command::{LoopDispatchRequest, LoopRunRequest, LoopTickRequest};
use crate::context::RepoContext;
#[cfg(test)]
use crate::execution::NoopExecutionObserver;
use crate::execution::{
    AdditionalCancellationControl, ExecutionControl, ExecutionPhase, PhasePosition,
};
use crate::state::{ReceiptInput, now_ms, record_receipt_with_cancellation};
use crate::tool_defs::LOOP_DISPATCH_TOOL;

use super::engine::{ScheduledTick, tick_scheduled_with_observer, tick_with_observer};
use super::occurrence::{
    OccurrenceAttentionScope, OccurrenceClaim, OccurrenceFinalization, OccurrenceFinish,
    OccurrenceGuard, OccurrenceStatus, OccurrenceStore, ScheduleOccurrence,
};
use super::state::prepare_coordination_state_for_dispatch;
use super::workflow::{
    CodexTaskCheckout, ResolvedWorkflow, TuningOverrides, WorkflowRunPolicy, list_workflows,
    loop_status_is_success, resolve_workflow,
};

mod attention;
mod cron;
mod policy;

use attention::DispatchAttention;
pub(super) use cron::ScheduleSpec;
use policy::{DispatchStep, DispatchSummary, RunSummary, RunTickDisposition, TerminalDetails};

#[cfg(test)]
pub(in crate::runtime) fn dispatch_due_at(ctx: &RepoContext, dispatch_at_ms: u64) -> Result<Value> {
    dispatch_due_at_with_observer(ctx, dispatch_at_ms, &mut NoopExecutionObserver)
}

pub(super) fn dispatch_due_with_observer(
    ctx: &RepoContext,
    _: LoopDispatchRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    dispatch_due_at_with_observer(ctx, now_ms(), observer)
}

fn dispatch_due_at_with_observer(
    ctx: &RepoContext,
    dispatch_at_ms: u64,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let workflows = list_workflows(ctx)?;
    super::pre_execution::require_ignored_loop_runtime_root(ctx, observer)?;
    let coordination_recovery = prepare_coordination_state_for_dispatch(ctx)?;
    let mut occurrences = OccurrenceStore::new(ctx);
    let reconciled = occurrences.reconcile_stale()?;
    let mut actions = Vec::new();
    let mut summary = DispatchSummary::default();
    if coordination_recovery.attempt_cache_reset {
        summary.include_state_errors(vec![json!({
            "kind": "attempts_reset",
            "error": "Malformed loop attempt coordination state was reset before dispatch",
        })]);
    }

    for workflow in workflows {
        let step = dispatch_workflow(ctx, &mut occurrences, &workflow, dispatch_at_ms, observer);
        let stop_for_repository_revision = step.repository_revision.requires_dispatch_stop();
        summary.include(&step);
        if let Some(action) = step.action {
            actions.push(action);
        }
        if stop_for_repository_revision {
            break;
        }
    }

    let attention = DispatchAttention::collect(ctx, &occurrences, &|| observer.cancelled());
    summary.needs_attention_count = attention.scheduled_occurrence_count;
    summary.exhausted_attempt_count = attention.exhausted_attempt_count;
    summary.include_state_errors(attention.state_errors);
    let state_error_text = summary.state_error_text();
    let status = summary.status();
    let ok = loop_status_is_success(status);
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_dispatch",
        "schema_version": 1,
        "dispatch_at_ms": dispatch_at_ms,
        "status": status,
        "due_count": summary.due_count,
        "executed_count": summary.executed_count,
        "deferred_count": summary.deferred_count,
        "skipped_count": summary.skipped_count,
        "failed_count": summary.failed_count,
        "needs_attention_count": summary.needs_attention_count,
        "exhausted_attempt_count": summary.exhausted_attempt_count,
        "state_error_count": summary.state_error_count,
        "state_errors": summary.state_errors,
        "repository_revision_changed": summary.repository_revision_changed,
        "reconciled_occurrences": reconciled,
        "actions": actions,
    });
    let receipt_id = record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: LOOP_DISPATCH_TOOL,
            args: json!({}),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: i32::from(!ok),
            stdout: "",
            stderr: &state_error_text,
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
        &|| observer.cancelled(),
    )?;
    Ok(json!({
        "ok": ok,
        "command": "loop dispatch",
        "receipt_id": receipt_id,
        "status": status,
        "dispatch_at_ms": dispatch_at_ms,
        "due_count": summary.due_count,
        "executed_count": summary.executed_count,
        "deferred_count": summary.deferred_count,
        "skipped_count": summary.skipped_count,
        "failed_count": summary.failed_count,
        "needs_attention_count": summary.needs_attention_count,
        "exhausted_attempt_count": summary.exhausted_attempt_count,
        "state_error_count": summary.state_error_count,
        "state_errors": evidence["state_errors"],
        "repository_revision_changed": evidence["repository_revision_changed"],
        "reconciled_occurrences": evidence["reconciled_occurrences"],
        "actions": evidence["actions"],
    }))
}

fn dispatch_workflow(
    ctx: &RepoContext,
    occurrences: &mut OccurrenceStore,
    workflow: &ResolvedWorkflow,
    dispatch_at_ms: u64,
    observer: &mut dyn ExecutionControl,
) -> DispatchStep {
    let Some(schedule) = workflow.schedule.as_ref() else {
        return DispatchStep::default();
    };
    if !workflow.enabled {
        return DispatchStep::action(json!({
            "workflow_id": workflow.id,
            "status": "disabled",
        }));
    }
    // Reconciliation above establishes/migrates the authoritative ledger. The
    // window projection only needs an atomic snapshot; taking both schedule
    // locks here would turn every configured workflow into another mutating
    // coordination pass before the claim's actual compare-and-set.
    let known_occurrences =
        match occurrences.snapshot_read_only_with_cancellation(&|| observer.cancelled()) {
            Ok(occurrences) => occurrences,
            Err(error) => {
                return DispatchStep::failure(
                    &workflow.id,
                    format!("Failed to refresh scheduled occurrence state: {error:#}"),
                );
            }
        };
    let latest = OccurrenceStore::latest_for_workflow(&known_occurrences, &workflow.id);
    let window = match schedule.window(
        dispatch_at_ms,
        latest.as_ref().map(|record| record.scheduled_at_ms),
    ) {
        Ok(window) => window,
        Err(error) => return DispatchStep::failure(&workflow.id, format!("{error:#}")),
    };
    let Some(due_at_ms) = window.due_at_ms else {
        return DispatchStep::action(json!({
            "workflow_id": workflow.id,
            "status": "not_due",
            "next_at_ms": window.next_at_ms,
        }));
    };
    let mut step = DispatchStep {
        due_count: 1,
        ..DispatchStep::default()
    };
    let blocks_on_retained_worktree = workflow.blocks_on_retained_worktree();
    let attention_scope = if workflow
        .codex_task
        .as_ref()
        .is_some_and(|task| task.checkout == CodexTaskCheckout::Repo)
    {
        OccurrenceAttentionScope::SharedRepository
    } else {
        OccurrenceAttentionScope::Workflow
    };
    let claim = match occurrences.claim_scheduled(
        &workflow.id,
        due_at_ms,
        workflow.lease_ttl_seconds,
        attention_scope,
        blocks_on_retained_worktree,
    ) {
        Ok(claim) => claim,
        Err(error) => {
            step.failed_count = 1;
            step.action = DispatchStep::failure(&workflow.id, format!("{error:#}")).action;
            return step;
        }
    };
    let claim = match claim {
        OccurrenceClaim::AlreadyRecorded(record) => {
            step.skipped_count = 1;
            step.action = Some(json!({
                "workflow_id": workflow.id,
                "occurrence": record,
                "status": "already_recorded",
                "next_at_ms": window.next_at_ms,
            }));
            return step;
        }
        OccurrenceClaim::BlockedByAttention(record) => {
            step.skipped_count = 1;
            step.action = Some(json!({
                "workflow_id": workflow.id,
                "occurrence": record,
                "status": "needs_attention",
                "reason": "occurrence_requires_attention",
                "retryable": false,
                "next_at_ms": window.next_at_ms,
                "error": "A prior scheduled occurrence requires acknowledgement before this workflow can claim another occurrence",
            }));
            return step;
        }
        OccurrenceClaim::BlockedByRunning(record) => {
            step.skipped_count = 1;
            step.deferred_count = 1;
            step.action = Some(json!({
                "workflow_id": workflow.id,
                "occurrence": record,
                "status": "deferred",
                "reason": "occurrence_in_progress",
                "retryable": true,
                "next_at_ms": window.next_at_ms,
                "error": "A live occurrence still owns the workflow or shared repository execution scope",
            }));
            return step;
        }
        OccurrenceClaim::BlockedByRetainedWorktree(record) => {
            step.skipped_count = 1;
            step.failed_count = 1;
            step.action = Some(json!({
                "workflow_id": workflow.id,
                "occurrence": record,
                "status": "failed",
                "reason": "retained_worktree_requires_cleanup",
                "retryable": true,
                "next_at_ms": window.next_at_ms,
                "error": "A retained task worktree must be removed before this workflow can run another scheduled occurrence",
            }));
            return step;
        }
        OccurrenceClaim::Acquired(claim) => claim,
    };

    let guard =
        match OccurrenceGuard::start(occurrences.clone(), &claim, workflow.lease_ttl_seconds) {
            Ok(guard) => guard,
            Err(error) => {
                return abandon_unexecuted_start_failure(
                    step,
                    occurrences,
                    workflow,
                    &claim,
                    window.next_at_ms,
                    format!("Failed to renew scheduled occurrence: {error:#}"),
                );
            }
        };
    let occurrence_cancelled = || guard.renewal_failed();
    let worktree_reservation = guard.worktree_reservation();
    let mut occurrence_control =
        AdditionalCancellationControl::new(observer, &occurrence_cancelled);
    let tick = match tick_scheduled_with_observer(
        ctx,
        &workflow.id,
        &claim.occurrence_id,
        worktree_reservation,
        &mut occurrence_control,
    ) {
        Ok(tick) => tick,
        Err(error) => {
            return abandon_unexecuted_tick_failure(
                step,
                guard,
                workflow,
                &claim,
                UnexecutedDispatchDetails {
                    next_at_ms: window.next_at_ms,
                    reason: "pre_execution_error",
                    tick: None,
                    error: format!("{error:#}"),
                },
            );
        }
    };
    step.repository_revision = tick.completion().repository_revision;
    step.state_errors = scheduled_tick_state_errors(&tick, &workflow.id, &claim.occurrence_id);
    if tick.workflow_was_unexecuted() && tick.completion().worktree.is_none() {
        let error = tick
            .completion()
            .error
            .clone()
            .unwrap_or_else(|| "Workflow was cancelled before execution started".into());
        let reason = tick
            .unexecuted_reason()
            .expect("an unexecuted tick carries its phase reason")
            .as_str();
        let tick = tick_value(&tick);
        return abandon_unexecuted_tick_failure(
            step,
            guard,
            workflow,
            &claim,
            UnexecutedDispatchDetails {
                next_at_ms: window.next_at_ms,
                reason,
                tick,
                error,
            },
        );
    }
    step.executed_count = u64::from(!tick.workflow_was_unexecuted());
    if tick.lease_was_held() {
        return match guard.abandon_unexecuted() {
            Ok(finalization) => {
                let abandoned = occurrence_from_finalization(
                    &mut step,
                    &workflow.id,
                    &claim.occurrence_id,
                    finalization,
                    true,
                );
                step.executed_count = 0;
                step.skipped_count = 1;
                step.deferred_count = 1;
                step.action = Some(json!({
                    "workflow_id": workflow.id,
                    "occurrence": abandoned,
                    "status": "deferred",
                    "reason": "workflow_lease_held",
                    "next_at_ms": window.next_at_ms,
                    "tick": tick_value(&tick),
                }));
                step
            }
            Err(error) => {
                step.failed_count = 1;
                step.action = Some(dispatch_state_failure(
                    workflow,
                    &claim,
                    window.next_at_ms,
                    tick_value(&tick),
                    format!("Failed to abandon deferred occurrence: {error:#}"),
                ));
                step
            }
        };
    }
    let details = TerminalDetails::from_tick(&tick);
    match guard.finish(OccurrenceFinish {
        outcome: details.outcome,
        worker_receipt_id: details.worker_receipt_id.as_deref(),
        worktree: details.worktree.as_deref(),
        error: details.error.as_deref(),
    }) {
        Ok(finalization) => {
            let record = occurrence_from_finalization(
                &mut step,
                &workflow.id,
                &claim.occurrence_id,
                finalization,
                false,
            );
            step.failed_count = u64::from(record.status == OccurrenceStatus::Failed);
            step.action = Some(dispatch_action(
                &record,
                record.status.as_str(),
                window.next_at_ms,
                tick_value(&tick),
            ));
        }
        Err(finish_error) => {
            step.failed_count = 1;
            step.action = Some(dispatch_state_failure(
                workflow,
                &claim,
                window.next_at_ms,
                tick_value(&tick),
                format!("Failed to finish scheduled occurrence: {finish_error:#}"),
            ));
        }
    }
    step
}

fn abandon_unexecuted_start_failure(
    mut step: DispatchStep,
    occurrences: &mut OccurrenceStore,
    workflow: &ResolvedWorkflow,
    claim: &ScheduleOccurrence,
    next_at_ms: u64,
    error: String,
) -> DispatchStep {
    match occurrences.abandon_unexecuted(&claim.occurrence_id, &claim.owner) {
        Ok(abandoned) => record_unexecuted_failure(
            &mut step,
            workflow,
            abandoned,
            next_at_ms,
            "pre_execution_error",
            None,
            error,
        ),
        Err(abandon_error) => {
            record_abandonment_failure(&mut step, workflow, claim, next_at_ms, error, abandon_error)
        }
    }
    step
}

struct UnexecutedDispatchDetails {
    next_at_ms: u64,
    reason: &'static str,
    tick: Option<Value>,
    error: String,
}

fn abandon_unexecuted_tick_failure(
    mut step: DispatchStep,
    guard: OccurrenceGuard,
    workflow: &ResolvedWorkflow,
    claim: &ScheduleOccurrence,
    details: UnexecutedDispatchDetails,
) -> DispatchStep {
    let UnexecutedDispatchDetails {
        next_at_ms,
        reason,
        tick,
        error,
    } = details;
    match guard.abandon_unexecuted() {
        Ok(finalization) => {
            let occurrence = occurrence_from_finalization(
                &mut step,
                &workflow.id,
                &claim.occurrence_id,
                finalization,
                true,
            );
            record_unexecuted_failure(
                &mut step, workflow, occurrence, next_at_ms, reason, tick, error,
            );
        }
        Err(abandon_error) => {
            record_abandonment_failure(&mut step, workflow, claim, next_at_ms, error, abandon_error)
        }
    }
    step
}

fn record_abandonment_failure(
    step: &mut DispatchStep,
    workflow: &ResolvedWorkflow,
    claim: &ScheduleOccurrence,
    next_at_ms: u64,
    error: String,
    abandon_error: anyhow::Error,
) {
    step.executed_count = 0;
    step.skipped_count = 1;
    step.failed_count = 1;
    step.action = Some(dispatch_state_failure(
        workflow,
        claim,
        next_at_ms,
        None,
        format!("{error}; abandoning the unexecuted occurrence also failed: {abandon_error:#}"),
    ));
}

fn record_unexecuted_failure(
    step: &mut DispatchStep,
    workflow: &ResolvedWorkflow,
    occurrence: ScheduleOccurrence,
    next_at_ms: u64,
    reason: &str,
    tick: Option<Value>,
    error: String,
) {
    step.executed_count = 0;
    step.skipped_count = 1;
    step.failed_count = 1;
    step.action = Some(json!({
        "workflow_id": workflow.id,
        "occurrence": occurrence,
        "status": "failed",
        "reason": reason,
        "retryable": true,
        "occurrence_state_persisted": false,
        "next_at_ms": next_at_ms,
        "tick": tick,
        "error": error,
    }));
}

fn include_occurrence_renewal_error(
    step: &mut DispatchStep,
    workflow_id: &str,
    occurrence_id: &str,
    renewal_error: Option<String>,
) {
    if let Some(error) = renewal_error {
        step.state_errors.push(json!({
            "kind": "occurrence_renewal",
            "error": format!("Occurrence renewal failed before terminal state was recorded: {error}"),
            "workflow_id": workflow_id,
            "occurrence_id": occurrence_id,
        }));
    }
}

fn occurrence_from_finalization(
    step: &mut DispatchStep,
    workflow_id: &str,
    occurrence_id: &str,
    finalization: OccurrenceFinalization,
    suppress_ownership_loss: bool,
) -> ScheduleOccurrence {
    let renewal_error = if suppress_ownership_loss && finalization.renewal_ownership_lost {
        None
    } else {
        finalization.renewal_error
    };
    include_occurrence_renewal_error(step, workflow_id, occurrence_id, renewal_error);
    finalization.occurrence
}

fn scheduled_tick_state_errors(
    tick: &ScheduledTick,
    workflow_id: &str,
    occurrence_id: &str,
) -> Vec<Value> {
    let mut state_errors = tick.state_errors().to_vec();
    for error in &mut state_errors {
        scope_state_error(error, workflow_id, occurrence_id);
    }
    if let Some(error) = tick.post_work_error()
        && !state_errors
            .iter()
            .any(|existing| existing["error"].as_str() == Some(error))
    {
        state_errors.push(json!({
            "kind": "tick",
            "error": error,
            "workflow_id": workflow_id,
            "occurrence_id": occurrence_id,
        }));
    }
    state_errors
}

fn scope_state_error(error: &mut Value, workflow_id: &str, occurrence_id: &str) {
    let Some(error) = error.as_object_mut() else {
        return;
    };
    error.insert("workflow_id".into(), workflow_id.into());
    error.insert("occurrence_id".into(), occurrence_id.into());
}

fn dispatch_state_failure(
    workflow: &ResolvedWorkflow,
    occurrence: &ScheduleOccurrence,
    next_at_ms: u64,
    tick: Option<Value>,
    error: impl std::fmt::Display,
) -> Value {
    json!({
        "workflow_id": workflow.id,
        "occurrence": occurrence,
        "status": "failed",
        "occurrence_state_persisted": false,
        "state_error": error.to_string(),
        "next_at_ms": next_at_ms,
        "tick": tick,
        "error": error.to_string(),
    })
}

fn tick_value(tick: &ScheduledTick) -> Option<Value> {
    tick.value().cloned()
}

fn dispatch_action(
    occurrence: &ScheduleOccurrence,
    status: &str,
    next_at_ms: u64,
    tick: Option<Value>,
) -> Value {
    json!({
        "workflow_id": occurrence.workflow_id,
        "occurrence": occurrence,
        "status": status,
        "next_at_ms": next_at_ms,
        "tick": tick,
    })
}

pub(super) fn run_until_with_observer(
    ctx: &RepoContext,
    request: LoopRunRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    if request.until != "idle" {
        bail!(
            "Unsupported loop run stop condition '{}'. Use --until idle.",
            request.until
        );
    }
    if request.max_ticks == 0 {
        bail!("--max-ticks must be greater than zero");
    }
    let workflow = resolve_workflow(
        ctx,
        request.workflow.as_deref(),
        TuningOverrides {
            lease_ttl_seconds: request.lease_ttl_seconds,
            max_attempts: request.max_attempts,
            backoff_seconds: request.backoff_seconds,
        },
    )?;
    if workflow.run_policy() == WorkflowRunPolicy::SingleTick {
        bail!(
            "Loop workflow '{}' runs one task per tick and does not support `loop run`; use `loop tick --workflow {}` for a manual run or `loop dispatch` for scheduled execution",
            workflow.id,
            workflow.id,
        );
    }

    let mut ticks = Vec::new();
    let mut summary = RunSummary::default();
    for index in 0..request.max_ticks {
        if observer.cancelled() {
            bail!("Loop execution was cancelled before the next tick started");
        }
        let position = PhasePosition::new((index + 1) as usize, request.max_ticks as usize)
            .expect("loop tick progress is within the configured nonzero maximum");
        let phase = ExecutionPhase::start(observer, "loop tick", position);
        let tick = tick_with_observer(
            ctx,
            LoopTickRequest {
                workflow: request.workflow.clone(),
                lease_ttl_seconds: request.lease_ttl_seconds,
                max_attempts: request.max_attempts,
                backoff_seconds: request.backoff_seconds,
            },
            observer,
        );
        phase.finish(observer, tick.is_ok());
        let tick = tick?;
        let disposition = RunTickDisposition::from_tick(&tick);
        ticks.push(tick);
        if summary.observe(disposition) {
            break;
        }
    }
    let status = summary.status();

    Ok(json!({
        "ok": loop_status_is_success(status),
        "command": "loop run",
        "until": request.until,
        "status": status,
        "tick_count": ticks.len(),
        "ticks": ticks,
    }))
}

#[cfg(test)]
mod tests;
