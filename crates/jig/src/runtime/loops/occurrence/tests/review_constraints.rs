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
fn shared_repository_attention_blocks_other_shared_repository_workflows() {
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
        panic!("shared-repository attention must not block an isolated workflow");
    };
    assert_eq!(isolated.uses_shared_checkout, Some(false));
}

#[test]
fn live_shared_repository_occurrence_blocks_only_overlapping_scope() {
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
        panic!("unrelated isolated work must remain admissible");
    };
    assert_eq!(isolated.uses_shared_checkout, Some(false));
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
                outcome: OccurrenceOutcome::Succeeded,
                worker_receipt_id: None,
                worktree: Some(retained.to_string_lossy().as_ref()),
                error: None,
            },
        )
        .unwrap();

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
