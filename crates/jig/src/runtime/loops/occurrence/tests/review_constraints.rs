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

    let OccurrenceClaim::AlreadyRecorded(record) =
        store.claim_scheduled("nightly", 100, 60, false).unwrap()
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

    let OccurrenceClaim::BlockedByAttention(blocker) =
        store.claim_scheduled("nightly", 200, 60, false).unwrap()
    else {
        panic!("unresolved attention must block another occurrence");
    };
    assert_eq!(blocker.occurrence_id, first.occurrence_id);
    assert_eq!(store.snapshot().unwrap().len(), 1);

    store.acknowledge(&first.occurrence_id).unwrap();
    let OccurrenceClaim::Acquired(second) =
        store.claim_scheduled("nightly", 200, 60, false).unwrap()
    else {
        panic!("acknowledgement must unblock the workflow");
    };
    assert_eq!(second.scheduled_at_ms, 200);
}
