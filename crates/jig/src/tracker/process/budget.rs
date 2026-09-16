use std::time::{Duration, Instant};

use super::super::{TrackerError, TrackerOperation};

#[derive(Clone, Copy)]
pub(crate) struct OperationBudget {
    deadline: Instant,
}

impl OperationBudget {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            deadline: Instant::now()
                .checked_add(timeout)
                .unwrap_or_else(Instant::now),
        }
    }

    pub(crate) fn checkpoint_before_spawn(
        self,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), TrackerError> {
        if cancelled() {
            return Err(TrackerError::CancelledBeforeStart { operation });
        }
        if self
            .deadline
            .saturating_duration_since(Instant::now())
            .is_zero()
        {
            return Err(TrackerError::TimedOut { operation });
        }
        Ok(())
    }

    pub(super) fn remaining_before_spawn(
        self,
        operation: TrackerOperation,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Duration, TrackerError> {
        self.checkpoint_before_spawn(operation, cancelled)?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }
}
