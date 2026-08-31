use tempfile::tempdir;

use super::super::*;

#[test]
fn scheduled_claim_cannot_run_older_work_after_a_newer_dispatch_records_it() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(newer) = store.claim("nightly", 200, 60).unwrap() else {
        panic!("expected newer occurrence claim");
    };
    store
        .finish(
            &newer.occurrence_id,
            &newer.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::Succeeded,
                worker_receipt_id: None,
                worktree: None,
                error: None,
            },
        )
        .unwrap();

    let OccurrenceClaim::AlreadyRecorded(record) = store
        .claim_scheduled(
            "nightly",
            100,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("the older occurrence must be superseded atomically");
    };

    assert_eq!(record.occurrence_id, newer.occurrence_id);
    assert_eq!(store.snapshot().unwrap().len(), 1);
}

#[test]
fn unresolved_attention_blocks_new_scheduled_claims_until_acknowledged() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(first) = store.claim("nightly", 100, 60).unwrap() else {
        panic!("expected first occurrence claim");
    };
    store
        .finish(
            &first.occurrence_id,
            &first.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::NeedsAttention,
                worker_receipt_id: Some("receipt-example"),
                worktree: None,
                error: Some("ambiguous worker result"),
            },
        )
        .unwrap();

    let OccurrenceClaim::BlockedByAttention(blocker) = store
        .claim_scheduled(
            "nightly",
            200,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("unresolved attention must block another occurrence");
    };
    assert_eq!(blocker.occurrence_id, first.occurrence_id);
    assert_eq!(store.snapshot().unwrap().len(), 1);

    store.acknowledge(&first.occurrence_id).unwrap();
    let OccurrenceClaim::Acquired(second) = store
        .claim_scheduled(
            "nightly",
            200,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("acknowledgement must unblock the workflow");
    };
    assert_eq!(second.scheduled_at_ms, 200);
}

#[test]
fn shared_repository_attention_blocks_all_workflows() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(first) = store
        .claim_scheduled(
            "repo-task-a",
            100,
            60,
            OccurrenceAttentionScope::SharedRepository,
            false,
        )
        .unwrap()
    else {
        panic!("expected first shared-repository occurrence claim");
    };
    store
        .finish(
            &first.occurrence_id,
            &first.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::NeedsAttention,
                worker_receipt_id: Some("receipt-example"),
                worktree: None,
                error: Some("ambiguous shared checkout"),
            },
        )
        .unwrap();

    let OccurrenceClaim::BlockedByAttention(blocker) = store
        .claim_scheduled(
            "repo-task-b",
            100,
            60,
            OccurrenceAttentionScope::SharedRepository,
            false,
        )
        .unwrap()
    else {
        panic!("shared-repository attention must block every shared workflow");
    };
    assert_eq!(blocker.occurrence_id, first.occurrence_id);

    let OccurrenceClaim::BlockedByAttention(isolated_blocker) = store
        .claim_scheduled(
            "isolated-task",
            100,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("shared-repository attention must block an isolated workflow");
    };
    assert_eq!(isolated_blocker.occurrence_id, first.occurrence_id);
}

#[test]
fn live_shared_repository_occurrence_blocks_all_workflows() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(first) = store
        .claim_scheduled(
            "repo-task-a",
            100,
            60,
            OccurrenceAttentionScope::SharedRepository,
            false,
        )
        .unwrap()
    else {
        panic!("expected first shared-repository occurrence claim");
    };

    let OccurrenceClaim::BlockedByRunning(blocker) = store
        .claim_scheduled(
            "repo-task-b",
            100,
            60,
            OccurrenceAttentionScope::SharedRepository,
            false,
        )
        .unwrap()
    else {
        panic!("live shared-repository work must retain admission authority");
    };
    assert_eq!(blocker.occurrence_id, first.occurrence_id);

    let OccurrenceClaim::BlockedByRunning(isolated_blocker) = store
        .claim_scheduled(
            "isolated-task",
            100,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("live shared-repository work must block isolated work");
    };
    assert_eq!(isolated_blocker.occurrence_id, first.occurrence_id);
}

