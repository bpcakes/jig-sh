use std::sync::mpsc::RecvTimeoutError;
use tempfile::tempdir;

use super::*;
use crate::runtime::loops::state::{LOOP_CACHE_DIR, LOOP_RUNTIME_DIR, with_exclusive_file_lock};

#[path = "tests/manual_evidence.rs"]
mod manual_evidence;
#[path = "tests/pruning.rs"]
mod pruning;
#[path = "tests/renewal_diagnostics.rs"]
mod renewal_diagnostics;
#[path = "tests/review_constraints.rs"]
mod review_constraints;
#[path = "tests/schema_migration.rs"]
mod schema_migration;
#[path = "tests/stale_enrichment.rs"]
mod stale_enrichment;
#[test]
fn occurrence_claim_is_single_use_and_owner_checked() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);

    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 60, 1_000).unwrap()
    else {
        panic!("expected occurrence claim");
    };
    let OccurrenceClaim::AlreadyRecorded(existing) =
        store.claim_at("nightly", 100, 60, 1_001).unwrap()
    else {
        panic!("expected duplicate occurrence");
    };
    assert_eq!(existing.owner, claim.owner);
    assert!(
        store
            .finish(
                &claim.occurrence_id,
                "another-owner",
                OccurrenceFinish {
                    outcome: OccurrenceOutcome::Succeeded,
                    worker_receipt_id: None,
                    worktree: None,
                    error: None,
                },
            )
            .unwrap_err()
            .to_string()
            .contains("owned by another dispatcher")
    );
}

#[test]
fn occurrence_renewal_and_stale_reconciliation_are_terminal() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };

    let renewed = store
        .renew_at(&claim.occurrence_id, &claim.owner, 2, 2_000)
        .unwrap();
    assert_eq!(renewed.claim_expires_at_ms, 4_000);
    assert!(store.reconcile_stale_at(3_999).unwrap().is_empty());
    let reconciled = store.reconcile_stale_at(4_000).unwrap();
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].status, OccurrenceStatus::NeedsAttention);

    let OccurrenceClaim::AlreadyRecorded(existing) =
        store.claim_at("nightly", 100, 2, 5_000).unwrap()
    else {
        panic!("stale occurrence must not be reclaimed");
    };
    assert_eq!(existing.status, OccurrenceStatus::NeedsAttention);
}

#[test]
fn renewal_error_does_not_skip_terminal_worker_evidence() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim("nightly", 100, 60).unwrap() else {
        panic!("expected occurrence claim");
    };
    let guard = guard_with_failed_renewal(store.clone(), &claim);

    let finalization = guard
        .finish(OccurrenceFinish {
            outcome: OccurrenceOutcome::Succeeded,
            worker_receipt_id: Some("receipt-worker"),
            worktree: Some("/tmp/ExampleProject-retained-worktree"),
            error: None,
        })
        .unwrap();

    assert_eq!(finalization.occurrence.status, OccurrenceStatus::Succeeded);
    assert_eq!(
        finalization.occurrence.worker_receipt_id.as_deref(),
        Some("receipt-worker")
    );
    assert_eq!(
        finalization.occurrence.worktree.as_deref(),
        Some("/tmp/ExampleProject-retained-worktree")
    );
    assert!(
        finalization
            .renewal_error
            .as_deref()
            .is_some_and(|error| error.contains("injected renewal failure"))
    );
    let persisted = store.snapshot().unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0], finalization.occurrence);
}

#[test]
fn renewal_error_does_not_turn_a_persisted_abandonment_into_ambiguous_attention() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim("nightly", 100, 60).unwrap() else {
        panic!("expected occurrence claim");
    };
    let guard = guard_with_failed_renewal(store.clone(), &claim);

    let finalization = guard.abandon().unwrap();

    assert_eq!(finalization.occurrence.status, OccurrenceStatus::Running);
    assert!(finalization.renewal_error.is_some());
    assert!(store.snapshot().unwrap().is_empty());
}

#[test]
fn expired_claim_cannot_be_abandoned_and_reopened() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };

    let abandoned = store
        .abandon_at(
            &claim.occurrence_id,
            &claim.owner,
            claim.claim_expires_at_ms,
        )
        .unwrap();

    assert_eq!(abandoned.status, OccurrenceStatus::NeedsAttention);
    let OccurrenceClaim::AlreadyRecorded(existing) =
        store.claim_at("nightly", 100, 2, 4_000).unwrap()
    else {
        panic!("expired occurrence must remain recorded");
    };
    assert_eq!(existing.status, OccurrenceStatus::NeedsAttention);
}

