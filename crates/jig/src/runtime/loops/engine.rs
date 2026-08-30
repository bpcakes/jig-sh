use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::command::{
    LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopStatusRequest, LoopTickRequest,
};
use crate::context::RepoContext;
use crate::execution::{AdditionalCancellationControl, ExecutionControl};
use crate::state::{ReceiptInput, now_ms, record_receipt, record_receipt_with_cancellation};
use crate::tool_defs::{LOOP_ACKNOWLEDGE_OCCURRENCE_TOOL, LOOP_CLEAR_ATTEMPT_TOOL, LOOP_TICK_TOOL};

use super::occurrence::{OccurrenceAcknowledgement, OccurrenceStore};
use super::state::{
    AttemptRecord, AttemptSections, AttemptStore, LeaseAcquire, LeaseGuard, LeaseRecord, LeaseStore,
};
use super::workflow::{
    CODEX_TASK_KIND, DEFAULT_WORKFLOW_ID, GITHUB_PR_STATUS_KIND, NOOP_STATUS_KIND, PR_MANAGER_KIND,
    ResolvedWorkflow, TuningOverrides, UnexecutedReason, WorkflowCompletion, WorkflowOutcome,
    WorkflowTick, list_workflows, loop_status_is_success, resolve_workflow,
};
use super::{codex_task, github, noop, pr_manager};

mod unexecuted;

use unexecuted::UnexecutedTickError;

struct TickExecution {
    item_key: String,
}

struct TickRuntimeState {
    live_leases: Vec<LeaseRecord>,
    attempts: Vec<AttemptRecord>,
    state_errors: Vec<Value>,
}

impl TickRuntimeState {
    fn collect(lease_store: &mut LeaseStore, attempt_store: &mut AttemptStore) -> Self {
        let mut state_errors = Vec::new();
        let live_leases = match lease_store.active_leases() {
            Ok(leases) => leases,
            Err(error) => {
                state_errors.push(runtime_state_error("leases", error));
                Vec::new()
            }
        };
        let attempts = match attempt_store.snapshot() {
            Ok(attempts) => attempts,
            Err(error) => {
                state_errors.push(runtime_state_error("attempts", error));
                Vec::new()
            }
        };
        Self {
            live_leases,
            attempts,
            state_errors,
        }
    }

    fn error_text(&self) -> Option<String> {
        let errors = self
            .state_errors
            .iter()
            .filter_map(|error| error["error"].as_str())
            .collect::<Vec<_>>();
        (!errors.is_empty()).then(|| errors.join("; "))
    }
}

fn runtime_state_error(kind: &str, error: anyhow::Error) -> Value {
    json!({
        "kind": kind,
        "error": format!("Failed to inspect post-work loop {kind} state: {error:#}"),
    })
}

