use std::sync::mpsc::RecvTimeoutError;

use super::*;

pub(super) fn run_occurrence_renewal(
    receiver: &mpsc::Receiver<()>,
    interval: Duration,
    claim_expires_at_ms: u64,
    renewal_failed: &AtomicBool,
    renew: impl FnMut(Instant) -> std::result::Result<u64, RenewalAttemptError>,
    now: impl Fn() -> u64,
) -> Result<()> {
    run_occurrence_renewal_with_wait(
        interval,
        claim_expires_at_ms,
        renewal_failed,
        renew,
        now,
        |wait| receiver.recv_timeout(wait),
    )
}

pub(super) fn run_occurrence_renewal_with_wait(
    interval: Duration,
    claim_expires_at_ms: u64,
    renewal_failed: &AtomicBool,
    renew: impl FnMut(Instant) -> std::result::Result<u64, RenewalAttemptError>,
    now: impl Fn() -> u64,
    wait_for_stop: impl FnMut(Duration) -> std::result::Result<(), RecvTimeoutError>,
) -> Result<()> {
    run_with_wait(
        interval,
        claim_expires_at_ms,
        renewal_failed,
        renew,
        now,
        wait_for_stop,
    )
}
