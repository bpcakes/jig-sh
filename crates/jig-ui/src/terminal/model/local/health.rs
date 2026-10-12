use jig_dashboard::Remediation;

use super::*;

pub(super) fn health_items(
    failures: &[FailureView],
    targets: &[TargetView],
    loops: Option<&LoopObservation>,
) -> Vec<HealthItemView> {
    let mut items = failures
        .iter()
        .map(|failure| HealthItemView {
            identity: failure.identity.clone(),
            section: "Recent failures",
            primary: format!("{} {}", failure.target, failure.outcome),
            secondary: format!("{} · {}", failure.ended_at, failure.run_id),
            detail: failure_detail(failure),
        })
        .chain(targets.iter().map(target_health))
        .collect::<Vec<_>>();
    if let Some(loops) = loops {
        items.push(loop_overview(loops));
        items.extend(loops.workflows.items().iter().map(workflow_health));
        items.extend(loops.leases.items().iter().map(lease_health));
        items.extend(
            loops
                .attempts
                .items()
                .iter()
                .map(|attempt| attempt_health(attempt, "Attempts")),
        );
        items.extend(
            loops
                .waiting_attempts
                .items()
                .iter()
                .map(|attempt| attempt_health(attempt, "Waiting attempts")),
        );
        items.extend(
            loops
                .scheduled_occurrences
                .items()
                .iter()
                .map(|occurrence| occurrence_health(occurrence, "Loop runs")),
        );
        let mut duplicate_ordinals = std::collections::BTreeMap::new();
        items.extend(loops.state_errors.iter().map(|error| {
            let key = (
                error.kind.clone(),
                error.workflow_id.clone(),
                error.error.clone(),
            );
            let ordinal = duplicate_ordinals.entry(key).or_insert(0_usize);
            let item = state_error_health(error, *ordinal);
            *ordinal += 1;
            item
        }));
        items.extend(
            loops
                .needs_attention
                .exhausted_attempts
                .items()
                .iter()
                .map(exhausted_health),
        );
        items.extend(
            loops
                .needs_attention
                .scheduled_occurrences
                .items()
                .iter()
                .map(|occurrence| occurrence_health(occurrence, "Needs attention")),
        );
    }
    items
}

fn loop_overview(loops: &LoopObservation) -> HealthItemView {
    let lines = vec![
        format!("Status: {}", if loops.ok { "complete" } else { "partial" }),
        field("Command", &loops.command),
        LimitView::from_rows(&loops.workflows).label("workflows"),
        LimitView::from_rows(&loops.leases).label("leases"),
        LimitView::from_rows(&loops.attempts).label("attempts"),
        LimitView::from_rows(&loops.waiting_attempts).label("waiting attempts"),
        LimitView::from_rows(&loops.scheduled_occurrences).label("scheduled occurrences"),
        LimitView::from_rows(&loops.needs_attention.exhausted_attempts).label("exhausted attempts"),
        LimitView::from_rows(&loops.needs_attention.scheduled_occurrences)
            .label("attention occurrences"),
        format!(
            "State errors: {} reported / {} retained",
            loops.state_error_count,
            loops.state_errors.len()
        ),
    ];
    HealthItemView {
        identity: "loops:overview".to_string(),
        section: "Loops",
        primary: if loops.ok {
            "loop state complete".to_string()
        } else {
            "loop state partial".to_string()
        },
        secondary: format!("{} workflows", loops.workflows.items().len()),
        detail: DetailDocument::new("Loop collection", lines),
    }
}

fn target_health(target: &TargetView) -> HealthItemView {
    HealthItemView {
        identity: format!("target:{}", target.raw_target),
        section: "Check health",
        primary: format!("{} {}", target.target, target.last_status),
        secondary: format!(
            "{} runs · {} failures · avg {}",
            target.runs, target.failures, target.average
        ),
        detail: DetailDocument::new(
            "Target health",
            vec![
                format!("Target: {}", target.target),
                format!("Last run: {}", target.last_ended_at),
                format!("Last conclusion: {}", target.last_status),
                format!("Runs: {}", target.runs),
                format!("Failures: {}", target.failures),
                format!("Average duration: {}", target.average),
            ],
        ),
    }
}

fn failure_detail(failure: &FailureView) -> DetailDocument {
    let mut lines = vec![
        format!("Run: {}", failure.run_id),
        format!("Target: {}", failure.target),
        format!("Conclusion: {}", failure.outcome),
        format!("Ended: {}", failure.ended_at),
    ];
    append_text(&mut lines, "Output tail", &failure.output);
    DetailDocument::new("Failure detail", lines)
}