fn append_tick_error(tick_error: &mut Option<String>, error: String) {
    match tick_error {
        Some(existing) => existing.push_str(&format!("; {error}")),
        None => *tick_error = Some(error),
    }
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
            item_key: format!("manual-{started}"),
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
                let lease_cancelled = || lease_guard.renewal_failed();
                let mut lease_control =
                    AdditionalCancellationControl::new(observer, &lease_cancelled);
                match run_workflow_tick(
                    ctx,
                    &workflow,
                    &execution,
                    &mut lease_store,
                    &mut attempt_store,
                    &mut lease_control,
                ) {
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
            }
            LeaseAcquire::Held(existing) => {
                lease_disposition = WorkflowLeaseDisposition::Held;
                lease = Some(existing);
            }
        }
    }

    let runtime_state = TickRuntimeState::collect(&mut lease_store, &mut attempt_store);
    if let Some(error) = runtime_state.error_text() {
        append_tick_error(&mut tick_error, error);
    }
    let TickRuntimeState {
        live_leases,
        attempts,
        state_errors,
    } = runtime_state;
    let attempt_check_at_ms = now_ms();
    let attempt_sections = AttemptSections::new(&attempts, attempt_check_at_ms);

    let blocked_by_runtime =
        release_warning.is_some() || !live_leases.is_empty() || attempt_sections.blocks_idle();

    // Idleness is machine-global for now: `loop run --until idle` should not
    // claim quiescence while any workflow lease or attempt backoff is live.
    if tick_error.is_some() || completion.outcome == WorkflowOutcome::Failed {
        idle = false;
        status = "failed";
    } else if completion.outcome == WorkflowOutcome::NeedsAttention
        || !attempt_sections.needs_attention.is_empty()
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
        },
        "state_error_count": state_errors.len(),
        "state_errors": state_errors,
        "release_warning": release_warning,
        "error": tick_error,
        "item_key": execution.item_key,
    });
    let mut post_work_error = release_warning
        .as_ref()
        .map(|error| format!("Loop workflow lease renewal or release failed: {error}"));
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
        &|| observer.cancelled(),
    ) {
        Ok(receipt_id) => receipt_id,
        Err(error) => {
            let error = format!("Failed to record loop tick receipt: {error:#}");
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

#[cfg(test)]
pub(super) fn status(ctx: &RepoContext, request: LoopStatusRequest) -> Result<Value> {
    status_with_cancellation(ctx, request, &|| false)
}

pub(super) fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    status_at_with_cancellation(ctx, request, cancelled, now_ms())
}

pub(super) fn status_at_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
    checked_at_ms: u64,
) -> Result<Value> {
    ensure_status_active(cancelled)?;
    let resolved_workflows = if let Some(workflow) = request.workflow.as_deref() {
        vec![resolve_workflow(
            ctx,
            Some(workflow),
            TuningOverrides {
                lease_ttl_seconds: None,
                max_attempts: None,
                backoff_seconds: None,
            },
        )?]
    } else {
        list_workflows(ctx)?
    };
    ensure_status_active(cancelled)?;

    let attempts = AttemptStore::new(ctx).snapshot_read_only_with_cancellation(cancelled)?;
    ensure_status_active(cancelled)?;
    let attempt_sections =
        AttemptSections::new_with_cancellation(&attempts, checked_at_ms, cancelled)?;
    ensure_status_active(cancelled)?;
    let leases = LeaseStore::new(ctx).active_leases_read_only_with_cancellation(cancelled)?;
    ensure_status_active(cancelled)?;
    let mut occurrences =
        OccurrenceStore::new(ctx).snapshot_read_only_with_cancellation(cancelled)?;
    if request.workflow.is_some() {
        let workflow_id = &resolved_workflows[0].id;
        occurrences.retain(|record| record.workflow_id == *workflow_id);
    }
    ensure_status_active(cancelled)?;
    let mut schedule_state_errors = Vec::new();
    let workflows = resolved_workflows
        .into_iter()
        .map(|workflow| {
            let mut value = workflow.value();
            if let Some(schedule) = workflow.schedule.as_ref() {
                let latest = OccurrenceStore::latest_for_workflow(&occurrences, &workflow.id);
                match schedule.window(
                    checked_at_ms,
                    latest.as_ref().map(|record| record.scheduled_at_ms),
                ) {
                    Ok(window) => {
                        value["schedule_state"] = json!({
                            "due_at_ms": window.due_at_ms,
                            "next_at_ms": window.next_at_ms,
                            "last_scheduled_at_ms": latest.as_ref().map(|record| record.scheduled_at_ms),
                            "last_status": latest.as_ref().map(|record| record.status.as_str()),
                        });
                    }
                    Err(error) => {
                        let error = format!("Failed to evaluate workflow schedule: {error:#}");
                        value["schedule_state_error"] = error.clone().into();
                        schedule_state_errors.push(json!({
                            "kind": "workflow_schedule",
                            "workflow_id": workflow.id,
                            "error": error,
                        }));
                    }
                }
            }
            value
        })
        .collect::<Vec<_>>();
    let scheduled_needs_attention = occurrences
        .iter()
        .filter(|record| record.requires_attention_at(checked_at_ms))
        .cloned()
        .collect::<Vec<_>>();

    Ok(json!({
        "ok": schedule_state_errors.is_empty(),
        "command": "loop status",
        "workflows": workflows,
        "leases": leases,
        "attempts": attempts,
        "scheduled_occurrences": occurrences,
        "waiting_attempts": attempt_sections.waiting,
        "state_error_count": schedule_state_errors.len(),
        "state_errors": schedule_state_errors,
        "needs_attention": {
            "exhausted_attempts": attempt_sections.needs_attention,
            "scheduled_occurrences": scheduled_needs_attention,
        },
    }))
}

fn ensure_status_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
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

pub(super) fn clear_attempt(ctx: &RepoContext, request: LoopClearAttemptRequest) -> Result<Value> {
    let started = now_ms();
    let workflow_id = request.workflow.trim();
    let item_key = request.item.trim();
    if workflow_id.is_empty() {
        bail!("--workflow must not be empty");
    }
    if item_key.is_empty() {
        bail!("--item must not be empty");
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
    let cleared_attempt = attempt_store.take_attempt(workflow_id, item_key)?;
    let cleared = cleared_attempt.is_some();
    let workflow = if workflow_configured || (!cleared && builtin_alias) {
        resolved_workflow.expect("configured workflows and built-in aliases are resolved above")
    } else {
        removed_workflow_value(workflow_id)
    };
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_clear_attempt",
        "schema_version": 1,
        "workflow": workflow,
        "workflow_id": workflow_id,
        "item_key": item_key,
        "cleared": cleared,
    });
    let receipt_id = record_receipt(
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
            ended_at_ms: ended,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
    )?;

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
) -> Result<Value> {
    let started = now_ms();
    let occurrence_id = request.occurrence.trim();
    if occurrence_id.is_empty() {
        bail!("--occurrence must not be empty");
    }
    let mut occurrence_store = OccurrenceStore::new(ctx);
    let (occurrence, changed) = match occurrence_store.acknowledge(occurrence_id)? {
        OccurrenceAcknowledgement::Acknowledged(occurrence) => (occurrence, true),
        OccurrenceAcknowledgement::AlreadyAcknowledged(occurrence) => (occurrence, false),
    };
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_acknowledge_occurrence",
        "schema_version": 1,
        "occurrence": occurrence,
        "changed": changed,
    });
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: LOOP_ACKNOWLEDGE_OCCURRENCE_TOOL,
            args: json!({
                "occurrence": occurrence_id,
            }),
            invoked_command_key: None,
            plan_id: None,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: Some(evidence.clone()),
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: true,
            worktree_fingerprint_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "command": "loop acknowledge-occurrence",
        "receipt_id": receipt_id,
        "occurrence_id": occurrence_id,
        "occurrence": evidence["occurrence"],
        "changed": changed,
    }))
}
