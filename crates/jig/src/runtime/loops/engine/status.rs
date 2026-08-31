use super::*;
use crate::cancellation::ensure_status_collection_active;
use crate::command::LoopStatusRequest;

#[cfg(test)]
pub(in crate::runtime::loops) fn status(
    ctx: &RepoContext,
    request: LoopStatusRequest,
) -> Result<Value> {
    status_with_cancellation(ctx, request, &|| false)
}

pub(in crate::runtime::loops) fn status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    status_at_with_cancellation(ctx, request, cancelled, now_ms())
}

pub(in crate::runtime::loops) fn status_at_with_cancellation(
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

    let selected_workflow = request.workflow.as_ref().map(|_| &resolved_workflows[0]);
    let mut attempts = AttemptStore::new(ctx).snapshot_read_only_with_cancellation(cancelled)?;
    if let Some(workflow) = selected_workflow {
        attempts.retain(|attempt| attempt.belongs_to(&workflow.id));
    }
    ensure_status_active(cancelled)?;
    let attempt_sections =
        AttemptSections::new_with_cancellation(&attempts, checked_at_ms, cancelled)?;
    ensure_status_active(cancelled)?;
    let mut leases = LeaseStore::new(ctx).active_leases_read_only_with_cancellation(cancelled)?;
    if let Some(workflow) = selected_workflow {
        leases.retain(|lease| lease.matches_key(&workflow.lease_key()));
    }
    if selected_workflow.is_none_or(|workflow| workflow.kind == PR_MANAGER_KIND) {
        leases.extend(
            LeaseStore::new_repository(ctx).active_leases_read_only_with_cancellation(cancelled)?,
        );
    }
    ensure_status_active(cancelled)?;
    let mut occurrences =
        OccurrenceStore::new(ctx).snapshot_read_only_with_cancellation(cancelled)?;
    if let Some(workflow) = selected_workflow {
        occurrences.retain(|record| record.workflow_id == workflow.id);
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