#[test]
fn shared_repository_claim_waits_for_all_managed_worktree_authority() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let retained = temp.path().join("retained-isolated-worktree");
    std::fs::create_dir(&retained).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(isolated) = store
        .claim_scheduled(
            "isolated-task",
            100,
            60,
            OccurrenceAttentionScope::Workflow,
            true,
        )
        .unwrap()
    else {
        panic!("expected isolated occurrence claim");
    };

    let claim_shared = |store: &mut OccurrenceStore| {
        store
            .claim_scheduled(
                "repo-task",
                200,
                60,
                OccurrenceAttentionScope::SharedRepository,
                false,
            )
            .unwrap()
    };
    let OccurrenceClaim::BlockedByRunning(blocker) = claim_shared(&mut store) else {
        panic!("an isolated worker must exclude a shared-root worker");
    };
    assert_eq!(blocker.occurrence_id, isolated.occurrence_id);

    store
        .finish(
            &isolated.occurrence_id,
            &isolated.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::NeedsAttention,
                worker_receipt_id: None,
                worktree: Some(retained.to_string_lossy().as_ref()),
                error: Some("retained isolated evidence"),
            },
        )
        .unwrap();
    let OccurrenceClaim::BlockedByAttention(blocker) = claim_shared(&mut store) else {
        panic!("isolated attention must exclude a shared-root worker");
    };
    assert_eq!(blocker.occurrence_id, isolated.occurrence_id);

    store.acknowledge(&isolated.occurrence_id).unwrap();
    let OccurrenceClaim::BlockedByRetainedWorktree(blocker) = claim_shared(&mut store) else {
        panic!("acknowledged retained evidence must exclude a shared-root worker");
    };
    assert_eq!(blocker.occurrence_id, isolated.occurrence_id);

    std::fs::remove_dir(&retained).unwrap();
    let OccurrenceClaim::Acquired(shared) = claim_shared(&mut store) else {
        panic!("removing retained evidence must release shared-root admission");
    };
    assert!(shared.uses_shared_checkout.unwrap());
}

#[test]
fn pre_creation_worktree_reservation_survives_a_crashed_occurrence() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let retained = temp.path().join("reserved-before-git-add");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(isolated) = store
        .claim_scheduled(
            "isolated-task",
            100,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("expected isolated occurrence claim");
    };
    let reservation = OccurrenceWorktreeReservation {
        store: store.clone(),
        occurrence_id: isolated.occurrence_id.clone(),
        owner: isolated.owner.clone(),
    };

    reservation.reserve(&retained).unwrap();
    std::fs::create_dir(&retained).unwrap();
    let running = store
        .snapshot()
        .unwrap()
        .into_iter()
        .find(|record| record.occurrence_id == isolated.occurrence_id)
        .unwrap();
    assert_eq!(
        running.worktree.as_deref(),
        Some(retained.to_string_lossy().as_ref())
    );

    let expired_at = isolated.claim_expires_at_ms;
    store.reconcile_stale_for_test(expired_at).unwrap();
    store
        .acknowledge_at(&isolated.occurrence_id, expired_at)
        .unwrap();
    let claim = store
        .claim_id_with_constraints_at(
            "shared-task@manual:test".into(),
            "shared-task",
            0,
            60,
            expired_at,
            super::super::claim::OccurrenceClaimConstraints {
                attention_scope: OccurrenceAttentionScope::SharedRepository,
                block_newer_occurrences: false,
                block_retained_worktree: false,
            },
        )
        .unwrap();
    let OccurrenceClaim::BlockedByRetainedWorktree(blocker) = claim else {
        panic!("a crash-created worktree must keep shared-root execution blocked");
    };
    assert_eq!(blocker.occurrence_id, isolated.occurrence_id);
}

#[test]
fn live_workflow_occurrence_blocks_a_newer_claim_for_the_same_workflow() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(first) = store
        .claim_scheduled(
            "nightly",
            100,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("expected first workflow occurrence claim");
    };

    let OccurrenceClaim::BlockedByRunning(blocker) = store
        .claim_scheduled(
            "nightly",
            200,
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("live workflow work must block a newer claim");
    };
    assert_eq!(blocker.occurrence_id, first.occurrence_id);
}

#[test]
fn retained_worktree_constraint_does_not_depend_on_attention_constraint() {
    let temp = tempdir().unwrap();
    super::write_loop_fixture_repo(temp.path());
    let retained = temp.path().join("retained-worktree");
    std::fs::create_dir(&retained).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(first) = store.claim("nightly", 100, 60).unwrap() else {
        panic!("expected first occurrence claim");
    };
    store
        .finish(
            &first.occurrence_id,
            &first.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::NeedsAttention,
                worker_receipt_id: None,
                worktree: Some(retained.to_string_lossy().as_ref()),
                error: None,
            },
        )
        .unwrap();
    store.acknowledge(&first.occurrence_id).unwrap();

    let OccurrenceClaim::BlockedByRetainedWorktree(blocker) = store
        .claim_id_with_constraints_at(
            "nightly@manual-test".into(),
            "nightly",
            0,
            60,
            200,
            super::super::claim::OccurrenceClaimConstraints {
                attention_scope: OccurrenceAttentionScope::None,
                block_newer_occurrences: false,
                block_retained_worktree: true,
            },
        )
        .unwrap()
    else {
        panic!("retained-worktree blocking must be an independent claim constraint");
    };
    assert_eq!(blocker.occurrence_id, first.occurrence_id);
}
