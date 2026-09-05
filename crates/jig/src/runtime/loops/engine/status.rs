use super::*;
use crate::cancellation::ensure_status_collection_active;
use crate::command::LoopStatusRequest;
use crate::runtime::loops::dashboard::{
    attempt_status, exhausted_attempt_status, lease_status, workflow_status,
};
use crate::runtime::loops::occurrence::{OccurrenceStore, ScheduleOccurrence};

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

pub(in crate::runtime::loops) fn typed_status_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<jig_ui::dashboard::StatusLoopObservation> {
    typed_status_at_with_cancellation(ctx, request, cancelled, now_ms())
}

pub(in crate::runtime::loops) fn status_at_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
    checked_at_ms: u64,
) -> Result<Value> {
    serde_json::to_value(typed_status_at_with_cancellation(
        ctx,
        request,
        cancelled,
        checked_at_ms,
    )?)
    .map_err(Into::into)
}

fn typed_status_at_with_cancellation(
    ctx: &RepoContext,
    request: LoopStatusRequest,
    cancelled: &dyn Fn() -> bool,
    checked_at_ms: u64,
) -> Result<jig_ui::dashboard::StatusLoopObservation> {
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
            let mut view = workflow_status(&workflow);
            if let Some(schedule) = workflow.schedule.as_ref() {
                let latest = OccurrenceStore::latest_for_workflow(&occurrences, &workflow.id);
                match schedule.window(
                    checked_at_ms,
                    latest.as_ref().map(|record| record.scheduled_at_ms),
                ) {
                    Ok(window) => {
                        view.schedule_state = Some(jig_ui::dashboard::LoopScheduleState {
                            due_at_ms: window.due_at_ms,
                            next_at_ms: window.next_at_ms,
                            last_scheduled_at_ms: latest
                                .as_ref()
                                .map(|record| record.scheduled_at_ms),
                            last_status: latest
                                .as_ref()
                                .map(|record| record.status.as_str().to_string()),
                        });
                    }
                    Err(error) => {
                        let error = format!("Failed to evaluate workflow schedule: {error:#}");
                        view.schedule_state_error = Some(error.clone());
                        schedule_state_errors.push(jig_ui::dashboard::LoopStateError {
                            kind: "workflow_schedule".to_string(),
                            workflow_id: Some(workflow.id.clone()),
                            error,
                        });
                    }
                }
            }
            view
        })
        .collect::<Vec<_>>();
    let scheduled_needs_attention = occurrences
        .iter()
        .filter(|record| record.requires_attention_at(checked_at_ms))
        .cloned()
        .collect::<Vec<_>>();

    Ok(jig_ui::dashboard::StatusLoopObservation {
        ok: schedule_state_errors.is_empty(),
        command: "loop status".to_string(),
        workflows,
        leases: leases.iter().map(lease_status).collect(),
        attempts: attempts.iter().map(attempt_status).collect(),
        scheduled_occurrences: occurrences
            .iter()
            .map(ScheduleOccurrence::status_view)
            .collect(),
        waiting_attempts: attempt_sections
            .waiting
            .iter()
            .map(attempt_status)
            .collect(),
        state_error_count: u64::try_from(schedule_state_errors.len()).unwrap_or(u64::MAX),
        state_errors: schedule_state_errors,
        needs_attention: jig_ui::dashboard::StatusLoopAttention {
            exhausted_attempts: attempt_sections
                .needs_attention
                .iter()
                .map(exhausted_attempt_status)
                .collect(),
            scheduled_occurrences: scheduled_needs_attention
                .iter()
                .map(ScheduleOccurrence::status_view)
                .collect(),
        },
    })
}

fn ensure_status_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}
