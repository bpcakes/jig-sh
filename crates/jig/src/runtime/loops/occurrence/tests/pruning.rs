use tempfile::tempdir;

use super::super::{
    OCCURRENCE_HISTORY_PER_WORKFLOW, OccurrenceStatus, ScheduleFile, ScheduleOccurrence,
    occurrence_id, prune_history,
};

#[test]
fn pruning_preserves_discoverability_until_retained_worktree_is_removed() {
    let temp = tempdir().unwrap();
    let retained = temp.path().join("retained-worktree");
    std::fs::create_dir(&retained).unwrap();
    let mut store = ScheduleFile::default();
    for scheduled_at_ms in 0..=21 {
        let occurrence_id = occurrence_id("nightly", scheduled_at_ms);
        store.occurrences.insert(
            occurrence_id.clone(),
            ScheduleOccurrence {
                occurrence_id,
                workflow_id: "nightly".into(),
                scheduled_at_ms,
                owner: "owner".into(),
                claim_expires_at_ms: 0,
                started_at_ms: 0,
                finished_at_ms: Some(1),
                acknowledged_at_ms: None,
                status: OccurrenceStatus::Succeeded,
                worker_receipt_id: None,
                worktree: (scheduled_at_ms == 0).then(|| retained.display().to_string()),
                error: None,
            },
        );
    }

    prune_history(&mut store);

    assert_eq!(store.occurrences.len(), 21);
    assert!(store.occurrences.contains_key("nightly@0"));
    assert!(!store.occurrences.contains_key("nightly@1"));

    std::fs::remove_dir(&retained).unwrap();
    prune_history(&mut store);

    assert_eq!(store.occurrences.len(), OCCURRENCE_HISTORY_PER_WORKFLOW);
    assert!(!store.occurrences.contains_key("nightly@0"));
}

#[test]
fn pruning_never_discards_occurrences_that_need_attention() {
    let mut store = ScheduleFile::default();
    for scheduled_at_ms in 0..=OCCURRENCE_HISTORY_PER_WORKFLOW as u64 {
        let occurrence_id = occurrence_id("nightly", scheduled_at_ms);
        store.occurrences.insert(
            occurrence_id.clone(),
            ScheduleOccurrence {
                occurrence_id,
                workflow_id: "nightly".into(),
                scheduled_at_ms,
                owner: "owner".into(),
                claim_expires_at_ms: 0,
                started_at_ms: 0,
                finished_at_ms: Some(1),
                acknowledged_at_ms: None,
                status: if scheduled_at_ms == 0 {
                    OccurrenceStatus::NeedsAttention
                } else {
                    OccurrenceStatus::Succeeded
                },
                worker_receipt_id: None,
                worktree: None,
                error: None,
            },
        );
    }

    prune_history(&mut store);

    assert_eq!(store.occurrences.len(), OCCURRENCE_HISTORY_PER_WORKFLOW + 1);
    assert!(store.occurrences.contains_key("nightly@0"));
}
