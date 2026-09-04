use tempfile::tempdir;

use super::super::{
    OCCURRENCE_HISTORY_PER_WORKFLOW, OccurrenceStatus, OccurrenceStore, ScheduleFile,
    ScheduleOccurrence, occurrence_id, prune_history, sorted_occurrences,
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
                uses_shared_checkout: Some(false),
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
                uses_shared_checkout: Some(false),
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

#[test]
fn pruning_keeps_the_newest_manual_occurrences() {
    let mut store = ScheduleFile::default();
    for started_at_ms in 1..=OCCURRENCE_HISTORY_PER_WORKFLOW as u64 + 1 {
        let occurrence_id = format!("nightly@manual-{started_at_ms:03}");
        store.occurrences.insert(
            occurrence_id.clone(),
            ScheduleOccurrence {
                occurrence_id,
                workflow_id: "nightly".into(),
                scheduled_at_ms: 0,
                owner: "owner".into(),
                claim_expires_at_ms: 0,
                started_at_ms,
                uses_shared_checkout: Some(false),
                finished_at_ms: Some(started_at_ms),
                acknowledged_at_ms: None,
                status: OccurrenceStatus::Failed,
                worker_receipt_id: None,
                worktree: None,
                error: None,
            },
        );
    }

    prune_history(&mut store);

    assert_eq!(store.occurrences.len(), OCCURRENCE_HISTORY_PER_WORKFLOW);
    assert!(!store.occurrences.contains_key("nightly@manual-001"));
    assert!(store.occurrences.contains_key("nightly@manual-021"));
}

#[test]
fn pruning_reserves_the_latest_scheduled_occurrence_as_the_dispatch_watermark() {
    let mut store = ScheduleFile::default();
    let scheduled_id = occurrence_id("nightly", 100);
    store.occurrences.insert(
        scheduled_id.clone(),
        ScheduleOccurrence {
            occurrence_id: scheduled_id.clone(),
            workflow_id: "nightly".into(),
            scheduled_at_ms: 100,
            owner: "owner".into(),
            claim_expires_at_ms: 0,
            started_at_ms: 1,
            uses_shared_checkout: Some(false),
            finished_at_ms: Some(1),
            acknowledged_at_ms: None,
            status: OccurrenceStatus::Succeeded,
            worker_receipt_id: None,
            worktree: None,
            error: None,
        },
    );
    for started_at_ms in 2..=OCCURRENCE_HISTORY_PER_WORKFLOW as u64 + 2 {
        let occurrence_id = format!("nightly@manual-{started_at_ms:03}");
        store.occurrences.insert(
            occurrence_id.clone(),
            ScheduleOccurrence {
                occurrence_id,
                workflow_id: "nightly".into(),
                scheduled_at_ms: 0,
                owner: "owner".into(),
                claim_expires_at_ms: 0,
                started_at_ms,
                uses_shared_checkout: Some(false),
                finished_at_ms: Some(started_at_ms),
                acknowledged_at_ms: None,
                status: OccurrenceStatus::Failed,
                worker_receipt_id: None,
                worktree: None,
                error: None,
            },
        );
    }

    prune_history(&mut store);

    assert_eq!(store.occurrences.len(), OCCURRENCE_HISTORY_PER_WORKFLOW);
    assert!(store.occurrences.contains_key(&scheduled_id));
    assert!(!store.occurrences.contains_key("nightly@manual-002"));
    let latest = OccurrenceStore::latest_for_workflow(&sorted_occurrences(&store), "nightly")
        .expect("scheduled watermark must remain visible to dispatch");
    assert_eq!(latest.scheduled_at_ms, 100);
}

#[cfg(unix)]
#[test]
fn retained_worktree_inspection_errors_fail_closed() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let retained = temp.path().join("retained-worktree");
    symlink("retained-worktree", &retained).unwrap();
    assert!(retained.try_exists().is_err());
    let occurrence = ScheduleOccurrence {
        occurrence_id: "nightly@0".into(),
        workflow_id: "nightly".into(),
        scheduled_at_ms: 0,
        owner: "owner".into(),
        claim_expires_at_ms: 0,
        started_at_ms: 0,
        uses_shared_checkout: Some(false),
        finished_at_ms: Some(1),
        acknowledged_at_ms: None,
        status: OccurrenceStatus::Succeeded,
        worker_receipt_id: None,
        worktree: Some(retained.display().to_string()),
        error: None,
    };

    assert!(occurrence.has_retained_worktree());
}
