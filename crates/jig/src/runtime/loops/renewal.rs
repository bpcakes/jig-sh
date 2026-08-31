use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::state::LOOP_STATE_LOCK_TIMEOUT;

#[derive(Debug)]
pub(super) struct RenewalOwnershipLost(String);

impl RenewalOwnershipLost {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for RenewalOwnershipLost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RenewalOwnershipLost {}

pub(super) enum RenewalAttemptError {
    Terminal(anyhow::Error),
    Retryable(anyhow::Error),
}

pub(super) fn renewal_interval(ttl_seconds: u64) -> Duration {
    let ttl_ms = ttl_seconds.saturating_mul(1_000);
    Duration::from_millis((ttl_ms / 3).max(1))
}

pub(super) fn run_with_wait(
    interval: Duration,
    mut expires_at_ms: u64,
    renewal_failed: &AtomicBool,
    mut renew: impl FnMut(Instant) -> std::result::Result<u64, RenewalAttemptError>,
    now: impl Fn() -> u64,
    mut wait_for_stop: impl FnMut(Duration) -> std::result::Result<(), RecvTimeoutError>,
) -> Result<()> {
    let mut wait = interval;
    let mut pending_error = None;
    loop {
        match wait_for_stop(wait) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if expires_at_ms <= now() {
                    renewal_failed.store(true, Ordering::Release);
                }
                return Ok(());
            }
            Err(RecvTimeoutError::Timeout) => {
                if pending_error.is_some() && failure_window_reached(now(), expires_at_ms, interval)
                {
                    renewal_failed.store(true, Ordering::Release);
                    return Err(pending_error
                        .take()
                        .expect("a pending renewal error was checked above"));
                }
                let now_ms = now();
                let deadline =
                    renewal_lock_deadline(Instant::now(), now_ms, expires_at_ms, interval);
                match renew(deadline) {
                    Ok(renewed_expires_at_ms) => {
                        expires_at_ms = renewed_expires_at_ms;
                        pending_error = None;
                        wait = interval;
                    }
                    Err(RenewalAttemptError::Terminal(error)) => {
                        renewal_failed.store(true, Ordering::Release);
                        return Err(error);
                    }
                    Err(RenewalAttemptError::Retryable(error)) => {
                        let Some(retry_delay) = retry_delay(now(), expires_at_ms, interval) else {
                            renewal_failed.store(true, Ordering::Release);
                            return Err(error);
                        };
                        pending_error.get_or_insert(error);
                        wait = retry_delay;
                    }
                }
            }
        }
    }
}

fn renewal_lock_deadline(
    now: Instant,
    now_ms: u64,
    expires_at_ms: u64,
    interval: Duration,
) -> Instant {
    now.checked_add(renewal_lock_wait(now_ms, expires_at_ms, interval))
        .unwrap_or(now)
}

fn renewal_lock_wait(now_ms: u64, expires_at_ms: u64, interval: Duration) -> Duration {
    let remaining_ms = expires_at_ms.saturating_sub(now_ms);
    let wait_budget_ms = remaining_ms.saturating_sub(cancellation_window_ms(interval));
    Duration::from_millis(wait_budget_ms).min(LOOP_STATE_LOCK_TIMEOUT)
}

pub(super) fn retry_delay(now_ms: u64, expires_at_ms: u64, interval: Duration) -> Option<Duration> {
    let cancellation_window_ms = cancellation_window_ms(interval);
    let remaining_ms = expires_at_ms.saturating_sub(now_ms);
    let retry_budget_ms = remaining_ms.checked_sub(cancellation_window_ms)?;
    if retry_budget_ms == 0 {
        return None;
    }
    let normal_retry_ms = (cancellation_window_ms / 4).max(1);
    Some(Duration::from_millis(normal_retry_ms.min(retry_budget_ms)))
}

fn failure_window_reached(now_ms: u64, expires_at_ms: u64, interval: Duration) -> bool {
    let cancellation_window_ms = cancellation_window_ms(interval);
    expires_at_ms.saturating_sub(now_ms) <= cancellation_window_ms
}

fn cancellation_window_ms(interval: Duration) -> u64 {
    u64::try_from(interval.as_millis())
        .unwrap_or(u64::MAX)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_lock_wait_preserves_the_cancellation_window() {
        assert_eq!(
            renewal_lock_wait(100, 1_000, Duration::from_millis(300)),
            Duration::from_millis(600)
        );
        assert_eq!(
            renewal_lock_wait(700, 1_000, Duration::from_millis(300)),
            Duration::ZERO
        );
    }

    #[test]
    fn renewal_lock_wait_never_exceeds_the_global_lock_timeout() {
        assert_eq!(
            renewal_lock_wait(0, 120_000, Duration::from_secs(10)),
            LOOP_STATE_LOCK_TIMEOUT
        );
    }
}
