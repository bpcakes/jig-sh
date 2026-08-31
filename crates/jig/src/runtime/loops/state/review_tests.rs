use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use serde_json::Value;
use tempfile::tempdir;

use super::*;
use crate::runtime::loops::workflow::NOOP_STATUS_KIND;

#[cfg(unix)]
#[test]
fn loop_cache_directory_symlink_cannot_redirect_parent_state_mutations() {
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let cache_parent = temp.path().join(".agent/.cache");
    fs::create_dir_all(&cache_parent).unwrap();
    let outside_temp = outside.path().join("leases.tmp-ExampleOutside");
    fs::write(&outside_temp, b"outside").unwrap();
    symlink(outside.path(), cache_parent.join("loop")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = match LeaseStore::new(&ctx).acquire("workflow:ExampleProject", 60) {
        Ok(_) => panic!("a symlinked loop cache must be rejected"),
        Err(error) => error.to_string(),
    };

    assert!(error.contains("without following links"), "{error}");
    assert_eq!(fs::read(&outside_temp).unwrap(), b"outside");
    assert!(!outside.path().join("leases.json").exists());
    assert!(!outside.path().join("leases.lock").exists());
}

#[test]
fn dispatch_preflight_fails_closed_for_lease_corruption_and_recovers_attempts() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let cache_dir = temp.path().join(LOOP_CACHE_DIR);
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("leases.json"), b"{").unwrap();
    fs::write(cache_dir.join("attempts.json"), b"{").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = prepare_coordination_state_for_dispatch(&ctx)
        .unwrap_err()
        .to_string();

    assert!(error.contains("leases.json"), "{error}");
    assert_eq!(fs::read(cache_dir.join("leases.json")).unwrap(), b"{");
    fs::write(cache_dir.join("leases.json"), br#"{"leases":{}}"#).unwrap();
    let recovery = prepare_coordination_state_for_dispatch(&ctx).unwrap();
    assert!(recovery.attempt_cache_reset);
    let mut leases = LeaseStore::new(&ctx);
    assert!(matches!(
        leases.acquire("workflow:ExampleProject", 60).unwrap(),
        LeaseAcquire::Acquired(_)
    ));
    let attempts = AttemptStore::new(&ctx);
    assert!(attempts.snapshot().unwrap().is_empty());

    serde_json::from_slice::<Value>(&fs::read(cache_dir.join("attempts.json")).unwrap()).unwrap();
}

#[test]
fn git_state_migrates_legacy_leases_and_attempts_to_protected_authority() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut legacy_leases = LeaseStore::new(&legacy_ctx);
    let LeaseAcquire::Acquired(lease) = legacy_leases
        .acquire("workflow:ExampleProject", 60)
        .unwrap()
    else {
        panic!("expected legacy lease acquisition");
    };
    let workflow = example_workflow();
    AttemptStore::new(&legacy_ctx)
        .record_attempt_for_transition(&workflow, "pr-17", Some("observed"), None, "failed")
        .unwrap();
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    prepare_coordination_state_for_dispatch(&ctx).unwrap();

    let mut leases = LeaseStore::new(&ctx);
    let LeaseAcquire::Held(migrated) = leases.acquire("workflow:ExampleProject", 60).unwrap()
    else {
        panic!("protected authority must retain the legacy lease");
    };
    assert_eq!(migrated.owner, lease.owner);
    let attempts = AttemptStore::new(&ctx);
    assert_eq!(attempts.snapshot().unwrap().len(), 1);

    let protected_leases = leases
        .persistence
        .protected_path()
        .unwrap()
        .expect("Git repositories need protected lease authority");
    let protected_attempts = attempts
        .persistence
        .protected_path()
        .unwrap()
        .expect("Git repositories need protected attempt authority");
    assert!(protected_leases.is_file());
    assert!(protected_attempts.is_file());
    assert!(
        serde_json::from_slice::<LeaseFile>(&fs::read(leases.persistence.legacy_path()).unwrap())
            .is_err(),
        "the legacy lease file must block older runtimes after cutover"
    );
}

