use tempfile::tempdir;

use super::super::*;
use super::write_loop_fixture_repo;

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
