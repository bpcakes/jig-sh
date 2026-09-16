use super::*;

#[test]
fn acknowledgement_does_not_hide_an_unresolved_branch_attempt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let reverse_path = temp.path().join("tracker-operations-reverse.jsonl");
    let intent = event(
        "tracker-event-intent",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    let left_attempt = event(
        "tracker-event-left-attempt",
        "tracker-operation-1",
        TrackerOperationPhase::Attempt,
    );
    let left_observation = event(
        "tracker-event-left-observation",
        "tracker-operation-1",
        TrackerOperationPhase::Observation,
    );
    let mut left_acknowledgement = event(
        "tracker-event-left-ack",
        "tracker-operation-1",
        TrackerOperationPhase::Acknowledgement,
    );
    left_acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
    left_acknowledgement.resolves_event_ids = vec![
        left_attempt.event_id.clone(),
        left_observation.event_id.clone(),
    ];
    let mut right_attempt = event(
        "tracker-event-right-attempt",
        "tracker-operation-1",
        TrackerOperationPhase::Attempt,
    );
    right_attempt.receipt_ids = vec!["receipt_right".into()];
    right_attempt.run_ids = vec!["run_right".into()];
    let facts = [
        &intent,
        &left_attempt,
        &left_observation,
        &right_attempt,
        &left_acknowledgement,
    ];
    for fact in facts {
        append_line(&path, &serde_json::to_value(fact).unwrap());
    }
    for fact in facts.into_iter().rev() {
        append_line(&reverse_path, &serde_json::to_value(fact).unwrap());
    }

    let pending = tracker_operation_projection_from_path(&path, &|| false).unwrap();
    assert_eq!(
        pending,
        tracker_operation_projection_from_path(&reverse_path, &|| false).unwrap()
    );
    assert_eq!(
        pending.operations["tracker-operation-1"].terminal_outcome,
        None
    );
    assert!(
        pending
            .pending_retention_roots
            .receipt_ids
            .contains("receipt_right")
    );
    assert!(
        pending
            .pending_retention_roots
            .run_ids
            .contains("run_right")
    );

    let mut reconciliation = event(
        "tracker-event-reconciliation-ack",
        "tracker-operation-1",
        TrackerOperationPhase::Acknowledgement,
    );
    reconciliation.outcome = Some(TrackerOperationOutcome::Applied);
    reconciliation.resolves_event_ids = vec![right_attempt.event_id];
    append_tracker_operation_event_at_path(&path, &reconciliation).unwrap();

    let terminal = tracker_operation_projection_from_path(&path, &|| false).unwrap();
    assert_eq!(
        terminal.operations["tracker-operation-1"].terminal_outcome,
        Some(TrackerOperationOutcome::Applied)
    );
    assert!(terminal.pending_retention_roots.plan_ids.is_empty());
}

#[test]
fn bounded_acknowledgements_collectively_resolve_more_than_one_record_can_name() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let intent = event(
        "tracker-event-intent",
        "tracker-operation-many-facts",
        TrackerOperationPhase::Intent,
    );
    append_tracker_operation_event_at_path(&path, &intent).unwrap();

    let mut unresolved = Vec::new();
    for index in 0..33 {
        let attempt_id = format!("tracker-event-attempt-{index:02}");
        let attempt = event(
            &attempt_id,
            "tracker-operation-many-facts",
            TrackerOperationPhase::Attempt,
        );
        append_tracker_operation_event_at_path(&path, &attempt).unwrap();
        unresolved.push(attempt_id);

        let observation_id = format!("tracker-event-observation-{index:02}");
        let observation = event(
            &observation_id,
            "tracker-operation-many-facts",
            TrackerOperationPhase::Observation,
        );
        append_tracker_operation_event_at_path(&path, &observation).unwrap();
        unresolved.push(observation_id);
    }

    let mut first_acknowledgement = event(
        "tracker-event-acknowledgement-1",
        "tracker-operation-many-facts",
        TrackerOperationPhase::Acknowledgement,
    );
    first_acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
    first_acknowledgement.resolves_event_ids = unresolved[..MAX_REFERENCES_PER_EVENT].to_vec();
    append_tracker_operation_event_at_path(&path, &first_acknowledgement).unwrap();
    assert!(
        tracker_operation_projection_from_path(&path, &|| false)
            .unwrap()
            .operations["tracker-operation-many-facts"]
            .is_pending()
    );

    let mut final_acknowledgement = event(
        "tracker-event-acknowledgement-2",
        "tracker-operation-many-facts",
        TrackerOperationPhase::Acknowledgement,
    );
    final_acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
    final_acknowledgement.resolves_event_ids = unresolved[MAX_REFERENCES_PER_EVENT..].to_vec();
    append_tracker_operation_event_at_path(&path, &final_acknowledgement).unwrap();
    assert_eq!(
        tracker_operation_projection_from_path(&path, &|| false)
            .unwrap()
            .operations["tracker-operation-many-facts"]
            .terminal_outcome,
        Some(TrackerOperationOutcome::Applied)
    );
}

#[test]
fn merged_acknowledgement_tail_requires_reconciliation_evidence_before_resolution() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let intent = event(
        "tracker-event-intent",
        "tracker-operation-merged-tail",
        TrackerOperationPhase::Intent,
    );
    let attempt = event(
        "tracker-event-branch-attempt",
        "tracker-operation-merged-tail",
        TrackerOperationPhase::Attempt,
    );
    let mut branch_acknowledgement = event(
        "tracker-event-branch-acknowledgement",
        "tracker-operation-merged-tail",
        TrackerOperationPhase::Acknowledgement,
    );
    branch_acknowledgement.outcome = Some(TrackerOperationOutcome::NoEffect);
    for fact in [&intent, &attempt, &branch_acknowledgement] {
        append_line(&path, &serde_json::to_value(fact).unwrap());
    }
    assert!(
        tracker_operation_projection_from_path(&path, &|| false)
            .unwrap()
            .operations["tracker-operation-merged-tail"]
            .is_pending()
    );

    let mut unsupported_resolution = event(
        "tracker-event-unsupported-resolution",
        "tracker-operation-merged-tail",
        TrackerOperationPhase::Acknowledgement,
    );
    unsupported_resolution.outcome = Some(TrackerOperationOutcome::NoEffect);
    unsupported_resolution.resolves_event_ids = vec![attempt.event_id.clone()];
    let bytes_before = fs::read(&path).unwrap();

    let error = append_tracker_operation_event_at_path(&path, &unsupported_resolution)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("without observation or error evidence"),
        "unexpected error: {error}"
    );
    assert_eq!(fs::read(&path).unwrap(), bytes_before);
    tracker_operation_projection_from_path(&path, &|| false).unwrap();

    let observation = event(
        "tracker-event-reconciliation-observation",
        "tracker-operation-merged-tail",
        TrackerOperationPhase::Observation,
    );
    append_tracker_operation_event_at_path(&path, &observation).unwrap();
    let mut final_acknowledgement = event(
        "tracker-event-final-acknowledgement",
        "tracker-operation-merged-tail",
        TrackerOperationPhase::Acknowledgement,
    );
    final_acknowledgement.outcome = Some(TrackerOperationOutcome::NoEffect);
    final_acknowledgement.resolves_event_ids = vec![attempt.event_id, observation.event_id];
    append_tracker_operation_event_at_path(&path, &final_acknowledgement).unwrap();

    assert_eq!(
        tracker_operation_projection_from_path(&path, &|| false)
            .unwrap()
            .operations["tracker-operation-merged-tail"]
            .terminal_outcome,
        Some(TrackerOperationOutcome::NoEffect)
    );
}
