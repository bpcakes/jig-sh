use anyhow::{Result, bail};
use serde_json::{Value, json};

use crate::command::{LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest};
use crate::context::RepoContext;
use crate::execution::ExecutionControl;
use crate::state::{ReceiptInput, now_ms, record_receipt_with_cancellation_until};
use crate::tool_defs::{LOOP_ACKNOWLEDGE_OCCURRENCE_TOOL, LOOP_CLEAR_ATTEMPT_TOOL};

use super::super::occurrence::{OccurrenceAcknowledgement, OccurrenceStore};
use super::super::state::AttemptStore;
use super::super::workflow::{
    DEFAULT_WORKFLOW_ID, NOOP_STATUS_KIND, TuningOverrides, resolve_workflow,
};

pub(in crate::runtime::loops) fn clear_attempt(
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
    let (cleared, (evidence, receipt_id)) = attempt_store.clear_attempt_and_then(
        workflow_id,
        item_key,
        &|| observer.cancelled(),
        |cleared, deadline| {
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

pub(in crate::runtime::loops) fn acknowledge_occurrence(
    ctx: &RepoContext,
    request: LoopAcknowledgeOccurrenceRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let started = now_ms();
    let occurrence_id = request.occurrence.trim();
    if occurrence_id.is_empty() {
        bail!("--occurrence must not be empty");
    }
    super::super::pre_execution::require_ignored_loop_runtime_root(ctx, observer)?;
    let mut occurrence_store = OccurrenceStore::new(ctx);
    let (acknowledgement, receipt_id) = occurrence_store.acknowledge_and_then(
        occurrence_id,
        &|| observer.cancelled(),
        |occurrence, changed, deadline| {
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
                    // Hold schedule locks through receipt publication; keep Git inspection outside.
                    collect_git_metadata: false,
                    collect_worktree_fingerprint: false,
                    worktree_fingerprint_override: None,
                },
                &|| observer.cancelled(),
                deadline,
            )
        },
    )?;
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
