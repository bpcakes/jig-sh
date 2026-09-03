use super::*;

impl ScheduleOccurrence {
    fn is_unacknowledged_stale_reconciliation(&self) -> bool {
        self.status == OccurrenceStatus::NeedsAttention
            && self.finished_at_ms.is_some()
            && self.acknowledged_at_ms.is_none()
            && self.error.as_deref().is_some_and(|error| {
                error == STALE_RECONCILIATION_ERROR
                    || error.starts_with(STALE_RECONCILIATION_STAGED_ERROR_PREFIX)
            })
    }

    fn is_unexecuted_stale_reconciliation(&self) -> bool {
        self.is_unacknowledged_stale_reconciliation()
            && self.worker_receipt_id.is_none()
            && self.error.as_deref() == Some(STALE_RECONCILIATION_ERROR)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OwnedOccurrenceState {
    Running,
    StaleReconciled,
}

pub(super) fn require_running_owner(record: &ScheduleOccurrence, owner: &str) -> Result<()> {
    require_owner(record, owner)?;
    if record.status != OccurrenceStatus::Running {
        bail!(
            "Scheduled occurrence '{}' is already {}",
            record.occurrence_id,
            record.status
        );
    }
    Ok(())
}

pub(super) fn require_owned_occurrence_state(
    record: &ScheduleOccurrence,
    owner: &str,
) -> Result<OwnedOccurrenceState> {
    require_owner(record, owner)?;
    match record.status {
        OccurrenceStatus::Running => Ok(OwnedOccurrenceState::Running),
        OccurrenceStatus::NeedsAttention if record.is_unacknowledged_stale_reconciliation() => {
            Ok(OwnedOccurrenceState::StaleReconciled)
        }
        _ => already_finished(record),
    }
}

pub(super) fn require_unexecuted_owner(record: &ScheduleOccurrence, owner: &str) -> Result<()> {
    require_owner(record, owner)?;
    match record.status {
        OccurrenceStatus::Running => Ok(()),
        OccurrenceStatus::NeedsAttention if record.is_unexecuted_stale_reconciliation() => Ok(()),
        _ => already_finished(record),
    }
}

fn require_owner(record: &ScheduleOccurrence, owner: &str) -> Result<()> {
    if record.owner != owner {
        bail!(
            "Scheduled occurrence '{}' is owned by another dispatcher",
            record.occurrence_id
        );
    }
    Ok(())
}

fn already_finished<T>(record: &ScheduleOccurrence) -> Result<T> {
    bail!(
        "Scheduled occurrence '{}' is already {}",
        record.occurrence_id,
        record.status
    )
}
