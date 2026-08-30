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
    OccurrenceClaim, OccurrenceFinish, OccurrenceGuard, OccurrenceOutcome, OccurrenceStatus,
    OccurrenceStore, ScheduleOccurrence,
};
use super::workflow::{
    ResolvedWorkflow, TuningOverrides, WorkflowRunPolicy, list_workflows, loop_status_is_success,
    resolve_workflow,
};

mod attention;
mod cron;
mod policy;

use attention::DispatchAttention;
pub(super) use cron::ScheduleSpec;
use policy::{
    DispatchStep, DispatchSummary, RunSummary, RunTickDisposition, TerminalDetails, begin_execution,
};

#[cfg(test)]
pub(super) fn dispatch_due_at(ctx: &RepoContext, dispatch_at_ms: u64) -> Result<Value> {
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
    let mut occurrences = OccurrenceStore::new(ctx);
    let reconciled = occurrences.reconcile_stale()?;
    let known_occurrences = occurrences.snapshot()?;
    let mut actions = Vec::new();
    let mut summary = DispatchSummary::default();

    for workflow in workflows {
        let step = dispatch_workflow(
            ctx,
            &mut occurrences,
            &known_occurrences,
            &workflow,
            dispatch_at_ms,
            observer,
        );
        summary.include(&step);
        if let Some(action) = step.action {
            actions.push(action);
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
        "reconciled_occurrences": evidence["reconciled_occurrences"],
        "actions": evidence["actions"],
    }))
}

fn dispatch_workflow(
    ctx: &RepoContext,
    occurrences: &mut OccurrenceStore,
    known_occurrences: &[ScheduleOccurrence],
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
    let latest = OccurrenceStore::latest_for_workflow(known_occurrences, &workflow.id);
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
    let claim = match occurrences.claim(&workflow.id, due_at_ms, workflow.lease_ttl_seconds) {
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
        OccurrenceClaim::Acquired(claim) => claim,
    };

    let guard = match begin_execution(&mut step, || {
        OccurrenceGuard::start(occurrences.clone(), &claim, workflow.lease_ttl_seconds)
    }) {
        Ok(guard) => guard,
        Err(error) => {
            let error = format!("Failed to renew scheduled occurrence: {error:#}");
            step.failed_count = 1;
            step.action = Some(
                match occurrences.finish(
                    &claim.occurrence_id,
                    &claim.owner,
                    OccurrenceFinish {
                        outcome: OccurrenceOutcome::Failed,
                        worker_receipt_id: None,
                        worktree: None,
                        error: Some(&error),
                    },
                ) {
                    Ok(record) => dispatch_action(&record, "failed", window.next_at_ms, None),
                    Err(finish_error) => dispatch_state_failure(
                        workflow,
                        &claim,
                        window.next_at_ms,
                        None,
                        format!("{error}; recording the failure also failed: {finish_error:#}"),
                    ),
                },
            );
            return step;
        }
    };
    let occurrence_cancelled = || guard.renewal_failed();
    let mut occurrence_control =
        AdditionalCancellationControl::new(observer, &occurrence_cancelled);
    let tick = tick_scheduled_with_observer(
        ctx,
        &workflow.id,
        &claim.occurrence_id,
        &mut occurrence_control,
    );
    step.state_errors = scheduled_tick_state_errors(&tick, &workflow.id, &claim.occurrence_id);
    if tick.as_ref().is_ok_and(ScheduledTick::lease_was_held) {
        return match guard.abandon_unexecuted() {
            Ok(finalization) => {
                include_occurrence_renewal_error(
                    &mut step,
                    &workflow.id,
                    &claim.occurrence_id,
                    finalization.renewal_error,
                );
                let abandoned = finalization.occurrence;
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
            include_occurrence_renewal_error(
                &mut step,
                &workflow.id,
                &claim.occurrence_id,
                finalization.renewal_error,
            );
            let record = finalization.occurrence;
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

fn scheduled_tick_state_errors(
    tick: &Result<ScheduledTick>,
    workflow_id: &str,
    occurrence_id: &str,
) -> Vec<Value> {
    let Ok(tick) = tick else {
        return Vec::new();
    };
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

fn tick_value(tick: &Result<ScheduledTick>) -> Option<Value> {
    tick.as_ref().ok().and_then(ScheduledTick::value).cloned()
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
