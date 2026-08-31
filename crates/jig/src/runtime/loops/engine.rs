use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::command::{LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopTickRequest};
use crate::context::RepoContext;
use crate::execution::{AdditionalCancellationControl, ExecutionControl};
use crate::state::{
    ReceiptInput, now_ms, record_receipt_with_cancellation, record_receipt_with_cancellation_until,
};
use crate::tool_defs::{LOOP_ACKNOWLEDGE_OCCURRENCE_TOOL, LOOP_CLEAR_ATTEMPT_TOOL, LOOP_TICK_TOOL};

use super::occurrence::{OccurrenceAcknowledgement, OccurrenceStore};
use super::state::{AttemptSections, AttemptStore, LeaseAcquire, LeaseGuard, LeaseStore};
use super::workflow::{
    CODEX_TASK_KIND, DEFAULT_WORKFLOW_ID, GITHUB_PR_STATUS_KIND, NOOP_STATUS_KIND, PR_MANAGER_KIND,
    ResolvedWorkflow, TuningOverrides, UnexecutedReason, WorkflowCompletion, WorkflowOutcome,
    WorkflowTick, list_workflows, loop_status_is_success, resolve_workflow,
};
use super::{codex_task, github, noop, pr_manager};

mod manual_occurrence;
mod runtime_state;
mod status;
mod unexecuted;

pub(super) use status::status_with_cancellation;
#[cfg(test)]
pub(super) use status::{status, status_at_with_cancellation};

use manual_occurrence::ManualOccurrenceGuard;
use runtime_state::{TickRuntimeState, append_tick_error};
use unexecuted::UnexecutedTickError;

struct TickExecution {
    item_key: String,
    manual: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkflowLeaseDisposition {
    NotAttempted,
    Acquired,
    Held,
}

pub(super) enum ScheduledTick {
    Reported {
        value: Value,
        completion: WorkflowCompletion,
        lease_disposition: WorkflowLeaseDisposition,
        state_errors: Vec<Value>,
    },
    Errored {
        value: Option<Value>,
        completion: WorkflowCompletion,
        lease_disposition: WorkflowLeaseDisposition,
        state_errors: Vec<Value>,
        error: String,
        post_work_error: Option<String>,
    },
}

impl ScheduledTick {
    pub(super) fn value(&self) -> Option<&Value> {
        match self {
            Self::Reported { value, .. } => Some(value),
            Self::Errored { value, .. } => value.as_ref(),
        }
    }

    pub(super) fn completion(&self) -> &WorkflowCompletion {
        match self {
            Self::Reported { completion, .. } | Self::Errored { completion, .. } => completion,
        }
    }

    pub(super) fn workflow_was_unexecuted(&self) -> bool {
        self.unexecuted_reason().is_some()
    }

    pub(super) fn unexecuted_reason(&self) -> Option<UnexecutedReason> {
        self.completion().execution.unexecuted_reason()
    }

    pub(super) fn post_work_error(&self) -> Option<&str> {
        match self {
            Self::Reported { .. } => None,
            Self::Errored {
                post_work_error, ..
            } => post_work_error.as_deref(),
        }
    }

    pub(super) fn lease_was_held(&self) -> bool {
        matches!(
            self,
            Self::Reported {
                lease_disposition: WorkflowLeaseDisposition::Held,
                ..
            } | Self::Errored {
                lease_disposition: WorkflowLeaseDisposition::Held,
                ..
            }
        )
    }

    pub(super) fn state_errors(&self) -> &[Value] {
        match self {
            Self::Reported { state_errors, .. } | Self::Errored { state_errors, .. } => {
                state_errors
            }
        }
    }

