use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::command::{LoopClearAttemptRequest, LoopStatusRequest, LoopTickRequest};
use crate::context::RepoContext;
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::{LOOP_CLEAR_ATTEMPT_TOOL, LOOP_TICK_TOOL};

use super::state::{AttemptSections, AttemptStore, LeaseAcquire, LeaseStore};
use super::workflow::{
    GITHUB_PR_STATUS_KIND, NOOP_STATUS_KIND, PR_MANAGER_KIND, ResolvedWorkflow, TuningOverrides,
    WorkflowTick, list_workflows, resolve_workflow,
};
use super::{github, noop, pr_manager};

pub(super) fn tick(ctx: &RepoContext, request: LoopTickRequest) -> Result<Value> {
    let started = now_ms();
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
    let mut release_warning = None;
    let mut observed = Value::Null;
    let mut actions = Vec::new();
    let mut tick_error = None;

    if !workflow.enabled {
        status = "disabled";
    } else {
        let lease_key = workflow.lease_key();
        match lease_store.acquire(&lease_key, workflow.lease_ttl_seconds)? {
            LeaseAcquire::Acquired(acquired) => {
                lease = Some(acquired.clone());
                match run_workflow_tick(ctx, &workflow, &mut lease_store, &mut attempt_store) {
                    Ok(tick) => {
                        observed = tick.observed;
                        actions = tick.actions;
                    }
                    Err(error) => {
                        tick_error = Some(format!("{error:#}"));
                    }
                }
                let released = lease_store.release(&lease_key, &acquired.owner);
                if let Err(error) = released {
                    release_warning = Some(format!("{error:#}"));
                }
            }
            LeaseAcquire::Held(existing) => {
                lease = Some(existing);
            }
        }
    }

    let live_leases = lease_store.active_leases()?;
    let attempts = attempt_store.snapshot()?;
    let attempt_check_at_ms = now_ms();
    let attempt_sections = AttemptSections::new(&attempts, attempt_check_at_ms);

    let blocked_by_runtime =
        release_warning.is_some() || !live_leases.is_empty() || attempt_sections.blocks_idle();

    // Idleness is machine-global for now: `loop run --until idle` should not
    // claim quiescence while any workflow lease or attempt backoff is live.
    if tick_error.is_some() {
        idle = false;
        status = "failed";
    } else if !attempt_sections.needs_attention.is_empty() {
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
        "live_leases": live_leases,
        "attempts": attempts,
        "waiting_attempts": attempt_sections.waiting,
        "needs_attention": {
            "exhausted_attempts": attempt_sections.needs_attention,
        },
        "release_warning": release_warning,
        "error": tick_error,
    });
    let receipt_id = record_receipt(
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
            exit_status: if evidence["error"].is_null() { 0 } else { 1 },
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
    )?;

    if let Some(error) = evidence["error"].as_str() {
        bail!(
            "Loop workflow '{}' failed; receipt {}: {}",
            workflow.id,
            receipt_id,
            error
        );
    }

    Ok(json!({
        "ok": true,
        "command": "loop tick",
        "receipt_id": receipt_id,
        "workflow": evidence["workflow"],
        "status": status,
        "idle": idle,
        "observed": evidence["observed"],
        "actions": evidence["actions"],
        "lease": evidence["lease"],
        "live_leases": evidence["live_leases"],
        "attempts": evidence["attempts"],
        "waiting_attempts": evidence["waiting_attempts"],
        "needs_attention": evidence["needs_attention"],
        "release_warning": release_warning,
    }))
}

pub(super) fn status(ctx: &RepoContext, request: LoopStatusRequest) -> Result<Value> {
    status_with_cancellation(ctx, request, &|| false)
}

pub(super) fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_status_active(cancelled)?;
    let workflows = if let Some(workflow) = request.workflow.as_deref() {
        vec![
            resolve_workflow(
                ctx,
                Some(workflow),
                TuningOverrides {
                    lease_ttl_seconds: None,
                    max_attempts: None,
                    backoff_seconds: None,
                },
            )?
            .value(),
        ]
    } else {
        list_workflows(ctx)?
            .into_iter()
            .map(|workflow| workflow.value())
            .collect::<Vec<_>>()
    };
    ensure_status_active(cancelled)?;

    let attempts = AttemptStore::new(ctx).snapshot_read_only_with_cancellation(cancelled)?;
    ensure_status_active(cancelled)?;
    let attempt_sections = AttemptSections::new_with_cancellation(&attempts, now_ms(), cancelled)?;
    ensure_status_active(cancelled)?;
    let leases = LeaseStore::new(ctx).active_leases_read_only_with_cancellation(cancelled)?;
    ensure_status_active(cancelled)?;

    Ok(json!({
        "ok": true,
        "command": "loop status",
        "workflows": workflows,
        "leases": leases,
        "attempts": attempts,
        "waiting_attempts": attempt_sections.waiting,
        "needs_attention": {
            "exhausted_attempts": attempt_sections.needs_attention,
        },
    }))
}

fn ensure_status_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

fn run_workflow_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
) -> Result<WorkflowTick> {
    match workflow.kind.as_str() {
        GITHUB_PR_STATUS_KIND => github::github_pr_status_tick(ctx),
        NOOP_STATUS_KIND => noop::noop_status_tick(ctx),
        PR_MANAGER_KIND => pr_manager::pr_manager_tick(ctx, workflow, lease_store, attempt_store),
        _ => bail!(
            "Unsupported loop workflow kind '{}'. Supported kinds: {NOOP_STATUS_KIND}, {GITHUB_PR_STATUS_KIND}, {PR_MANAGER_KIND}.",
            workflow.kind
        ),
    }
}

fn actions_include_work(actions: &[Value]) -> bool {
    actions.iter().any(|action| {
        !matches!(
            action.get("status").and_then(Value::as_str),
            Some("skipped" | "waiting" | "needs_attention")
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
    if request.item.trim().is_empty() {
        bail!("--item must not be empty");
    }
    let workflow = resolve_workflow(
        ctx,
        Some(&request.workflow),
        TuningOverrides {
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        },
    )?;
    let mut attempt_store = AttemptStore::new(ctx);
    let cleared = attempt_store.clear_attempt(&workflow.id, &request.item)?;
    let ended = now_ms();
    let evidence = json!({
        "kind": "loop_clear_attempt",
        "schema_version": 1,
        "workflow": workflow.value(),
        "item_key": request.item,
        "cleared": cleared,
    });
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: LOOP_CLEAR_ATTEMPT_TOOL,
            args: json!({
                "workflow": &workflow.id,
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
        "item_key": evidence["item_key"],
        "cleared": cleared,
    }))
}
