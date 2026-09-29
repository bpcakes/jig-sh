use super::*;

pub(super) fn recorder_loops(
    status: &StatusLoopObservation,
) -> Result<LoopObservation, SourceError> {
    let workflows = status
        .workflows
        .iter()
        .map(|value| LoopWorkflow {
            id: value.id.clone(),
            kind: value.kind.clone(),
            enabled: value.enabled,
            configured: value.configured,
            lease_ttl_seconds: value.lease_ttl_seconds,
            max_attempts: value.max_attempts,
            backoff_seconds: value.backoff_seconds,
            codex_home_configured: value.codex_home_configured.clone(),
            schedule: value.schedule.clone(),
            schedule_state: value.schedule_state.clone(),
            schedule_state_error: value.schedule_state_error.clone(),
            codex_task: value.codex_task.clone(),
        })
        .collect::<Vec<_>>();
    let attempts = status.attempts.iter().map(loop_attempt).collect::<Vec<_>>();
    let waiting = status
        .waiting_attempts
        .iter()
        .map(loop_attempt)
        .collect::<Vec<_>>();
    let exhausted = status
        .needs_attention
        .exhausted_attempts
        .iter()
        .map(|attempt| {
            let argv = vec![
                "scripts/jig".to_string(),
                "loop".to_string(),
                "clear-attempt".to_string(),
                "--workflow".to_string(),
                attempt.workflow_id.clone(),
                "--item".to_string(),
                attempt.item_key.clone(),
            ];
            ExhaustedAttempt {
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
                remediation: Some(Remediation {
                    display: shell_display(&argv),
                    argv,
                }),
            }
        })
        .collect::<Vec<_>>();
    Ok(LoopObservation {
        ok: status.ok,
        command: status.command.clone(),
        workflows: bounded_rows(workflows, LimitId::LoopWorkflows)?,
        leases: bounded_rows(status.leases.clone(), LimitId::LoopLeases)?,
        attempts: bounded_rows(attempts, LimitId::LoopAttempts)?,
        scheduled_occurrences: bounded_rows(
            status
                .scheduled_occurrences
                .iter()
                .map(scheduled_occurrence)
                .collect(),
            LimitId::LoopScheduledOccurrences,
        )?,
        waiting_attempts: bounded_rows(waiting, LimitId::LoopWaitingAttempts)?,
        state_error_count: status.state_error_count,
        state_errors: status.state_errors.clone(),
        needs_attention: LoopAttention {
            exhausted_attempts: bounded_rows(exhausted, LimitId::LoopExhaustedAttempts)?,
            scheduled_occurrences: bounded_rows(
                status
                    .needs_attention
                    .scheduled_occurrences
                    .iter()
                    .map(scheduled_occurrence)
                    .collect(),
                LimitId::LoopScheduledOccurrences,
            )?,
        },
    })
}

pub(super) fn loop_attempt(value: &StatusLoopAttempt) -> LoopAttempt {
    LoopAttempt {
        key: value.key.clone(),
        workflow_id: value.workflow_id.clone(),
        item_key: value.item_key.clone(),
        item_version: value.item_version.clone(),
        observed_item_version: value.observed_item_version.clone(),
        attempts: value.attempts,
        max_attempts: value.max_attempts,
        last_attempt_ms: value.last_attempt_ms,
        next_eligible_ms: value.next_eligible_ms,
        exhausted: value.exhausted,
        last_status: value.last_status.clone(),
    }
}

pub(super) fn scheduled_occurrence(value: &StatusScheduledOccurrence) -> ScheduledOccurrence {
    ScheduledOccurrence {
        occurrence_id: value.occurrence_id.clone(),
        workflow_id: value.workflow_id.clone(),
        scheduled_at_ms: value.scheduled_at_ms,
        owner: value.owner.clone(),
        claim_expires_at_ms: value.claim_expires_at_ms,
        started_at_ms: value.started_at_ms,
        uses_shared_checkout: value.uses_shared_checkout,
        finished_at_ms: value.finished_at_ms,
        acknowledged_at_ms: value.acknowledged_at_ms,
        status: value.status.clone(),
        worker_invoked: value.worker_invoked,
        worktree: value.worktree.clone(),
        error: value.error.clone(),
    }
}

pub(super) fn stable_identity(kind: &str, record: RawJsonlRecord<'_>) -> String {
    let digest = Sha256::digest(record.bytes);
    format!("{kind}:{}:{digest:x}", record.start_offset)
}

pub(super) struct NewestRows<T> {
    limit: usize,
    sequence: u64,
    rows: BTreeMap<(u64, std::cmp::Reverse<String>, u64), T>,
}

