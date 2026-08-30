use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::runtime::loops) enum OccurrenceAttentionScope {
    None,
    Workflow,
    SharedRepository,
}

pub(super) struct OccurrenceClaimConstraints {
    pub(super) attention_scope: OccurrenceAttentionScope,
    pub(super) block_newer_occurrences: bool,
    pub(super) block_retained_worktree: bool,
}

impl OccurrenceStore {
    pub(super) fn claim_with_constraints_at(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        attention_scope: OccurrenceAttentionScope,
        block_retained_worktree: bool,
    ) -> Result<OccurrenceClaim> {
        let occurrence_id = occurrence_id(workflow_id, scheduled_at_ms);
        self.claim_id_with_constraints_at(
            occurrence_id,
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            now,
            OccurrenceClaimConstraints {
                attention_scope,
                block_newer_occurrences: attention_scope != OccurrenceAttentionScope::None,
                block_retained_worktree,
            },
        )
    }

    pub(super) fn claim_id_with_constraints_at(
        &mut self,
        occurrence_id: String,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        constraints: OccurrenceClaimConstraints,
    ) -> Result<OccurrenceClaim> {
        self.with_locked(|store| {
            validate_schema(store)?;
            reconcile_stale_file(store, now);
            if let Some(existing) = store.occurrences.get(&occurrence_id) {
                return Ok(OccurrenceClaim::AlreadyRecorded(existing.clone()));
            }
            if let Some(attention) = latest_status_for_scope(
                store,
                workflow_id,
                constraints.attention_scope,
                OccurrenceStatus::NeedsAttention,
            ) {
                return Ok(OccurrenceClaim::BlockedByAttention(attention));
            }
            if let Some(running) = latest_status_for_scope(
                store,
                workflow_id,
                constraints.attention_scope,
                OccurrenceStatus::Running,
            ) {
                return Ok(OccurrenceClaim::BlockedByRunning(running));
            }
            if constraints.block_retained_worktree
                && let Some(retained) = latest_workflow_occurrence(
                    store,
                    workflow_id,
                    ScheduleOccurrence::has_retained_worktree,
                )
            {
                return Ok(OccurrenceClaim::BlockedByRetainedWorktree(retained));
            }
            if constraints.block_newer_occurrences
                && let Some(latest) = latest_workflow_occurrence(store, workflow_id, |_| true)
                && latest.scheduled_at_ms > scheduled_at_ms
            {
                return Ok(OccurrenceClaim::AlreadyRecorded(latest));
            }
            let record = ScheduleOccurrence {
                occurrence_id: occurrence_id.clone(),
                workflow_id: workflow_id.to_string(),
                scheduled_at_ms,
                owner: format!("{}-{}", std::process::id(), Ulid::new()),
                claim_expires_at_ms: expiry(now, ttl_seconds),
                started_at_ms: now,
                uses_shared_checkout: Some(
                    constraints.attention_scope == OccurrenceAttentionScope::SharedRepository,
                ),
                finished_at_ms: None,
                acknowledged_at_ms: None,
                status: OccurrenceStatus::Running,
                worker_receipt_id: None,
                worktree: None,
                error: None,
            };
            store.occurrences.insert(occurrence_id, record.clone());
            prune_history(store);
            Ok(OccurrenceClaim::Acquired(record))
        })
    }
}

fn latest_status_for_scope(
    store: &ScheduleFile,
    workflow_id: &str,
    scope: OccurrenceAttentionScope,
    status: OccurrenceStatus,
) -> Option<ScheduleOccurrence> {
    store
        .occurrences
        .values()
        .filter(|record| record.status == status)
        .filter(|record| match scope {
            OccurrenceAttentionScope::None => false,
            OccurrenceAttentionScope::Workflow => record.workflow_id == workflow_id,
            OccurrenceAttentionScope::SharedRepository => {
                record.workflow_id == workflow_id || record.uses_shared_checkout.unwrap_or(true)
            }
        })
        .max_by_key(|record| record.scheduled_at_ms)
        .cloned()
}

fn latest_workflow_occurrence(
    store: &ScheduleFile,
    workflow_id: &str,
    predicate: impl Fn(&ScheduleOccurrence) -> bool,
) -> Option<ScheduleOccurrence> {
    store
        .occurrences
        .values()
        .filter(|record| record.workflow_id == workflow_id && predicate(record))
        .max_by_key(|record| record.scheduled_at_ms)
        .cloned()
}
