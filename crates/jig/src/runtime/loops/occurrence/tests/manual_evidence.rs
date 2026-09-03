use std::time::Duration;

use tempfile::{TempDir, tempdir};

use super::super::*;
use super::write_loop_fixture_repo;

#[test]
fn manual_occurrence_namespace_cannot_collide_with_a_numeric_schedule_instant() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);

    let OccurrenceClaim::Acquired(claim) = store
        .claim_manual(
            "nightly",
            "100",
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("expected manual occurrence claim");
    };

    assert_eq!(claim.occurrence_id, "nightly@manual:100");
    assert_ne!(claim.occurrence_id, "nightly@100");
}

#[test]
fn stale_reconciliation_preserves_staged_manual_diagnostics() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store.claim("manual", 100, 60).unwrap() else {
        panic!("expected occurrence claim");
    };
    store
        .stage_manual(
            &claim.occurrence_id,
            &claim.owner,
            OccurrenceFinish {
                outcome: OccurrenceOutcome::Failed,
                worker_receipt_id: None,
                worktree: None,
                error: Some("tick receipt publication failed"),
            },
        )
        .unwrap();

    let reconciled = store.reconcile_stale_at(claim.claim_expires_at_ms).unwrap();

    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].status, OccurrenceStatus::NeedsAttention);
    let error = reconciled[0].error.as_deref().unwrap();
    assert!(error.contains(STALE_RECONCILIATION_ERROR), "{error}");
    assert!(error.contains("tick receipt publication failed"), "{error}");
    assert!(
        store
            .abandon_unexecuted(&claim.occurrence_id, &claim.owner)
            .unwrap_err()
            .to_string()
            .contains("already needs_attention")
    );
    assert_eq!(store.snapshot().unwrap(), reconciled);
}

#[test]
fn original_owner_can_stage_manual_evidence_after_stale_reconciliation() {
    let (_temp, mut store, _claim, mut guard) = claimed_manual_occurrence();
    store.reconcile_stale_at(u64::MAX).unwrap();

    let staged = guard
        .stage_manual(OccurrenceFinish {
            outcome: OccurrenceOutcome::Failed,
            worker_receipt_id: Some("receipt-worker"),
            worktree: None,
            error: Some("worker failed"),
        })
        .unwrap();

    assert_eq!(staged.status, OccurrenceStatus::NeedsAttention);
    assert_eq!(staged.finished_at_ms, Some(u64::MAX));
    assert_eq!(staged.worker_receipt_id.as_deref(), Some("receipt-worker"));
    assert!(
        staged
            .error
            .as_deref()
            .is_some_and(|error| error.contains("claim expired")),
        "{staged:?}"
    );
    assert_eq!(
        store.snapshot().unwrap()[0].worker_receipt_id.as_deref(),
        Some("receipt-worker")
    );
}

#[test]
fn original_owner_can_finish_manual_occurrence_after_stale_reconciliation() {
    let (_temp, mut store, _claim, guard) = claimed_manual_occurrence();
    store.reconcile_stale_at(u64::MAX).unwrap();

    let finalization = guard
        .finish_manual(
            OccurrenceFinish {
                outcome: OccurrenceOutcome::Succeeded,
                worker_receipt_id: Some("receipt-worker"),
                worktree: None,
                error: None,
            },
            false,
        )
        .unwrap();

    assert_eq!(
        finalization.occurrence.status,
        OccurrenceStatus::NeedsAttention
    );
    assert_eq!(finalization.occurrence.finished_at_ms, Some(u64::MAX));
    assert_eq!(
        finalization.occurrence.worker_receipt_id.as_deref(),
        Some("receipt-worker")
    );
    assert_eq!(store.snapshot().unwrap(), vec![finalization.occurrence]);
}

fn claimed_manual_occurrence() -> (
    TempDir,
    OccurrenceStore,
    ScheduleOccurrence,
    OccurrenceGuard,
) {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(claim) = store
        .claim_manual(
            "manual",
            "item-1",
            60,
            OccurrenceAttentionScope::Workflow,
            false,
        )
        .unwrap()
    else {
        panic!("expected manual occurrence claim");
    };
    let guard =
        OccurrenceGuard::start_for_test(store.clone(), &claim, 60, Duration::from_secs(3_600))
            .unwrap();
    (temp, store, claim, guard)
}