fn workflow_health(workflow: &LoopWorkflow) -> HealthItemView {
    let schedule = workflow.schedule.as_ref().map_or_else(
        || "manual".to_string(),
        |schedule| {
            format!(
                "{} ({})",
                sanitize_text(&schedule.cron),
                sanitize_text(&schedule.timezone)
            )
        },
    );
    let mut lines = vec![
        field("Workflow", &workflow.id),
        field("Kind", &workflow.kind),
        format!("Enabled: {}", workflow.enabled),
        format!("Configured: {}", workflow.configured),
        format!("Lease TTL: {}s", workflow.lease_ttl_seconds),
        format!("Max attempts: {}", workflow.max_attempts),
        format!("Backoff: {}s", workflow.backoff_seconds),
        format!("Schedule: {schedule}"),
    ];
    push_optional(
        &mut lines,
        "Codex home",
        workflow.codex_home_configured.as_deref(),
    );
    push_optional(
        &mut lines,
        "Schedule error",
        workflow.schedule_state_error.as_deref(),
    );
    if let Some(state) = &workflow.schedule_state {
        lines.extend([
            format!("Due: {}", format_timestamp(state.due_at_ms)),
            format!("Next: {}", format_timestamp(Some(state.next_at_ms))),
            format!(
                "Last scheduled: {}",
                format_timestamp(state.last_scheduled_at_ms)
            ),
        ]);
        push_optional(&mut lines, "Last status", state.last_status.as_deref());
    }
    if let Some(task) = &workflow.codex_task {
        lines.extend([
            field("Prompt file", &task.prompt_file),
            field("Sandbox", &task.sandbox),
            field("Checkout", &task.checkout),
        ]);
        push_optional(&mut lines, "Model", task.model.as_deref());
    }
    HealthItemView {
        identity: format!("workflow:{}", workflow.id),
        section: "Loop workflows",
        primary: format!(
            "{} {}",
            sanitize_text(&workflow.id),
            if workflow.enabled {
                "enabled"
            } else {
                "disabled"
            }
        ),
        secondary: format!("{} · {schedule}", sanitize_text(&workflow.kind)),
        detail: DetailDocument::new("Loop workflow", lines),
    }
}

fn lease_health(lease: &LoopLease) -> HealthItemView {
    HealthItemView {
        identity: format!("lease:{}", lease.key),
        section: "Active leases",
        primary: sanitize_text(&lease.key),
        secondary: format!(
            "owner {} · expires {}",
            sanitize_text(&lease.owner),
            format_timestamp(Some(lease.expires_at_ms))
        ),
        detail: DetailDocument::new(
            "Loop lease",
            vec![
                field("Key", &lease.key),
                field("Owner", &lease.owner),
                format!("Acquired: {}", format_timestamp(Some(lease.acquired_at_ms))),
                format!("Expires: {}", format_timestamp(Some(lease.expires_at_ms))),
            ],
        ),
    }
}

fn attempt_health(attempt: &LoopAttempt, section: &'static str) -> HealthItemView {
    let mut lines = vec![
        field("Key", &attempt.key),
        field("Workflow", &attempt.workflow_id),
        field("Item", &attempt.item_key),
        format!("Attempts: {}/{}", attempt.attempts, attempt.max_attempts),
        format!(
            "Last attempt: {}",
            format_timestamp(Some(attempt.last_attempt_ms))
        ),
        format!(
            "Next eligible: {}",
            format_timestamp(Some(attempt.next_eligible_ms))
        ),
        format!("Exhausted: {}", attempt.exhausted),
        field("Last status", &attempt.last_status),
    ];
    push_optional(&mut lines, "Item version", attempt.item_version.as_deref());
    push_optional(
        &mut lines,
        "Observed version",
        attempt.observed_item_version.as_deref(),
    );
    HealthItemView {
        identity: format!("{}:{}", section, attempt.key),
        section,
        primary: format!(
            "{} / {}",
            sanitize_text(&attempt.workflow_id),
            sanitize_text(&attempt.item_key)
        ),
        secondary: format!(
            "{}/{} · {}",
            attempt.attempts,
            attempt.max_attempts,
            sanitize_text(&attempt.last_status)
        ),
        detail: DetailDocument::new("Loop attempt", lines),
    }
}

