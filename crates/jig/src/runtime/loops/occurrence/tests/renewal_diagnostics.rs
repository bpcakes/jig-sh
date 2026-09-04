use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::super::*;
use super::write_loop_fixture_repo;
use crate::runtime::loops::state::{LOOP_RUNTIME_DIR, with_exclusive_file_lock};

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
        |_| {
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
fn stopping_before_the_failure_window_does_not_latch_a_pending_transient_error() {
    let failed = AtomicBool::new(false);
    let now_ms = Cell::new(0_u64);
    let waits = Cell::new(0_u64);

    run_occurrence_renewal_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        |_| {
            Err(RenewalAttemptError::Retryable(anyhow::anyhow!(
                "injected transient renewal failure"
            )))
        },
        || now_ms.get(),
        |wait| {
            let count = waits.get().saturating_add(1);
            waits.set(count);
            now_ms.set(
                now_ms
                    .get()
                    .saturating_add(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)),
            );
            if count == 1 {
                Err(RecvTimeoutError::Timeout)
            } else {
                Ok(())
            }
        },
    )
    .unwrap();

    assert_eq!(waits.get(), 2);
    assert!(!failed.load(Ordering::Acquire));
}

#[test]
fn occurrence_renewal_stops_immediately_after_ownership_loss() {
    let failed = AtomicBool::new(false);
    let calls = Cell::new(0_u64);

    let error = run_occurrence_renewal_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        |_| {
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
        .renew_for_guard(
            &claim.occurrence_id,
            &claim.owner,
            60,
            crate::runtime::loops::state::loop_state_lock_deadline(),
        )
        .unwrap_err();

    assert!(matches!(error, RenewalAttemptError::Terminal(_)));
}

#[test]
fn occurrence_renewal_samples_expiry_after_acquiring_the_schedule_lock() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    let lock_path = runtime_dir.join("schedule.lock");
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        with_exclusive_file_lock(&runtime_dir, &lock_path, || {
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
    });
    locked_rx.recv().unwrap();

    let (clock_tx, clock_rx) = std::sync::mpsc::channel();
    let occurrence_id = claim.occurrence_id.clone();
    let owner = claim.owner;
    let renewal = std::thread::spawn(move || {
        store.renew_with_lock_deadline(
            &occurrence_id,
            &owner,
            2,
            Instant::now() + Duration::from_secs(1),
            || {
                clock_tx.send(()).unwrap();
                2_000
            },
        )
    });

    assert!(clock_rx.recv_timeout(Duration::from_millis(100)).is_err());
    release_tx.send(()).unwrap();
    clock_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let renewed = renewal.join().unwrap().unwrap();
    holder.join().unwrap();

    assert_eq!(renewed.claim_expires_at_ms, 4_000);
}