    fn into_manual_result(self) -> Result<Value> {
        match self {
            Self::Reported {
                value, completion, ..
            } if matches!(
                completion.execution.unexecuted_reason(),
                Some(
                    UnexecutedReason::BlockedByActiveOccurrence
                        | UnexecutedReason::BlockedByAttention
                )
            ) =>
            {
                Ok(value)
            }
            Self::Reported {
                value: _,
                completion,
                ..
            } if completion.execution.unexecuted_reason().is_some()
                && completion.worker_receipt_id.is_none() =>
            {
                Err(anyhow::anyhow!(completion.error.unwrap_or_else(|| {
                    "Workflow did not start execution".into()
                })))
            }
            Self::Reported { value, .. } => Ok(value),
            Self::Errored { error, .. } => Err(anyhow::anyhow!(error)),
        }
    }
}

pub(super) fn tick_with_observer(
    ctx: &RepoContext,
    request: LoopTickRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    tick_with_execution(
        ctx,
        request,
        started,
        TickExecution {
            item_key: started.to_string(),
            manual: true,
        },
        observer,
    )
    .map_err(UnexecutedTickError::into_inner)
    .and_then(ScheduledTick::into_manual_result)
}

pub(super) fn tick_scheduled_with_observer(
    ctx: &RepoContext,
    workflow_id: &str,
    occurrence_id: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<ScheduledTick, UnexecutedTickError> {
    tick_with_execution(
        ctx,
        LoopTickRequest {
            workflow: Some(workflow_id.to_string()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        },
        now_ms(),
        TickExecution {
            item_key: occurrence_id.to_string(),
            manual: false,
        },
        observer,
    )
}

fn tick_with_execution(
    ctx: &RepoContext,
    request: LoopTickRequest,
    started: u64,
    execution: TickExecution,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<ScheduledTick, UnexecutedTickError> {
    if observer.cancelled() {
        return Err(anyhow::anyhow!(
            "Loop execution was cancelled before workflow execution started"
        )
        .into());
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
    if execution.manual {
        super::pre_execution::require_ignored_loop_runtime_root(ctx, observer)?;
    }
    let mut lease_store = LeaseStore::new(ctx);
    let mut attempt_store = AttemptStore::new(ctx);

    let mut status = "idle";
    let mut idle = true;
    let mut lease = None;
    let mut lease_acquired = false;
    let mut lease_disposition = WorkflowLeaseDisposition::NotAttempted;
    let mut release_warning = None;
    let mut observed = Value::Null;
    let mut actions = Vec::new();
    let mut completion = WorkflowCompletion::default();
    let mut tick_error = None;
    let mut manual_occurrence = None;
    let mut manual_receipt_guard = None;

    if !workflow.enabled {
        status = "disabled";
    } else {
        let lease_key = workflow.lease_key();
        match lease_store.acquire(&lease_key, workflow.lease_ttl_seconds)? {
            LeaseAcquire::Acquired(acquired) => {
                lease_acquired = true;
                lease_disposition = WorkflowLeaseDisposition::Acquired;
                lease = Some(acquired.clone());
                let lease_guard = LeaseGuard::start(
                    lease_store.clone(),
                    &lease_key,
                    &acquired,
                    workflow.lease_ttl_seconds,
                )?;
                let mut manual_blocked = false;
                let manual_guard = if execution.manual {
                    match ManualOccurrenceGuard::start(&workflow, &execution.item_key, ctx) {
                        Ok(start) => {
                            let (guard, occurrence) =
                                start.prepare_tick(&mut actions, &mut completion);
                            manual_blocked = occurrence.is_some();
                            manual_occurrence = occurrence;
                            guard
                        }
                        Err(error) => {
                            let release_error = lease_guard.finish().err();
                            let error = release_error.map_or_else(
                                || format!("Failed to start manual loop occurrence: {error:#}"),
                                |release_error| {
                                    format!(
                                        "Failed to start manual loop occurrence: {error:#}; workflow lease release also failed: {release_error:#}"
                                    )
                                },
                            );
                            return Err(anyhow::anyhow!(error).into());
                        }
                    }
                } else {
                    None
                };
                let lease_cancelled = || lease_guard.renewal_failed();
                let manual_cancelled = || {
                    manual_guard
                        .as_ref()
                        .is_some_and(ManualOccurrenceGuard::renewal_failed)
                };
                if !manual_blocked {
                    let workflow_tick = {
                        let mut lease_control =
                            AdditionalCancellationControl::new(observer, &lease_cancelled);
                        let mut occurrence_control = AdditionalCancellationControl::new(
                            &mut lease_control,
                            &manual_cancelled,
                        );
                        run_workflow_tick(
                            ctx,
                            &workflow,
                            &execution,
                            &mut lease_store,
                            &mut attempt_store,
                            &mut occurrence_control,
                        )
                    };
                    match workflow_tick {
                        Ok(tick) => {
                            observed = tick.observed;
                            actions = tick.actions;
                            completion = tick.completion;
                        }
                        Err(error) => {
                            let error = format!("{error:#}");
                            completion = WorkflowCompletion {
                                outcome: WorkflowOutcome::Failed,
                                error: Some(error.clone()),
                                ..WorkflowCompletion::default()
                            };
                            tick_error = Some(error);
                        }
                    }
                }
                let released = lease_guard.finish();
                if let Err(error) = released {
                    let error = format!("{error:#}");
                    release_warning = Some(error.clone());
                    if completion.execution.unexecuted_reason().is_none() {
                        completion.outcome = WorkflowOutcome::NeedsAttention;
                        let ownership_error = format!(
                            "Workflow lease ownership was lost before completion could be finalized: {error}"
                        );
                        match &mut completion.error {
                            Some(existing) => existing.push_str(&format!("; {ownership_error}")),
                            None => completion.error = Some(ownership_error),
                        }
                    }
                    if tick_error.is_none() {
                        tick_error = Some(format!(
                            "Loop workflow lease renewal or release failed: {error}"
                        ));
                    }
                }
                if let Some(mut manual_guard) = manual_guard {
                    if ManualOccurrenceGuard::completion_requires_retention(&completion) {
                        let (occurrence, error) = manual_guard.complete_tick(&mut completion);
                        manual_occurrence = occurrence;
                        if let Some(error) = error {
                            append_tick_error(&mut tick_error, error);
                        }
                    } else {
                        match manual_guard.stage_tick(&mut completion) {
                            Ok(Some(occurrence)) => manual_occurrence = Some(occurrence),
                            Ok(None) => manual_receipt_guard = Some(manual_guard),
                            Err(error) => {
                                completion.outcome = WorkflowOutcome::NeedsAttention;
                                let error = format!(
                                    "Failed to stage manual loop occurrence before receipt publication: {error:#}"
                                );
                                match &mut completion.error {
                                    Some(existing) => existing.push_str(&format!("; {error}")),
                                    None => completion.error = Some(error.clone()),
                                }
                                let (_occurrence, finalization_error) =
                                    manual_guard.complete_tick(&mut completion);
                                append_tick_error(&mut tick_error, error);
                                if let Some(error) = finalization_error {
                                    append_tick_error(&mut tick_error, error);
                                }
                            }
                        }
                    }
                }
            }
            LeaseAcquire::Held(existing) => {
                lease_disposition = WorkflowLeaseDisposition::Held;
                lease = Some(existing);
            }
        }
    }

    let active_scheduled_occurrence = (!execution.manual).then_some(execution.item_key.as_str());
    let runtime_state = TickRuntimeState::collect(
        ctx,
        &mut lease_store,
        &mut attempt_store,
        active_scheduled_occurrence,
    );
    if let Some(error) = runtime_state.error_text() {
        append_tick_error(&mut tick_error, error);
    }
    let TickRuntimeState {
        checked_at_ms,
        live_leases,
        attempts,
        scheduled_needs_attention,
        state_errors,
    } = runtime_state;
    let attempt_sections = AttemptSections::new(&attempts, checked_at_ms);

    let blocked_by_runtime = release_warning.is_some()
        || !live_leases.is_empty()
        || attempt_sections.blocks_idle()
        || !scheduled_needs_attention.is_empty();

    // Idleness is machine-global for now: `loop run --until idle` should not
    // claim quiescence while any workflow lease or attempt backoff is live.
    if tick_error.is_some() || completion.outcome == WorkflowOutcome::Failed {
        idle = false;
        status = "failed";
    } else if completion.outcome == WorkflowOutcome::NeedsAttention
        || !attempt_sections.needs_attention.is_empty()
        || !scheduled_needs_attention.is_empty()
    {
        idle = false;
        status = "needs_attention";
    } else if !workflow.enabled {
        idle = !blocked_by_runtime;
        status = "disabled";
    } else if blocked_by_runtime {
        idle = false;
        status = "waiting";
    } else if actions_include_work(&actions) {
        idle = false;
        status = "acted";
    } else if actions_include_waiting(&actions) {
        idle = false;
        status = "waiting";
    }

    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_tick",
        "schema_version": 1,
        "workflow": workflow.value(),
        "status": status,
        "idle": idle,
        "observed": observed,
        "actions": actions,
        "lease": lease,
        "lease_acquired": lease_acquired,
        "live_leases": live_leases,
        "attempts": attempts,
        "waiting_attempts": attempt_sections.waiting,
        "needs_attention": {
            "exhausted_attempts": attempt_sections.needs_attention,
            "scheduled_occurrences": scheduled_needs_attention,
        },
        "state_error_count": state_errors.len(),
        "state_errors": state_errors,
        "release_warning": release_warning,
        "error": tick_error,
        "item_key": execution.item_key,
        "manual_occurrence": manual_occurrence,
    });
    let mut post_work_error = release_warning
        .as_ref()
        .map(|error| format!("Loop workflow lease renewal or release failed: {error}"));
    let receipt_cancelled = || {
        observer.cancelled()
            || manual_receipt_guard
                .as_ref()
                .is_some_and(ManualOccurrenceGuard::renewal_failed)
    };
    let receipt_id = match record_receipt_with_cancellation(
        ctx,
        ReceiptInput {
            tool_name: LOOP_TICK_TOOL,
            args: json!({
                "workflow": &workflow.id,
                "kind": &workflow.kind,
            }),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: if evidence["error"].is_null() && loop_status_is_success(status) {
                0
            } else {
                1
            },
            stdout: "",
            stderr: evidence["error"]
                .as_str()
                .or(release_warning.as_deref())
                .unwrap_or(""),
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
        &receipt_cancelled,
    ) {
        Ok(receipt_id) => receipt_id,
        Err(error) => {
            let mut error = format!("Failed to record loop tick receipt: {error:#}");
            if let Some(manual_guard) = manual_receipt_guard.take() {
                completion.outcome = WorkflowOutcome::NeedsAttention;
                match &mut completion.error {
                    Some(existing) => existing.push_str(&format!("; {error}")),
                    None => completion.error = Some(error.clone()),
                }
                let (_occurrence, finalization_error) = manual_guard.complete_tick(&mut completion);
                if let Some(finalization_error) = finalization_error {
                    error.push_str(&format!("; {finalization_error}"));
                }
            }
            append_tick_error(&mut post_work_error, error.clone());
            return Ok(ScheduledTick::Errored {
                value: None,
                completion,
                lease_disposition,
                state_errors,
                post_work_error,
                error,
            });
        }
    };

    if let Some(manual_guard) = manual_receipt_guard.take() {
        let (_occurrence, finalization_error) = manual_guard.complete_tick(&mut completion);
        let unexpected_attention =
            (completion.outcome == WorkflowOutcome::NeedsAttention).then(|| {
                completion.error.clone().unwrap_or_else(|| {
                    "Manual loop occurrence required attention after receipt publication".into()
                })
            });
        if let Some(error) = finalization_error.or(unexpected_attention) {
            let error = format!(
                "Manual loop occurrence could not be cleanly finalized after receipt {receipt_id}: {error}"
            );
            append_tick_error(&mut post_work_error, error.clone());
            return Ok(ScheduledTick::Errored {
                value: None,
                completion,
                lease_disposition,
                state_errors,
                post_work_error,
                error,
            });
        }
    }

    let value = json!({
        "ok": loop_status_is_success(status),
        "command": "loop tick",
        "receipt_id": receipt_id,
        "workflow": evidence["workflow"],
        "status": status,
        "idle": idle,
        "observed": evidence["observed"],
        "actions": evidence["actions"],
        "lease": evidence["lease"],
        "lease_acquired": lease_acquired,
        "live_leases": evidence["live_leases"],
        "attempts": evidence["attempts"],
        "waiting_attempts": evidence["waiting_attempts"],
        "needs_attention": evidence["needs_attention"],
        "state_error_count": evidence["state_error_count"],
        "state_errors": evidence["state_errors"],
        "release_warning": release_warning,
        "item_key": evidence["item_key"],
        "manual_occurrence": evidence["manual_occurrence"],
    });
    if let Some(error) = evidence["error"].as_str() {
        return Ok(ScheduledTick::Errored {
            value: Some(value),
            completion,
            lease_disposition,
            state_errors,
            post_work_error,
            error: format!(
                "Loop workflow '{}' failed; receipt {}: {}",
                workflow.id, receipt_id, error
            ),
        });
    }

    Ok(ScheduledTick::Reported {
        value,
        completion,
        lease_disposition,
        state_errors,
    })
}

fn run_workflow_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    execution: &TickExecution,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkflowTick> {
    match workflow.kind.as_str() {
        CODEX_TASK_KIND => codex_task::codex_task_tick(
            ctx,
            workflow,
            codex_task::CodexTaskExecution {
                item_key: &execution.item_key,
            },
            observer,
        ),
        GITHUB_PR_STATUS_KIND => github::github_pr_status_tick(ctx, observer),
        NOOP_STATUS_KIND => noop::noop_status_tick(ctx),
        PR_MANAGER_KIND => {
            pr_manager::pr_manager_tick(ctx, workflow, lease_store, attempt_store, observer)
        }
        _ => bail!(
            "Unsupported loop workflow kind '{}'. Supported kinds: {CODEX_TASK_KIND}, {NOOP_STATUS_KIND}, {GITHUB_PR_STATUS_KIND}, {PR_MANAGER_KIND}.",
            workflow.kind
        ),
    }
}

fn actions_include_work(actions: &[Value]) -> bool {
    actions.iter().any(|action| {
        !matches!(
            action.get("status").and_then(Value::as_str),
            Some("skipped" | "waiting" | "exhausted" | "needs_attention")
        )
    })
}

fn actions_include_waiting(actions: &[Value]) -> bool {
    actions.iter().any(|action| {
        matches!(
            action.get("status").and_then(Value::as_str),
            Some("waiting")
        )
    })
}

pub(super) fn clear_attempt(
    ctx: &RepoContext,
    request: LoopClearAttemptRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let workflow_id = request.workflow.trim();
    let item_key = request.item.trim();
    if workflow_id.is_empty() {
        bail!("--workflow must not be empty");
    }
    if item_key.is_empty() {
        bail!("--item must not be empty");
    }
    if observer.cancelled() {
        bail!("Execution was cancelled before clearing loop attempt state");
    }
    let workflow_configured = ctx
        .loop_workflows()
        .iter()
        .any(|workflow| workflow.id == workflow_id);
    let builtin_alias = matches!(workflow_id, DEFAULT_WORKFLOW_ID | NOOP_STATUS_KIND);
    let resolved_workflow = if workflow_configured || builtin_alias {
        Some(
            resolve_workflow(
                ctx,
                Some(workflow_id),
                TuningOverrides {
                    lease_ttl_seconds: None,
                    max_attempts: None,
                    backoff_seconds: None,
                },
            )?
            .value(),
        )
    } else {
        None
    };
    let mut attempt_store = AttemptStore::new(ctx);
    let (cleared, (evidence, receipt_id)) =
        attempt_store.clear_attempt_and_then(workflow_id, item_key, |cleared, deadline| {
            let workflow = if workflow_configured || (!cleared && builtin_alias) {
                resolved_workflow
                    .expect("configured workflows and built-in aliases are resolved above")
            } else {
                removed_workflow_value(workflow_id)
            };
            let evidence = json!({
                "kind": "loop_clear_attempt",
                "schema_version": 1,
                "workflow": workflow,
                "workflow_id": workflow_id,
                "item_key": item_key,
                "cleared": cleared,
            });
            let receipt_id = record_receipt_with_cancellation_until(
                ctx,
                ReceiptInput {
                    tool_name: LOOP_CLEAR_ATTEMPT_TOOL,
                    args: json!({
                        "workflow": workflow_id,
                        "item": evidence["item_key"],
                    }),
                    invoked_command_key: None,
                    plan_id: None,
                    started_at_ms: started,
                    ended_at_ms: now_ms(),
                    exit_status: 0,
                    stdout: "",
                    stderr: "",
                    evidence: Some(evidence.clone()),
                    session_override: None,
                    collect_git_metadata: false,
                    collect_worktree_fingerprint: false,
                    worktree_fingerprint_override: None,
                },
                &|| observer.cancelled(),
                deadline,
            )?;
            Ok((evidence, receipt_id))
        })?;

    Ok(json!({
        "ok": true,
        "command": "loop clear-attempt",
        "receipt_id": receipt_id,
        "workflow": evidence["workflow"],
        "workflow_id": evidence["workflow_id"],
        "item_key": evidence["item_key"],
        "cleared": cleared,
    }))
}

fn removed_workflow_value(workflow_id: &str) -> Value {
    json!({
        "id": workflow_id,
        "configured": false,
        "removed": true,
    })
}

pub(super) fn acknowledge_occurrence(
    ctx: &RepoContext,
    request: LoopAcknowledgeOccurrenceRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let occurrence_id = request.occurrence.trim();
    if occurrence_id.is_empty() {
        bail!("--occurrence must not be empty");
    }
    super::pre_execution::require_ignored_loop_runtime_root(ctx, observer)?;
    let mut occurrence_store = OccurrenceStore::new(ctx);
    let (acknowledgement, receipt_id) =
        occurrence_store.acknowledge_and_then(occurrence_id, |occurrence, changed, deadline| {
            if observer.cancelled() {
                bail!("Execution was cancelled before recording occurrence acknowledgement");
            }
            record_receipt_with_cancellation_until(
                ctx,
                ReceiptInput {
                    tool_name: LOOP_ACKNOWLEDGE_OCCURRENCE_TOOL,
                    args: json!({
                        "occurrence": occurrence_id,
                    }),
                    invoked_command_key: None,
                    plan_id: None,
                    started_at_ms: started,
                    ended_at_ms: now_ms(),
                    exit_status: 0,
                    stdout: "",
                    stderr: "",
                    evidence: Some(json!({
                        "kind": "loop_acknowledge_occurrence",
                        "schema_version": 1,
                        "occurrence": occurrence,
                        "changed": changed,
                    })),
                    session_override: None,
                    // The schedule locks stay held until receipt publication so
                    // rollback cannot overwrite a concurrent state change. Keep
                    // Git inspection outside this short commit boundary.
                    collect_git_metadata: false,
                    collect_worktree_fingerprint: false,
                    worktree_fingerprint_override: None,
                },
                &|| observer.cancelled(),
                deadline,
            )
        })?;
    let (occurrence, changed) = match acknowledgement {
        OccurrenceAcknowledgement::Acknowledged(occurrence) => (occurrence, true),
        OccurrenceAcknowledgement::AlreadyAcknowledged(occurrence) => (occurrence, false),
    };

    Ok(json!({
        "ok": true,
        "command": "loop acknowledge-occurrence",
        "receipt_id": receipt_id,
        "occurrence_id": occurrence_id,
        "occurrence": occurrence,
        "changed": changed,
    }))
}
