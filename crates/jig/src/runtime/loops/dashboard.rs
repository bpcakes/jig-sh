use super::state::{AttemptRecord, LeaseRecord};
use super::workflow::ResolvedWorkflow;

pub(super) fn lease_status(lease: &LeaseRecord) -> jig_ui::dashboard::LoopLease {
    jig_ui::dashboard::LoopLease {
        key: lease.key.clone(),
        owner: lease.owner.clone(),
        acquired_at_ms: lease.acquired_at_ms,
        expires_at_ms: lease.expires_at_ms,
    }
}

pub(super) fn attempt_status(attempt: &AttemptRecord) -> jig_ui::dashboard::StatusLoopAttempt {
    jig_ui::dashboard::StatusLoopAttempt {
        key: attempt.key.clone(),
        workflow_id: attempt.workflow_id.clone(),
        item_key: attempt.item_key.clone(),
        item_version: attempt.item_version.clone(),
        observed_item_version: attempt.observed_item_version.clone(),
        attempts: attempt.attempts,
        max_attempts: attempt.max_attempts,
        last_attempt_ms: attempt.last_attempt_ms,
        next_eligible_ms: attempt.next_eligible_ms,
        exhausted: attempt.exhausted,
        last_status: attempt.last_status.clone(),
    }
}

pub(super) fn exhausted_attempt_status(
    attempt: &AttemptRecord,
) -> jig_ui::dashboard::StatusExhaustedAttempt {
    let view = attempt_status(attempt);
    jig_ui::dashboard::StatusExhaustedAttempt {
        key: view.key,
        workflow_id: view.workflow_id,
        item_key: view.item_key,
        item_version: view.item_version,
        observed_item_version: view.observed_item_version,
        attempts: view.attempts,
        max_attempts: view.max_attempts,
        last_attempt_ms: view.last_attempt_ms,
        next_eligible_ms: view.next_eligible_ms,
        exhausted: view.exhausted,
        last_status: view.last_status,
    }
}

pub(super) fn workflow_status(
    workflow: &ResolvedWorkflow,
) -> jig_ui::dashboard::StatusLoopWorkflow {
    jig_ui::dashboard::StatusLoopWorkflow {
        id: workflow.id.clone(),
        kind: workflow.kind.clone(),
        enabled: workflow.enabled,
        configured: workflow.configured,
        lease_ttl_seconds: workflow.lease_ttl_seconds,
        max_attempts: workflow.max_attempts,
        backoff_seconds: workflow.backoff_seconds,
        codex_home_configured: workflow
            .codex_home_configured
            .as_ref()
            .map(|home| home.display().to_string()),
        schedule: workflow
            .schedule
            .as_ref()
            .map(|schedule| jig_ui::dashboard::LoopSchedule {
                cron: schedule.expression().to_string(),
                timezone: schedule.timezone_name().to_string(),
            }),
        schedule_state: None,
        schedule_state_error: None,
        codex_task: workflow
            .codex_task
            .as_ref()
            .map(|task| jig_ui::dashboard::LoopCodexTask {
                prompt_file: task.prompt_file.display().to_string(),
                model: task.model.clone(),
                sandbox: task.sandbox.clone(),
                checkout: task.checkout.as_str().to_string(),
            }),
    }
}