#[test]
fn expired_unexecuted_claim_can_be_abandoned_and_reopened() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };

    let abandoned = store
        .abandon_unexecuted(&claim.occurrence_id, &claim.owner)
        .unwrap();

    assert_eq!(abandoned.status, OccurrenceStatus::Running);
    let OccurrenceClaim::Acquired(retried) = store.claim_at("nightly", 100, 2, 4_000).unwrap()
    else {
        panic!("an unexecuted occurrence must be claimable again");
    };
    assert_ne!(retried.owner, claim.owner);
}

#[test]
fn reconciled_unexecuted_claim_can_be_abandoned_and_reopened() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    let reconciled = store.reconcile_stale_at(3_000).unwrap();
    assert_eq!(reconciled.len(), 1);

    let abandoned = store
        .abandon_unexecuted(&claim.occurrence_id, &claim.owner)
        .unwrap();

    assert_eq!(abandoned.status, OccurrenceStatus::NeedsAttention);
    assert_eq!(abandoned.finished_at_ms, Some(3_000));
    assert_eq!(abandoned.error.as_deref(), Some(STALE_RECONCILIATION_ERROR));
    let OccurrenceClaim::Acquired(retried) = store.claim_at("nightly", 100, 2, 4_000).unwrap()
    else {
        panic!("a reconciled but unexecuted occurrence must be claimable again");
    };
    assert_ne!(retried.owner, claim.owner);
}

#[test]
fn unexecuted_abandonment_refuses_ambiguous_worker_evidence() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    let finished = store
        .finish_at(
            &claim.occurrence_id,
            &claim.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::Succeeded,
                worker_receipt_id: Some("receipt-worker"),
                worktree: None,
                error: None,
            },
            claim.claim_expires_at_ms,
        )
        .unwrap();
    assert_eq!(finished.status, OccurrenceStatus::NeedsAttention);

    let error = store
        .abandon_unexecuted(&claim.occurrence_id, &claim.owner)
        .unwrap_err()
        .to_string();

    assert!(error.contains("already needs_attention"), "{error}");
    assert_eq!(store.snapshot().unwrap(), vec![finished]);
}

#[test]
fn finish_samples_expiry_after_acquiring_the_schedule_lock() {
    assert_expiry_clock_runs_under_lock(DelayedTransition::Finish);
}

#[test]
fn abandon_samples_expiry_after_acquiring_the_schedule_lock() {
    assert_expiry_clock_runs_under_lock(DelayedTransition::Abandon);
}

#[test]
fn acknowledging_attention_is_idempotent_and_preserves_the_claim() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    store.reconcile_stale_at(3_000).unwrap();

    let OccurrenceAcknowledgement::Acknowledged(acknowledged) =
        store.acknowledge(&claim.occurrence_id).unwrap()
    else {
        panic!("expected first acknowledgement to change state");
    };
    assert_eq!(acknowledged.status, OccurrenceStatus::Acknowledged);
    assert!(acknowledged.acknowledged_at_ms.is_some());

    let OccurrenceAcknowledgement::AlreadyAcknowledged(existing) =
        store.acknowledge(&claim.occurrence_id).unwrap()
    else {
        panic!("expected repeated acknowledgement to be idempotent");
    };
    assert_eq!(existing.acknowledged_at_ms, acknowledged.acknowledged_at_ms);

    let OccurrenceClaim::AlreadyRecorded(existing) =
        store.claim_at("nightly", 100, 2, 4_000).unwrap()
    else {
        panic!("acknowledgement must not make the occurrence runnable again");
    };
    assert_eq!(existing.status, OccurrenceStatus::Acknowledged);
}

#[test]
fn acknowledging_an_expired_running_claim_reconciles_it_under_the_same_lock() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };

    let OccurrenceAcknowledgement::Acknowledged(acknowledged) =
        store.acknowledge_at(&claim.occurrence_id, 3_000).unwrap()
    else {
        panic!("expected expired occurrence to be acknowledged");
    };

    assert_eq!(acknowledged.status, OccurrenceStatus::Acknowledged);
    assert_eq!(acknowledged.finished_at_ms, Some(3_000));
    assert_eq!(acknowledged.acknowledged_at_ms, Some(3_000));
    assert_eq!(
        acknowledged.error.as_deref(),
        Some(STALE_RECONCILIATION_ERROR)
    );
}

#[test]
fn read_only_snapshot_can_inspect_legacy_state_without_migrating_it() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": LEGACY_SCHEDULE_SCHEMA_VERSION,
            "occurrences": {}
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let snapshot = OccurrenceStore::new(&ctx)
        .snapshot_read_only_with_cancellation(&|| false)
        .unwrap();

    assert!(snapshot.is_empty());
    assert!(!temp.path().join(LOOP_RUNTIME_DIR).exists());
}

