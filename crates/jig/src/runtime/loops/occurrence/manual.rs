use super::claim::{OccurrenceAttentionScope, OccurrenceClaimExecution};
use super::*;

pub(super) const MANUAL_OCCURRENCE_SCHEDULED_AT_MS: u64 = 0;

impl OccurrenceGuard {
    pub(in crate::runtime::loops) fn stage_manual(
        &mut self,
        finish: OccurrenceFinish<'_>,
    ) -> Result<ScheduleOccurrence> {
        self.store
            .stage_manual(&self.occurrence_id, &self.owner, finish)
    }

    pub(in crate::runtime::loops) fn finish_manual(
        self,
        finish: OccurrenceFinish<'_>,
        retain: bool,
    ) -> Result<OccurrenceFinalization> {
        self.finalize(|store, occurrence_id, owner| {
            store.finish_manual(occurrence_id, owner, finish, retain)
        })
    }
}

impl OccurrenceStore {
    #[cfg(test)]
    pub(in crate::runtime::loops) fn claim_manual(
        &mut self,
        workflow_id: &str,
        item_key: &str,
        ttl_seconds: u64,
        attention_scope: OccurrenceAttentionScope,
        block_retained_worktree: bool,
    ) -> Result<OccurrenceClaim> {
        self.claim_manual_with_cancellation(
            workflow_id,
            item_key,
            ttl_seconds,
            attention_scope,
            block_retained_worktree,
            &|| false,
        )
    }

    pub(in crate::runtime::loops) fn claim_manual_with_cancellation(
        &mut self,
        workflow_id: &str,
        item_key: &str,
        ttl_seconds: u64,
        attention_scope: OccurrenceAttentionScope,
        block_retained_worktree: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<OccurrenceClaim> {
        self.claim_id_with_execution_at(
            format!("{workflow_id}@manual:{item_key}"),
            workflow_id,
            MANUAL_OCCURRENCE_SCHEDULED_AT_MS,
            ttl_seconds,
            now_ms(),
            OccurrenceClaimExecution::manual(attention_scope, block_retained_worktree, cancelled),
        )
    }

    fn finish_manual(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        finish: OccurrenceFinish<'_>,
        retain: bool,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let now = now_ms();
            let record = store
                .occurrences
                .get_mut(occurrence_id)
                .ok_or_else(|| anyhow::anyhow!("Loop occurrence not found: {occurrence_id}"))?;
            match require_owned_occurrence_state(record, owner)? {
                OwnedOccurrenceState::Running => {}
                OwnedOccurrenceState::StaleReconciled => {
                    record_expired_finish_evidence(record, &finish);
                    return Ok(record.clone());
                }
            }
            if record.claim_expires_at_ms <= now {
                mark_expired_claim(record, now, Some(&finish));
                return Ok(record.clone());
            }
            if retain {
                record.status = finish.outcome.status();
                record.finished_at_ms = Some(now);
                record.worker_receipt_id = finish.worker_receipt_id.map(str::to_string);
                record.worktree = finish.worktree.map(str::to_string);
                record.error = finish.error.map(bounded_error);
                let finished = record.clone();
                prune_history(store);
                Ok(finished)
            } else {
                store
                    .occurrences
                    .remove(occurrence_id)
                    .ok_or_else(|| anyhow::anyhow!("Loop occurrence not found: {occurrence_id}"))
            }
        })
    }

    pub(super) fn stage_manual(
        &mut self,
        occurrence_id: &str,
        owner: &str,
        finish: OccurrenceFinish<'_>,
    ) -> Result<ScheduleOccurrence> {
        self.with_locked(|store| {
            let now = now_ms();
            let record = store
                .occurrences
                .get_mut(occurrence_id)
                .ok_or_else(|| anyhow::anyhow!("Loop occurrence not found: {occurrence_id}"))?;
            match require_owned_occurrence_state(record, owner)? {
                OwnedOccurrenceState::Running => {}
                OwnedOccurrenceState::StaleReconciled => {
                    record_expired_finish_evidence(record, &finish);
                    return Ok(record.clone());
                }
            }
            if record.claim_expires_at_ms <= now {
                mark_expired_claim(record, now, Some(&finish));
                return Ok(record.clone());
            }
            record.worker_receipt_id = finish.worker_receipt_id.map(str::to_string);
            record.worktree = finish.worktree.map(str::to_string);
            record.error = finish.error.map(bounded_error);
            Ok(record.clone())
        })
    }
}
