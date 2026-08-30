use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use tempfile::tempdir;

use super::super::*;
use super::write_loop_fixture_repo;

#[test]
fn occurrence_renewal_retry_delay_preserves_the_finish_window() {
    let interval = Duration::from_millis(300);

    assert_eq!(
        renewal_retry_delay(300, 900, interval),
        Some(Duration::from_millis(75))
    );
    assert_eq!(
        renewal_retry_delay(599, 900, interval),
        Some(Duration::from_millis(1))
    );
    assert_eq!(renewal_retry_delay(600, 900, interval), None);
}

#[test]
fn occurrence_renewal_preserves_the_first_unrecovered_error() {
    let failed = AtomicBool::new(false);
    let now_ms = Cell::new(0_u64);
    let calls = Cell::new(0_u64);

    let error = run_occurrence_renewal_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        || {
            let call = calls.get().saturating_add(1);
            calls.set(call);
            Err(RenewalAttemptError::Retryable(anyhow::anyhow!(
                "renewal failure {call}"
            )))
        },
        || now_ms.get(),
        |wait| {
            now_ms.set(
                now_ms
                    .get()
                    .saturating_add(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)),
            );
            Err(RecvTimeoutError::Timeout)
        },
    )
    .unwrap_err()
    .to_string();

    assert_eq!(error, "renewal failure 1");
    assert!(calls.get() > 1);
    assert!(failed.load(Ordering::Acquire));
}

#[test]
fn occurrence_renewal_stops_immediately_after_ownership_loss() {
    let failed = AtomicBool::new(false);
    let calls = Cell::new(0_u64);

    let error = run_occurrence_renewal_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        || {
            calls.set(calls.get().saturating_add(1));
            Err(RenewalAttemptError::Terminal(anyhow::anyhow!(
                RenewalOwnershipLost::new("ownership changed")
            )))
        },
        || 300,
        |_| Err(RecvTimeoutError::Timeout),
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "ownership changed");
    assert_eq!(calls.get(), 1);
    assert!(failed.load(Ordering::Acquire));
}

#[test]
fn removed_occurrence_is_classified_as_terminal_renewal_loss() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim("nightly", 100, 60).unwrap() else {
        panic!("claim should be acquired");
    };
    store
        .abandon_unexecuted(&claim.occurrence_id, &claim.owner)
        .unwrap();

    let error = store
        .renew_for_guard(&claim.occurrence_id, &claim.owner, 60)
        .unwrap_err();

    assert!(matches!(error, RenewalAttemptError::Terminal(_)));
}