#[test]
fn locked_snapshot_reclaims_orphaned_schedule_temp_files() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let legacy_temp = legacy_dir.join("schedule.tmp-orphan");
    let runtime_temp = runtime_dir.join("schedule.tmp-orphan");
    std::fs::write(&legacy_temp, b"partial").unwrap();
    std::fs::write(&runtime_temp, b"partial").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert!(OccurrenceStore::new(&ctx).snapshot().unwrap().is_empty());

    assert!(!legacy_temp.exists());
    assert!(!runtime_temp.exists());
}

#[cfg(unix)]
#[test]
fn locked_snapshot_does_not_replace_an_unchanged_durable_ledger() {
    use std::os::unix::fs::MetadataExt;

    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(_) = store.claim_at("nightly", 100, 60, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    let path = temp.path().join(LOOP_RUNTIME_DIR).join("schedule.json");
    let inode_before = std::fs::metadata(&path).unwrap().ino();

    assert_eq!(store.snapshot().unwrap().len(), 1);

    assert_eq!(std::fs::metadata(path).unwrap().ino(), inode_before);
}

#[test]
fn migration_marker_without_durable_state_fails_closed_for_reads_and_writes() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    write_legacy_marker(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let read_error = OccurrenceStore::new(&ctx)
        .snapshot_read_only_with_cancellation(&|| false)
        .unwrap_err()
        .to_string();
    let write_error = OccurrenceStore::new(&ctx)
        .snapshot()
        .unwrap_err()
        .to_string();

    assert!(
        read_error.contains("migration marker exists without durable state"),
        "{read_error}"
    );
    assert!(
        write_error.contains("migration marker exists without durable state"),
        "{write_error}"
    );
    assert!(
        !temp
            .path()
            .join(LOOP_RUNTIME_DIR)
            .join("schedule.json")
            .exists()
    );
}

#[test]
fn divergent_legacy_and_durable_ledgers_require_manual_reconciliation() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(
        legacy_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": SCHEDULE_SCHEMA_VERSION,
            "occurrences": {}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": SCHEDULE_SCHEMA_VERSION,
            "occurrences": {
                "nightly@100": {
                    "occurrence_id": "nightly@100",
                    "workflow_id": "nightly",
                    "scheduled_at_ms": 100,
                    "owner": "owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "finished_at_ms": 150,
                    "status": "succeeded"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = OccurrenceStore::new(&ctx)
        .snapshot()
        .unwrap_err()
        .to_string();
    let read_error = OccurrenceStore::new(&ctx)
        .snapshot_read_only_with_cancellation(&|| false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("exists at both"), "{error}");
    assert!(error.contains("reconcile the files"), "{error}");
    assert!(read_error.contains("exists at both"), "{read_error}");
    assert!(read_error.contains("reconcile the files"), "{read_error}");
}

#[test]
fn occurrence_guard_renews_the_persisted_claim_before_expiry() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim("nightly", 100, 60).unwrap() else {
        panic!("expected occurrence claim");
    };
    let guard =
        OccurrenceGuard::start_for_test(store.clone(), &claim, 60, Duration::from_millis(1))
            .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let renewed = store.snapshot().unwrap().into_iter().next().unwrap();
        if renewed.claim_expires_at_ms > claim.claim_expires_at_ms {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "occurrence renewal was not persisted before the test deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let finished = guard
        .finish(OccurrenceFinish {
            outcome: OccurrenceOutcome::Succeeded,
            worker_receipt_id: None,
            worktree: None,
            error: None,
        })
        .unwrap();
    assert_eq!(finished.occurrence.status, OccurrenceStatus::Succeeded);
    assert!(finished.renewal_error.is_none());
}

#[test]
fn occurrence_renewal_retries_transient_failure_before_claim_expiry() {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::AtomicUsize;

    let failed = AtomicBool::new(false);
    let calls = AtomicUsize::new(0);
    let now_ms = Cell::new(0_u64);
    let waits = RefCell::new(Vec::new());

    run_occurrence_renewal_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        |_| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(RenewalAttemptError::Retryable(anyhow::anyhow!(
                    "injected transient renewal failure"
                )));
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

#[test]
fn occurrence_renewal_latches_failure_at_claim_expiry() {
    let (_stop, receiver) = mpsc::channel();
    let failed = AtomicBool::new(false);

    let error = run_occurrence_renewal(
        &receiver,
        Duration::from_millis(1),
        100,
        &failed,
        |_| {
            Err(RenewalAttemptError::Retryable(anyhow::anyhow!(
                "persistent renewal failure"
            )))
        },
        || 100,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("persistent renewal failure"), "{error}");
    assert!(failed.load(Ordering::Acquire));
}

#[test]
fn occurrence_renewal_latches_failure_with_time_to_cancel_and_finish() {
    let (_stop, receiver) = mpsc::channel();
    let failed = AtomicBool::new(false);

    let error = run_occurrence_renewal(
        &receiver,
        Duration::from_millis(100),
        1_000,
        &failed,
        |_| {
            Err(RenewalAttemptError::Retryable(anyhow::anyhow!(
                "persistent renewal failure"
            )))
        },
        || 950,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("persistent renewal failure"), "{error}");
    assert!(failed.load(Ordering::Acquire));
}

#[test]
fn occurrence_renewal_stop_latches_an_already_expired_claim() {
    let failed = AtomicBool::new(false);

    run_occurrence_renewal_with_wait(
        Duration::from_millis(300),
        900,
        &failed,
        |_| panic!("an immediate stop must not renew"),
        || 900,
        |_| Ok(()),
    )
    .unwrap();

    assert!(failed.load(Ordering::Acquire));
}

#[test]
fn durable_migration_marker_fails_closed_for_reads_and_writes() {
    for operation in ["read", "write"] {
        let temp = tempdir().unwrap();
        write_loop_fixture_repo(temp.path());
        write_durable_marker(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut store = OccurrenceStore::new(&ctx);

        let error = if operation == "read" {
            store.snapshot().unwrap_err()
        } else {
            match store.claim_at("nightly", 100, 60, 1_000) {
                Ok(_) => panic!("durable migration marker must block writes"),
                Err(error) => error,
            }
        }
        .to_string();

        assert!(
            error.contains("durable loop schedule"),
            "{operation}: {error}"
        );
        assert!(error.contains("migration marker"), "{operation}: {error}");
    }
}

fn write_loop_fixture_repo(root: &std::path::Path) {
    crate::test_env::TestRepoBuilder::new(root)
        .required_commands(Vec::<String>::new())
        .write();
}

fn guard_with_failed_renewal(
    store: OccurrenceStore,
    claim: &ScheduleOccurrence,
) -> OccurrenceGuard {
    OccurrenceGuard {
        store,
        occurrence_id: claim.occurrence_id.clone(),
        owner: claim.owner.clone(),
        stop: None,
        renewal: Some(std::thread::spawn(|| {
            anyhow::bail!("injected renewal failure")
        })),
        renewal_failed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
    }
}

fn write_legacy_marker(root: &std::path::Path) {
    let legacy_dir = root.join(LOOP_CACHE_DIR);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": SCHEDULE_SCHEMA_VERSION,
            "migrated_to": ".agent/runtime/loop/schedule.json",
            "occurrences": {}
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_durable_marker(root: &std::path::Path) {
    let runtime_dir = root.join(LOOP_RUNTIME_DIR);
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": SCHEDULE_SCHEMA_VERSION,
            "migrated_to": ".agent/runtime/loop/schedule.json",
            "occurrences": {}
        }))
        .unwrap(),
    )
    .unwrap();
}

#[derive(Clone, Copy)]
enum DelayedTransition {
    Finish,
    Abandon,
}

fn assert_expiry_clock_runs_under_lock(transition: DelayedTransition) {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 1, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };

    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    let lock_path = runtime_dir.join("schedule.lock");
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = thread::spawn(move || {
        with_exclusive_file_lock(&runtime_dir, &lock_path, || {
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
    });
    locked_rx.recv().unwrap();

    let (clock_tx, clock_rx) = mpsc::channel();
    let occurrence_id = claim.occurrence_id.clone();
    let owner = claim.owner.clone();
    let expires_at_ms = claim.claim_expires_at_ms;
    let finisher = thread::spawn(move || match transition {
        DelayedTransition::Finish => store.finish_with_clock(
            &occurrence_id,
            &owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::Succeeded,
                worker_receipt_id: Some("receipt-worker"),
                worktree: None,
                error: None,
            },
            || {
                clock_tx.send(()).unwrap();
                expires_at_ms
            },
        ),
        DelayedTransition::Abandon => store.abandon_with_clock(&occurrence_id, &owner, || {
            clock_tx.send(()).unwrap();
            expires_at_ms
        }),
    });

    assert!(
        clock_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "the expiry clock ran before the schedule lock was acquired"
    );
    release_tx.send(()).unwrap();
    clock_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let record = finisher.join().unwrap().unwrap();
    holder.join().unwrap();

    assert_eq!(record.status, OccurrenceStatus::NeedsAttention);
}