fn exhausted_health(attempt: &ExhaustedAttempt) -> HealthItemView {
    let mut item = attempt_health(
        &LoopAttempt {
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
        },
        "Needs attention",
    );
    item.identity = format!("attention:{}", attempt.key);
    item.primary = format!(
        "{} / {} exhausted",
        sanitize_text(&attempt.workflow_id),
        sanitize_text(&attempt.item_key)
    );
    item.detail.title = "Loop attention".to_string();
    if let Some(remediation) = attempt.remediation.clone().map(RemediationView::from) {
        item.detail.lines.extend([
            format!("Recovery display: {}", remediation.display),
            format!("Recovery argv: {}", remediation.inert_argv),
        ]);
    }
    item
}

fn occurrence_health(occurrence: &ScheduledOccurrence, section: &'static str) -> HealthItemView {
    let scheduled = occurrence_schedule(occurrence);
    let mut lines = vec![
        field("Occurrence", &occurrence.occurrence_id),
        field("Workflow", &occurrence.workflow_id),
        field("Owner", &occurrence.owner),
        field("Status", &occurrence.status),
        format!("Scheduled: {scheduled}"),
        format!(
            "Claim expires: {}",
            nonzero_timestamp(occurrence.claim_expires_at_ms)
        ),
        format!("Started: {}", nonzero_timestamp(occurrence.started_at_ms)),
    ];
    if occurrence.worker_invoked {
        lines.push("Worker: invoked".into());
    }
    push_optional(&mut lines, "Worktree", occurrence.worktree.as_deref());
    push_optional(&mut lines, "Error", occurrence.error.as_deref());
    if let Some(value) = occurrence.uses_shared_checkout {
        lines.push(format!("Shared checkout: {value}"));
    }
    lines.push(field(
        "Evidence",
        &format!("scripts/jig loop show {}", occurrence.occurrence_id),
    ));
    HealthItemView {
        identity: format!("occurrence:{section}:{}", occurrence.occurrence_id),
        section,
        primary: format!(
            "{} {}",
            sanitize_text(&occurrence.workflow_id),
            sanitize_text(&occurrence.status)
        ),
        secondary: scheduled,
        detail: DetailDocument::new("Scheduled occurrence", lines),
    }
}

fn occurrence_schedule(occurrence: &ScheduledOccurrence) -> String {
    if occurrence.scheduled_at_ms == 0 {
        if occurrence.started_at_ms == 0 {
            "Manual".to_string()
        } else {
            format!("Manual ({})", nonzero_timestamp(occurrence.started_at_ms))
        }
    } else {
        format_timestamp(Some(occurrence.scheduled_at_ms))
    }
}

fn nonzero_timestamp(timestamp_ms: u64) -> String {
    if timestamp_ms == 0 {
        "—".to_string()
    } else {
        format_timestamp(Some(timestamp_ms))
    }
}

fn state_error_health(error: &LoopStateError, duplicate_ordinal: usize) -> HealthItemView {
    let workflow = error
        .workflow_id
        .as_deref()
        .map(sanitize_text)
        .unwrap_or_else(|| "global".to_string());
    HealthItemView {
        identity: format!(
            "loop-error:{}:{}:{}:{duplicate_ordinal}",
            error.kind,
            error.workflow_id.as_deref().unwrap_or("global"),
            error.error
        ),
        section: "Loop errors",
        primary: format!("{} · {workflow}", sanitize_text(&error.kind)),
        secondary: sanitize_text(&error.error),
        detail: DetailDocument::new(
            "Loop state error",
            vec![
                field("Kind", &error.kind),
                format!("Workflow: {workflow}"),
                field("Error", &error.error),
            ],
        ),
    }
}

/// A producer recovery command shown as inert, shell-quoted text. The
/// dashboard never executes it.
struct RemediationView {
    display: String,
    inert_argv: String,
}

impl From<Remediation> for RemediationView {
    fn from(remediation: Remediation) -> Self {
        Self {
            display: sanitize_text(&remediation.display),
            inert_argv: remediation
                .argv
                .iter()
                .map(|arg| quote_argument(&sanitize_text(arg)))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

fn quote_argument(argument: &str) -> String {
    if !argument.is_empty()
        && argument
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_@%+=:,./-".contains(character))
    {
        argument.to_string()
    } else {
        format!("'{}'", argument.replace('\'', "'\"'\"'"))
    }
}
