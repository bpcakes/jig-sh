use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use super::super::*;

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
            anyhow::bail!("renewal failure {call}")
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