#[test]
fn writable_checkout_cache_tampering_cannot_change_protected_loop_authority() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut leases = LeaseStore::new(&ctx);
    let LeaseAcquire::Acquired(lease) = leases.acquire("checkout:repo", 60).unwrap() else {
        panic!("expected protected lease acquisition");
    };
    let workflow = example_workflow();
    let mut attempts = AttemptStore::new(&ctx);
    attempts
        .record_attempt_for_transition(&workflow, "pr-17", Some("observed"), None, "failed")
        .unwrap();

    fs::write(leases.persistence.legacy_path(), br#"{"leases":{}}"#).unwrap();
    fs::write(attempts.persistence.legacy_path(), br#"{"attempts":{}}"#).unwrap();

    let LeaseAcquire::Held(current) = leases.acquire("checkout:repo", 60).unwrap() else {
        panic!("checkout-local tampering must not release protected lease authority");
    };
    assert_eq!(current.owner, lease.owner);
    assert_eq!(attempts.snapshot().unwrap().len(), 1);
}

#[test]
fn git_dispatch_preflight_fails_closed_for_protected_leases_and_resets_attempts() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut leases = LeaseStore::new(&ctx);
    leases.acquire("workflow:ExampleProject", 60).unwrap();
    let mut attempts = AttemptStore::new(&ctx);
    attempts
        .record_attempt_for_transition(
            &example_workflow(),
            "pr-17",
            Some("observed"),
            None,
            "failed",
        )
        .unwrap();
    let lease_path = leases.persistence.protected_path().unwrap().unwrap();
    let attempt_path = attempts.persistence.protected_path().unwrap().unwrap();
    let valid_leases = fs::read(lease_path).unwrap();
    fs::write(lease_path, b"{").unwrap();

    let error = prepare_coordination_state_for_dispatch(&ctx)
        .unwrap_err()
        .to_string();

    assert!(error.contains("leases.json"), "{error}");
    assert_eq!(fs::read(lease_path).unwrap(), b"{");
    fs::write(lease_path, valid_leases).unwrap();
    fs::write(attempt_path, b"{").unwrap();

    let recovery = prepare_coordination_state_for_dispatch(&ctx).unwrap();

    assert!(recovery.attempt_cache_reset);
    assert!(AttemptStore::new(&ctx).snapshot().unwrap().is_empty());
}

#[test]
fn protected_coordination_state_rejects_an_unknown_schema_without_rewriting_it() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut leases = LeaseStore::new(&ctx);
    let protected_path = leases
        .persistence
        .protected_path()
        .unwrap()
        .unwrap()
        .to_path_buf();
    fs::create_dir_all(protected_path.parent().unwrap()).unwrap();
    let unsupported = br#"{"schema_version":2,"state":{"leases":{}}}"#;
    fs::write(&protected_path, unsupported).unwrap();

    let error = leases.active_leases().unwrap_err().to_string();

    assert!(
        error.contains("Unsupported protected loop state schema version 2"),
        "{error}"
    );
    assert_eq!(fs::read(protected_path).unwrap(), unsupported);
}

#[test]
fn attempt_decision_read_waits_for_compensating_rollback() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let workflow = example_workflow();
    let mut attempts = AttemptStore::new(&ctx);
    attempts
        .record_attempt_for_transition(&workflow, "pr-17", Some("observed"), None, "failed")
        .unwrap();
    let (start_read, read_start) = std::sync::mpsc::channel();
    let (read_result, result_read) = std::sync::mpsc::channel();
    let root = temp.path().to_path_buf();
    let reader = std::thread::spawn(move || {
        read_start.recv().unwrap();
        let ctx = RepoContext::load_from(&root).unwrap();
        read_result
            .send(AttemptStore::new(&ctx).get("ExampleProject", "pr-17"))
            .unwrap();
    });

    let error = attempts
        .clear_attempt_and_then("ExampleProject", "pr-17", |cleared, _| {
            assert!(cleared);
            start_read.send(()).unwrap();
            assert!(matches!(
                result_read.recv_timeout(Duration::from_millis(100)),
                Err(RecvTimeoutError::Timeout)
            ));
            Err::<(), _>(anyhow!("injected receipt failure"))
        })
        .unwrap_err();

    assert_eq!(error.to_string(), "injected receipt failure");
    let restored = result_read
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(restored.is_some());
    reader.join().unwrap();
}

