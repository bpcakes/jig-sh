use super::*;

impl OccurrenceStore {
    pub(super) fn claim_with_constraints_at(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        scheduled_claim: bool,
        block_retained_worktree: bool,
    ) -> Result<OccurrenceClaim> {
        let occurrence_id = occurrence_id(workflow_id, scheduled_at_ms);
        self.with_locked(|store| {
            validate_schema(store)?;
            reconcile_stale_file(store, now);
            if let Some(existing) = store.occurrences.get(&occurrence_id) {
                return Ok(OccurrenceClaim::AlreadyRecorded(existing.clone()));
            }
            if scheduled_claim {
                if let Some(attention) = latest_matching_occurrence(store, workflow_id, |record| {
                    record.status == OccurrenceStatus::NeedsAttention
                }) {
                    return Ok(OccurrenceClaim::BlockedByAttention(attention));
                }
                if block_retained_worktree
                    && let Some(retained) = latest_matching_occurrence(
                        store,
                        workflow_id,
                        ScheduleOccurrence::has_retained_worktree,
                    )
                {
                    return Ok(OccurrenceClaim::BlockedByRetainedWorktree(retained));
                }
                if let Some(latest) = latest_matching_occurrence(store, workflow_id, |_| true)
                    && latest.scheduled_at_ms > scheduled_at_ms
                {
                    return Ok(OccurrenceClaim::AlreadyRecorded(latest));
                }
            }
            let record = ScheduleOccurrence {
                occurrence_id: occurrence_id.clone(),
                workflow_id: workflow_id.to_string(),
                scheduled_at_ms,
                owner: format!("{}-{}", std::process::id(), Ulid::new()),
                claim_expires_at_ms: expiry(now, ttl_seconds),
                started_at_ms: now,
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

fn latest_matching_occurrence(
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
