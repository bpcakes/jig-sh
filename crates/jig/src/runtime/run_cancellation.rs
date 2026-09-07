use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::context::RepoContext;

const DURABLE_CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(super) struct RunCancellationProbe {
    ctx: RepoContext,
    run_id: String,
    pub(super) signalled: Arc<AtomicBool>,
    last_durable_check: Mutex<Option<Instant>>,
    event_cursor: Mutex<crate::state::RunEventCursor>,
    poll_failure: Mutex<Option<String>>,
}

impl RunCancellationProbe {
    pub(super) fn new(
        ctx: RepoContext,
        run_id: String,
        event_cursor: crate::state::RunEventCursor,
    ) -> Self {
        Self {
            ctx,
            run_id,
            signalled: Arc::new(AtomicBool::new(false)),
            last_durable_check: Mutex::new(None),
            event_cursor: Mutex::new(event_cursor),
            poll_failure: Mutex::new(None),
        }
    }

    pub(super) fn signal(&self) {
        self.signalled.store(true, Ordering::Release);
    }

    pub(super) fn is_cancelled(&self) -> Result<bool> {
        self.is_cancelled_with(&|| false)
    }

    pub(super) fn is_cancelled_with(
        &self,
        foreground_cancelled: &dyn Fn() -> bool,
    ) -> Result<bool> {
        let signalled = || self.signalled.load(Ordering::Acquire) || foreground_cancelled();
        if let Some(message) = self
            .poll_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            bail!(message);
        }
        if signalled() {
            return Ok(true);
        }

        let now = Instant::now();
        let mut last_check = self
            .last_durable_check
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last_check
            .is_some_and(|last| now.duration_since(last) < DURABLE_CANCELLATION_POLL_INTERVAL)
        {
            return Ok(false);
        }
        *last_check = Some(now);
        drop(last_check);

        let mut cursor = match self.event_cursor.try_lock() {
            Ok(cursor) => cursor,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
        };
        let requested = crate::state::run_cancel_requested_since(
            &self.ctx,
            &self.run_id,
            &mut cursor,
            &signalled,
        );
        match requested {
            Ok(true) => {
                self.signal();
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(_) if signalled() => Ok(true),
            Err(error) => {
                let message = format!(
                    "failed to inspect durable cancellation state for run '{}': {error:#}",
                    self.run_id
                );
                *self
                    .poll_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message.clone());
                self.signal();
                bail!(message)
            }
        }
    }
}
