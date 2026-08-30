use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use serde_json::Value;
use tempfile::tempdir;

use super::*;

#[test]
fn dispatch_preflight_fails_closed_for_lease_corruption_and_recovers_attempts() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let cache_dir = temp.path().join(LOOP_CACHE_DIR);
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("leases.json"), b"{").unwrap();
    fs::write(cache_dir.join("attempts.json"), b"{").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = prepare_disposable_state_for_dispatch(&ctx)
        .unwrap_err()
        .to_string();

    assert!(error.contains("leases.json"), "{error}");
    assert_eq!(fs::read(cache_dir.join("leases.json")).unwrap(), b"{");
    fs::write(cache_dir.join("leases.json"), br#"{"leases":{}}"#).unwrap();
    let recovery = prepare_disposable_state_for_dispatch(&ctx).unwrap();
    assert!(recovery.attempt_cache_reset);
    let mut leases = LeaseStore::new(&ctx);
    assert!(matches!(
        leases.acquire("workflow:ExampleProject", 60).unwrap(),
        LeaseAcquire::Acquired(_)
    ));
    let mut attempts = AttemptStore::new(&ctx);
    assert!(attempts.snapshot().unwrap().is_empty());

    serde_json::from_slice::<Value>(&fs::read(cache_dir.join("attempts.json")).unwrap()).unwrap();
}

#[test]
fn lease_guard_cannot_finalize_after_another_owner_reacquires_the_key() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = LeaseStore::new(&ctx);
    let LeaseAcquire::Acquired(first) = store.acquire("workflow:paused", 60).unwrap() else {
        panic!("expected first lease acquisition");
    };
    let guard = LeaseGuard::start_for_test(
        store.clone(),
        "workflow:paused",
        &first,
        60,
        Duration::from_secs(30),
    )
    .unwrap();
    store.release("workflow:paused", &first.owner).unwrap();
    let LeaseAcquire::Acquired(second) = store.acquire("workflow:paused", 60).unwrap() else {
        panic!("expected replacement lease acquisition");
    };

    let error = guard.finish().unwrap_err().to_string();

    assert!(error.contains("owned by another worker"), "{error}");
    let LeaseAcquire::Held(current) = store.acquire("workflow:paused", 60).unwrap() else {
        panic!("replacement lease must remain held");
    };
    assert_eq!(current.owner, second.owner);
}

#[test]
fn release_rejects_an_expired_lease() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = LeaseStore::new(&ctx);
    let LeaseAcquire::Acquired(lease) = store.acquire("workflow:expired", 60).unwrap() else {
        panic!("expected lease acquisition");
    };

    let error = store
        .release_at("workflow:expired", &lease.owner, lease.expires_at_ms)
        .unwrap_err()
        .to_string();

    assert!(error.contains("expired before release"), "{error}");
}

#[test]
fn lease_renewal_is_owner_checked_and_cannot_revive_expired_lease() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = LeaseStore::new(&ctx);
    let LeaseAcquire::Acquired(lease) = store.acquire("workflow:owner", 60).unwrap() else {
        panic!("expected lease acquisition");
    };

    assert!(
        store
            .renew_at(
                "workflow:owner",
                "another-owner",
                60,
                lease.acquired_at_ms + 1
            )
            .unwrap_err()
            .to_string()
            .contains("owned by another worker")
    );
    assert!(
        store
            .renew_at("workflow:owner", &lease.owner, 60, lease.expires_at_ms)
            .unwrap_err()
            .to_string()
            .contains("expired before renewal")
    );
}

#[test]
fn lease_renewal_retries_transient_state_failure_before_expiry() {
    use std::cell::{Cell, RefCell};

    let failed = AtomicBool::new(false);
    let calls = AtomicUsize::new(0);
    let now_ms = Cell::new(0_u64);
    let waits = RefCell::new(Vec::new());

    super::super::renewal::run_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(super::super::renewal::RenewalAttemptError::Retryable(
                    anyhow!("injected transient lease state failure"),
                ));
            }
            Ok(1_800)
        },
        || now_ms.get(),
        |wait| {
            if calls.load(Ordering::SeqCst) >= 2 {
                return Err(RecvTimeoutError::Disconnected);
            }
            waits.borrow_mut().push(wait);
            now_ms.set(
                now_ms
                    .get()
                    .saturating_add(u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)),
            );
            Err(RecvTimeoutError::Timeout)
        },
    )
    .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        waits.into_inner(),
        [Duration::from_millis(300), Duration::from_millis(75)]
    );
    assert!(!failed.load(Ordering::Acquire));
}

fn write_loop_fixture_repo(root: &Path) {
    crate::test_env::TestRepoBuilder::new(root)
        .required_commands(Vec::<String>::new())
        .write();
}
