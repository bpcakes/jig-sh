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

pub(super) struct OccurrenceClaimExecution<'a> {
    constraints: OccurrenceClaimConstraints,
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a> OccurrenceClaimExecution<'a> {
    pub(super) fn scheduled(
        attention_scope: OccurrenceAttentionScope,
        block_retained_worktree: bool,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Self {
        Self {
            constraints: OccurrenceClaimConstraints {
                attention_scope,
                block_newer_occurrences: attention_scope != OccurrenceAttentionScope::None,
                block_retained_worktree,
            },
            cancelled,
        }
    }

    pub(super) fn manual(
        attention_scope: OccurrenceAttentionScope,
        block_retained_worktree: bool,
        cancelled: &'a dyn Fn() -> bool,
    ) -> Self {
        Self {
            constraints: OccurrenceClaimConstraints {
                attention_scope,
                block_newer_occurrences: false,
                block_retained_worktree,
            },
            cancelled,
        }
    }
}

impl OccurrenceStore {
    #[cfg(test)]
    pub(super) fn claim_with_constraints_at(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        attention_scope: OccurrenceAttentionScope,
        block_retained_worktree: bool,
    ) -> Result<OccurrenceClaim> {
        self.claim_with_execution_at(
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            now,
            OccurrenceClaimExecution::scheduled(attention_scope, block_retained_worktree, &|| {
                false
            }),
        )
    }

    pub(super) fn claim_with_execution_at(
        &mut self,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        execution: OccurrenceClaimExecution<'_>,
    ) -> Result<OccurrenceClaim> {
        let occurrence_id = occurrence_id(workflow_id, scheduled_at_ms);
        self.claim_id_with_execution_at(
            occurrence_id,
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            now,
            execution,
        )
    }

    #[cfg(test)]
    pub(super) fn claim_id_with_constraints_at(
        &mut self,
        occurrence_id: String,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        constraints: OccurrenceClaimConstraints,
    ) -> Result<OccurrenceClaim> {
        self.claim_id_with_execution_at(
            occurrence_id,
            workflow_id,
            scheduled_at_ms,
            ttl_seconds,
            now,
            OccurrenceClaimExecution {
                constraints,
                cancelled: &|| false,
            },
        )
    }

    pub(super) fn claim_id_with_execution_at(
        &mut self,
        occurrence_id: String,
        workflow_id: &str,
        scheduled_at_ms: u64,
        ttl_seconds: u64,
        now: u64,
        execution: OccurrenceClaimExecution<'_>,
    ) -> Result<OccurrenceClaim> {
        self.with_locked_with_cancellation(execution.cancelled, |store| {
            validate_schema(store)?;
            reconcile_stale_file(store, now);
            if let Some(existing) = store.occurrences.get(&occurrence_id) {
                return Ok(OccurrenceClaim::AlreadyRecorded(existing.clone()));
            }
            if let Some(attention) = latest_status_for_scope(
                store,
                workflow_id,
                execution.constraints.attention_scope,
                OccurrenceStatus::NeedsAttention,
            ) {
                return Ok(OccurrenceClaim::BlockedByAttention(attention));
            }
            if let Some(running) = latest_status_for_scope(
                store,
                workflow_id,
                execution.constraints.attention_scope,
                OccurrenceStatus::Running,
            ) {
                return Ok(OccurrenceClaim::BlockedByRunning(running));
            }
            if let Some(retained) = latest_retained_worktree_for_claim(
                store,
                workflow_id,
                execution.constraints.attention_scope,
                execution.constraints.block_retained_worktree,
            ) {
                return Ok(OccurrenceClaim::BlockedByRetainedWorktree(retained));
            }
            if execution.constraints.block_newer_occurrences
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
                    execution.constraints.attention_scope
                        == OccurrenceAttentionScope::SharedRepository,
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
        .filter(|record| occurrence_claims_overlap(record, workflow_id, scope))
        .max_by_key(|record| record.scheduled_at_ms)
        .cloned()
}

fn occurrence_claims_overlap(
    record: &ScheduleOccurrence,
    workflow_id: &str,
    scope: OccurrenceAttentionScope,
) -> bool {
    match scope {
        OccurrenceAttentionScope::None => false,
        OccurrenceAttentionScope::Workflow => {
            record.workflow_id == workflow_id || record.uses_shared_checkout.unwrap_or(true)
        }
        OccurrenceAttentionScope::SharedRepository => true,
    }
}

fn latest_retained_worktree_for_claim(
    store: &ScheduleFile,
    workflow_id: &str,
    scope: OccurrenceAttentionScope,
    block_workflow_retained_worktree: bool,
) -> Option<ScheduleOccurrence> {
    let block_every_worktree = scope == OccurrenceAttentionScope::SharedRepository;
    if !block_every_worktree && !block_workflow_retained_worktree {
        return None;
    }
    store
        .occurrences
        .values()
        .filter(|record| {
            (block_every_worktree || record.workflow_id == workflow_id)
                && record.has_retained_worktree()
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
