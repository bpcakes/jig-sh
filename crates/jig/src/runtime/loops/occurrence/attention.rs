use super::*;

impl ScheduleOccurrence {
    pub(in crate::runtime::loops) fn requires_attention_at(&self, checked_at_ms: u64) -> bool {
        self.status == OccurrenceStatus::NeedsAttention
            || (self.status == OccurrenceStatus::Running
                && self.claim_expires_at_ms <= checked_at_ms)
    }
}

impl OccurrenceStore {
    #[cfg(test)]
    pub(in crate::runtime::loops) fn acknowledge(
        &mut self,
        occurrence_id: &str,
    ) -> Result<OccurrenceAcknowledgement> {
        self.acknowledge_with_clock(occurrence_id, now_ms)
    }

    pub(in crate::runtime::loops) fn acknowledge_and_then<T>(
        &mut self,
        occurrence_id: &str,
        cancelled: &dyn Fn() -> bool,
        after_commit: impl FnOnce(&ScheduleOccurrence, bool, Instant) -> Result<T>,
    ) -> Result<(OccurrenceAcknowledgement, T)> {
        self.persistence.with_locked_compensating(
            cancelled,
            |store| acknowledge_record(store, occurrence_id, now_ms()),
            |acknowledgement, deadline| match acknowledgement {
                OccurrenceAcknowledgement::Acknowledged(occurrence) => {
                    after_commit(occurrence, true, deadline)
                }
                OccurrenceAcknowledgement::AlreadyAcknowledged(occurrence) => {
                    after_commit(occurrence, false, deadline)
                }
            },
        )
    }

    #[cfg(test)]
    pub(super) fn acknowledge_at(
        &mut self,
        occurrence_id: &str,
        now: u64,
    ) -> Result<OccurrenceAcknowledgement> {
        self.acknowledge_with_clock(occurrence_id, || now)
    }

    #[cfg(test)]
    fn acknowledge_with_clock(
        &mut self,
        occurrence_id: &str,
        now: impl FnOnce() -> u64,
    ) -> Result<OccurrenceAcknowledgement> {
        self.with_locked(|store| acknowledge_record(store, occurrence_id, now()))
    }
}

fn acknowledge_record(
    store: &mut ScheduleFile,
    occurrence_id: &str,
    now: u64,
) -> Result<OccurrenceAcknowledgement> {
    let record = store
        .occurrences
        .get_mut(occurrence_id)
        .ok_or_else(|| anyhow::anyhow!("Scheduled occurrence not found: {occurrence_id}"))?;
    if record.status == OccurrenceStatus::Running && record.requires_attention_at(now) {
        mark_expired_claim(record, now, None);
    }
    match record.status {
        OccurrenceStatus::NeedsAttention => {
            record.status = OccurrenceStatus::Acknowledged;
            record.acknowledged_at_ms = Some(now);
            let acknowledged = record.clone();
            prune_history(store);
            Ok(OccurrenceAcknowledgement::Acknowledged(acknowledged))
        }
        OccurrenceStatus::Acknowledged => Ok(OccurrenceAcknowledgement::AlreadyAcknowledged(
            record.clone(),
        )),
        status => bail!(
            "Scheduled occurrence '{occurrence_id}' is {status}; only needs_attention occurrences can be acknowledged"
        ),
    }
}