impl<T: Timestamped> NewestRows<T> {
    pub(super) const fn new(limit: usize) -> Self {
        Self {
            limit,
            sequence: 0,
            rows: BTreeMap::new(),
        }
    }

    pub(super) fn push(&mut self, row: T) {
        let key = (
            row.timestamp(),
            std::cmp::Reverse(row.tie_breaker().to_string()),
            self.sequence,
        );
        self.sequence = self.sequence.saturating_add(1);
        self.rows.insert(key, row);
        if self.rows.len() > self.limit {
            self.rows.pop_first();
        }
    }

    pub(super) fn into_rows(self) -> Vec<T> {
        self.rows.into_values().collect()
    }
}

pub(super) trait Timestamped {
    fn timestamp(&self) -> u64;
    fn tie_breaker(&self) -> &str;
}

impl Timestamped for TimelineRow {
    fn timestamp(&self) -> u64 {
        timeline_timestamp(self)
    }

    fn tie_breaker(&self) -> &str {
        self.stable_identity()
    }
}

pub(super) fn timeline_timestamp(row: &TimelineRow) -> u64 {
    row.timestamp_ms.unwrap_or(0)
}

impl Timestamped for Failure {
    fn timestamp(&self) -> u64 {
        self.ended_at_ms.unwrap_or(0)
    }

    fn tie_breaker(&self) -> &str {
        &self.run_id
    }
}

/// Keeps the final characters of `value`, where failure output is most useful.
pub(super) fn bounded_tail(value: &str, limit: LimitId) -> Result<BoundedText, SourceError> {
    let total = value.chars().count();
    let text = value
        .chars()
        .skip(total.saturating_sub(limit.ceiling()))
        .collect::<String>();
    BoundedText::for_limit(text, Some(total), limit).map_err(limit_error)
}

pub(super) fn bounded_rows<T>(
    mut rows: Vec<T>,
    limit: LimitId,
) -> Result<BoundedRows<T>, SourceError> {
    let total = rows.len();
    rows.truncate(limit.ceiling());
    BoundedRows::for_limit(rows, Some(total), limit).map_err(limit_error)
}

pub(super) fn stream_error(
    domain: CollectionDomain,
    result: anyhow::Result<impl Sized>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<SnapshotError>, SourceError> {
    match result {
        Ok(_) => Ok(None),
        Err(error)
            if crate::cancellation::is_status_collection_cancellation(&error) || cancelled() =>
        {
            Err(SourceError::Cancelled)
        }
        Err(error) => {
            let code = if error.downcast_ref::<JsonlRecordTooLarge>().is_some() {
                SnapshotErrorCode::RecordTooLarge
            } else if error
                .chain()
                .any(|cause| cause.downcast_ref::<serde_json::Error>().is_some())
            {
                SnapshotErrorCode::RecordDecodeFailed
            } else {
                SnapshotErrorCode::StreamReadFailed
            };
            Ok(Some(SnapshotError::new(
                domain,
                code,
                None,
                format!("{error:#}"),
            )))
        }
    }
}

pub(super) fn status_error(error: SnapshotError) -> StatusCollectionError {
    let (scope, code) = match error.scope() {
        "loops" => ("loops".to_string(), "loop_status_unavailable".to_string()),
        scope => (scope.to_string(), error.code().to_string()),
    };
    StatusCollectionError {
        scope,
        code,
        message: error.message().to_string(),
    }
}

pub(super) fn limit_error(error: impl std::fmt::Display) -> SourceError {
    SourceError::InternalContract {
        message: error.to_string(),
    }
}

pub(super) fn ensure_active(cancelled: &dyn Fn() -> bool) -> Result<(), SourceError> {
    if cancelled() {
        Err(SourceError::Cancelled)
    } else {
        Ok(())
    }
}

pub(in crate::ui::source) fn collection_error(
    error: anyhow::Error,
    cancelled: &dyn Fn() -> bool,
) -> SourceError {
    collection_error_for(CollectionDomain::Repository, error, cancelled)
}

pub(super) fn collection_error_for(
    domain: CollectionDomain,
    error: anyhow::Error,
    cancelled: &dyn Fn() -> bool,
) -> SourceError {
    if cancelled() || crate::cancellation::is_status_collection_cancellation(&error) {
        SourceError::Cancelled
    } else {
        SourceError::Collection {
            domain,
            message: format!("{error:#}"),
        }
    }
}

pub(super) fn shell_display(argv: &[String]) -> String {
    argv.iter()
        .map(|part| crate::shell::quote(part))
        .collect::<Vec<_>>()
        .join(" ")
}