#[test]
fn legacy_migration_marker_cannot_redirect_state_to_another_authority() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut leases = LeaseStore::new(&ctx);
    let legacy_path = leases.persistence.legacy_path().to_path_buf();
    fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    fs::write(
        legacy_path,
        br#"{"schema_version":1,"protected_state_path":"jig/loop/attempts.json","state":{"leases":{}}}"#,
    )
    .unwrap();

    let error = match leases.acquire("workflow:ExampleProject", 60) {
        Ok(_) => panic!("cross-authority migration marker must fail closed"),
        Err(error) => error.to_string(),
    };

    assert!(
        error.contains("points to jig/loop/attempts.json; expected jig/loop/leases.json"),
        "{error}"
    );
    assert!(
        !leases
            .persistence
            .protected_path()
            .unwrap()
            .unwrap()
            .exists()
    );
}

#[test]
fn failed_protected_mutation_publish_recovers_only_preexisting_migrated_state() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut legacy = LeaseStore::new(&legacy_ctx);
    let LeaseAcquire::Acquired(lease) = legacy.acquire("workflow:ExampleProject", 60).unwrap()
    else {
        panic!("expected legacy lease acquisition");
    };
    git_init(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = JsonStatePersistence::new(&ctx, "leases");
    let protected_path = persistence.protected_path().unwrap().unwrap().to_path_buf();

    let error = persistence
        .with_locked::<_, LeaseFile>(|store| {
            store.leases.clear();
            fs::remove_file(&protected_path).unwrap();
            fs::create_dir(&protected_path).unwrap();
            Ok(())
        })
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Failed to replace loop cache file"),
        "{error}"
    );
    assert!(
        serde_json::from_slice::<LeaseFile>(&fs::read(persistence.legacy_path()).unwrap()).is_err(),
        "a completed legacy marker must block an older runtime"
    );
    fs::remove_dir(&protected_path).unwrap();
    let mut recovered = LeaseStore::new(&ctx);
    let LeaseAcquire::Held(current) = recovered.acquire("workflow:ExampleProject", 60).unwrap()
    else {
        panic!("the migration marker must recover the pre-cutover lease");
    };
    assert_eq!(current.owner, lease.owner);
    assert_eq!(recovered.active_leases().unwrap().len(), 1);
    assert!(protected_path.is_file());
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
fn lease_guard_refresh_refuses_cleanup_authority_after_reacquisition() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = LeaseStore::new(&ctx);
    let LeaseAcquire::Acquired(first) = store.acquire("branch:repair/example", 60).unwrap() else {
        panic!("expected first lease acquisition");
    };
    let mut guard = LeaseGuard::start(store.clone(), "branch:repair/example", &first, 60).unwrap();
    store
        .release("branch:repair/example", &first.owner)
        .unwrap();
    let LeaseAcquire::Acquired(replacement) = store.acquire("branch:repair/example", 60).unwrap()
    else {
        panic!("expected replacement lease acquisition");
    };

    let error = guard.refresh().unwrap_err().to_string();

    assert!(error.contains("owned by another worker"), "{error}");
    let LeaseAcquire::Held(current) = store.acquire("branch:repair/example", 60).unwrap() else {
        panic!("replacement lease must remain held");
    };
    assert_eq!(current.owner, replacement.owner);
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
        |_| {
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

fn git_init(root: &Path) {
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn example_workflow() -> ResolvedWorkflow {
    ResolvedWorkflow {
        id: "ExampleProject".into(),
        kind: NOOP_STATUS_KIND.into(),
        enabled: true,
        configured: true,
        lease_ttl_seconds: 60,
        max_attempts: 2,
        backoff_seconds: 1,
        codex_home_configured: None,
        schedule: None,
        codex_task: None,
    }
}
