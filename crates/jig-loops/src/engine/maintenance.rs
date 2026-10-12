use anyhow::{Result, bail};
use jig_context::RepoContext;
use jig_execution::ExecutionControl;
use serde_json::{Value, json};

use crate::{LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest};

use super::super::occurrence::{OccurrenceAcknowledgement, OccurrenceStore};
use super::super::state::AttemptStore;
use super::super::workflow::{
    DEFAULT_WORKFLOW_ID, NOOP_STATUS_KIND, TuningOverrides, resolve_workflow,
};

pub fn clear_attempt(
    ctx: &RepoContext,
    request: LoopClearAttemptRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
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
    let cleared =
        AttemptStore::new(ctx)
            .clear_attempt_with_cancellation(workflow_id, item_key, &|| observer.cancelled())?;
    let workflow = if workflow_configured || (!cleared && builtin_alias) {
        resolved_workflow.expect("configured workflows and built-in aliases are resolved above")
    } else {
        removed_workflow_value(workflow_id)
    };

    Ok(json!({
        "ok": true,
        "command": "loop clear-attempt",
        "workflow": workflow,
        "workflow_id": workflow_id,
        "item_key": item_key,
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

pub fn acknowledge_occurrence(
    ctx: &RepoContext,
    request: LoopAcknowledgeOccurrenceRequest,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let occurrence_id = request.occurrence.trim();
    if occurrence_id.is_empty() {
        bail!("--occurrence must not be empty");
    }
    super::super::pre_execution::require_ignored_loop_runtime_root(ctx, observer)?;
    let acknowledgement = OccurrenceStore::new(ctx)
        .acknowledge_with_cancellation(occurrence_id, &|| observer.cancelled())?;
    let (occurrence, changed) = match acknowledgement {
        OccurrenceAcknowledgement::Acknowledged(occurrence) => (occurrence, true),
        OccurrenceAcknowledgement::AlreadyAcknowledged(occurrence) => (occurrence, false),
    };

    Ok(json!({
        "ok": true,
        "command": "loop acknowledge-occurrence",
        "occurrence_id": occurrence_id,
        "occurrence": occurrence,
        "changed": changed,
    }))
}
