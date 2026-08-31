use tempfile::{TempDir, tempdir};

use super::super::*;
use super::write_loop_fixture_repo;

#[test]
fn expired_claim_finishes_as_attention_and_preserves_worker_evidence() {
    let (_temp, mut store, claim) = claimed_occurrence();

    let finished = finish_with_evidence(&mut store, &claim, claim.claim_expires_at_ms).unwrap();

    assert_expired_evidence(&finished);
}

#[test]
fn original_owner_can_enrich_an_unacknowledged_stale_reconciliation() {
    let (_temp, mut store, claim) = claimed_occurrence();
    store.reconcile_stale_at(3_500).unwrap();

    let finished = finish_with_evidence(&mut store, &claim, 4_000).unwrap();

    assert_expired_evidence(&finished);
    assert_eq!(finished.finished_at_ms, Some(3_500));
}

#[test]
fn stale_reconciliation_rejects_evidence_from_another_owner() {
    let (_temp, mut store, claim) = claimed_occurrence();
    store.reconcile_stale_at(3_500).unwrap();

    let error = store
        .finish_at(
            &claim.occurrence_id,
            "another-owner",
            evidence_finish(),
            4_000,
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("owned by another dispatcher"), "{error}");
}

#[test]
fn acknowledged_stale_reconciliation_rejects_late_worker_evidence() {
    let (_temp, mut store, claim) = claimed_occurrence();
    store.reconcile_stale_at(3_500).unwrap();
    store.acknowledge_at(&claim.occurrence_id, 3_600).unwrap();

    let error = finish_with_evidence(&mut store, &claim, 4_000)
        .unwrap_err()
        .to_string();

    assert!(error.contains("already acknowledged"), "{error}");
}

fn claimed_occurrence() -> (TempDir, OccurrenceStore, ScheduleOccurrence) {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim_at("nightly", 100, 2, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    (temp, store, claim)
}

fn finish_with_evidence(
    store: &mut OccurrenceStore,
    claim: &ScheduleOccurrence,
    now: u64,
) -> Result<ScheduleOccurrence> {
    store.finish_at(&claim.occurrence_id, &claim.owner, evidence_finish(), now)
}

fn evidence_finish() -> OccurrenceFinish<'static> {
    OccurrenceFinish {
        outcome: OccurrenceOutcome::Succeeded,
        worker_receipt_id: Some("receipt-worker"),
        worktree: Some("/tmp/retained-worktree"),
        error: None,
    }
}

fn assert_expired_evidence(finished: &ScheduleOccurrence) {
    assert_eq!(finished.status, OccurrenceStatus::NeedsAttention);
    assert_eq!(
        finished.worker_receipt_id.as_deref(),
        Some("receipt-worker")
    );
    assert_eq!(finished.worktree.as_deref(), Some("/tmp/retained-worktree"));
    assert!(
        finished
            .error
            .as_deref()
            .is_some_and(|error| error.contains("claim expired")),
        "{finished:?}"
    );
}
