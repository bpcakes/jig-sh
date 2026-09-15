use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;

mod merge_semantics;
use crate::state::jsonl::{
    DurableAppendFailurePoint, JsonlRecordTooLarge, append_jsonl, fail_next_durable_append_at,
    parent_directory_sync_count, reset_parent_directory_sync_count,
};

fn issue() -> PortableTrackerIssueRef {
    PortableTrackerIssueRef {
        provider: "beads".into(),
        workspace_id: "ExampleProject".into(),
        issue_id: "example-123".into(),
        tracker_root: ".beads".into(),
    }
}

fn event(
    event_id: &str,
    operation_id: &str,
    phase: TrackerOperationPhase,
) -> TrackerOperationEventV1 {
    TrackerOperationEventV1 {
        schema_version: TRACKER_OPERATION_SCHEMA_VERSION,
        event_id: event_id.into(),
        operation_id: operation_id.into(),
        plan_id: "plan_example".into(),
        issue: issue(),
        kind: TrackerOperationKind::Claim,
        phase,
        timestamp_ms: 1,
        outcome: None,
        correlation: None,
        receipt_ids: vec!["receipt_example".into()],
        run_ids: vec!["run_example".into()],
        resolves_event_ids: Vec::new(),
        detail: None,
    }
}

fn append_line(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    let mut existing = fs::read(path).unwrap_or_default();
    existing.extend(bytes);
    fs::write(path, existing).unwrap();
}

#[test]
fn missing_journal_is_empty_and_read_only() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("missing/state/tracker-operations.jsonl");

    let projection = tracker_operation_projection_from_path(&path, &|| false).unwrap();

    assert!(projection.operations.is_empty());
    assert!(!path.parent().unwrap().exists());
}

#[test]
fn every_successful_append_confirms_the_journal_directory_entry() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    reset_parent_directory_sync_count();

    append_tracker_operation_event_at_path(&path, &intent).unwrap();
    assert_eq!(parent_directory_sync_count(), 1);

    let mut attempt = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Attempt,
    );
    attempt.timestamp_ms = 2;
    append_tracker_operation_event_at_path(&path, &attempt).unwrap();
    assert_eq!(parent_directory_sync_count(), 2);

    append_jsonl(
        &temp.path().join("ordinary.jsonl"),
        &json!({"ordinary": true}),
    )
    .unwrap();
    assert_eq!(
        parent_directory_sync_count(),
        2,
        "ordinary state appends keep their historical file-sync-only boundary"
    );
}

#[test]
fn exact_retry_reconfirms_durability_after_each_ambiguous_sync_failure() {
    for failure in [
        DurableAppendFailurePoint::BeforeFileSync,
        DurableAppendFailurePoint::BeforeParentSync,
    ] {
        let temp = tempdir().unwrap();
        let path = temp.path().join(TRACKER_OPERATIONS_FILE);
        let intent = event(
            "tracker-event-1",
            "tracker-operation-1",
            TrackerOperationPhase::Intent,
        );
        fail_next_durable_append_at(failure);

        let error = append_tracker_operation_event_at_path(&path, &intent).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected durable append failure")
        );
        let visible_bytes = fs::read(&path).unwrap();
        assert!(visible_bytes.ends_with(b"\n"));

        assert_eq!(
            append_tracker_operation_event_at_path(&path, &intent).unwrap(),
            TrackerOperationAppendOutcome::AlreadyPresent
        );
        assert_eq!(fs::read(&path).unwrap(), visible_bytes);
        assert_eq!(
            tracker_operation_projection_from_path(&path, &|| false)
                .unwrap()
                .operations
                .len(),
            1
        );
    }
}

#[test]
fn terminated_and_unterminated_oversized_records_are_bounded_before_decode() {
    for terminated in [true, false] {
        let temp = tempdir().unwrap();
        let path = temp.path().join(TRACKER_OPERATIONS_FILE);
        let mut bytes = vec![b'x'; MAX_RECORD_BYTES + 1];
        if terminated {
            bytes.push(b'\n');
        }
        fs::write(&path, bytes).unwrap();

        let error = tracker_operation_projection_from_path(&path, &|| false).unwrap_err();
        let oversized = error.downcast_ref::<JsonlRecordTooLarge>().unwrap();
        assert_eq!(oversized.start_offset(), 0);
        assert_eq!(oversized.limit(), MAX_RECORD_BYTES);
    }
}

#[test]
fn pending_and_terminal_operations_project_retention_roots() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    let mut attempt = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Attempt,
    );
    attempt.receipt_ids = vec!["receipt_attempt".into()];
    append_tracker_operation_event_at_path(&path, &intent).unwrap();
    append_tracker_operation_event_at_path(&path, &attempt).unwrap();

    let pending = tracker_operation_projection_from_path(&path, &|| false).unwrap();
    assert_eq!(pending.operations.len(), 1);
    assert_eq!(
        pending.pending_retention_roots.plan_ids,
        BTreeSet::from(["plan_example".into()])
    );
    assert_eq!(
        pending.pending_retention_roots.receipt_ids,
        BTreeSet::from(["receipt_attempt".into(), "receipt_example".into()])
    );
    assert_eq!(
        pending.pending_retention_roots.run_ids,
        BTreeSet::from(["run_example".into()])
    );

    let mut acknowledgement = event(
        "tracker-event-3",
        "tracker-operation-1",
        TrackerOperationPhase::Acknowledgement,
    );
    acknowledgement.outcome = Some(TrackerOperationOutcome::NoEffect);
    // An acknowledgement follows an observation, not an unresolved attempt.
    let observation = event(
        "tracker-event-observation",
        "tracker-operation-1",
        TrackerOperationPhase::Observation,
    );
    acknowledgement.resolves_event_ids = vec![attempt.event_id, observation.event_id.clone()];
    append_tracker_operation_event_at_path(&path, &observation).unwrap();
    append_tracker_operation_event_at_path(&path, &acknowledgement).unwrap();

    let terminal = tracker_operation_projection_from_path(&path, &|| false).unwrap();
    assert!(terminal.pending_retention_roots.plan_ids.is_empty());
    assert_eq!(
        terminal.operations["tracker-operation-1"].terminal_outcome,
        Some(TrackerOperationOutcome::NoEffect)
    );
}

#[test]
fn exact_append_retry_is_idempotent() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );

    assert_eq!(
        append_tracker_operation_event_at_path(&path, &intent).unwrap(),
        TrackerOperationAppendOutcome::Appended
    );
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        append_tracker_operation_event_at_path(&path, &intent).unwrap(),
        TrackerOperationAppendOutcome::AlreadyPresent
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn semantic_duplicates_include_unknown_fields() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let mut raw = serde_json::to_value(event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    ))
    .unwrap();
    raw["future_context"] = json!("same");
    append_line(&path, &raw);
    append_line(&path, &raw);

    let projection = tracker_operation_projection_from_path(&path, &|| false).unwrap();
    assert_eq!(projection.operations["tracker-operation-1"].events.len(), 1);
    assert_eq!(
        projection.operations["tracker-operation-1"].events[0].raw["future_context"],
        "same"
    );

    let mut conflicting = raw;
    conflicting["future_context"] = json!("different");
    append_line(&path, &conflicting);
    let error = tracker_operation_projection_from_path(&path, &|| false).unwrap_err();
    assert!(error.to_string().contains("conflicting semantic records"));
}

#[test]
fn conflicting_identity_and_invalid_transition_fail_closed() {
    let temp = tempdir().unwrap();
    let identity_path = temp.path().join("identity.jsonl");
    let intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    append_line(&identity_path, &serde_json::to_value(&intent).unwrap());
    let mut conflict = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Attempt,
    );
    conflict.plan_id = "plan_other".into();
    append_line(&identity_path, &serde_json::to_value(conflict).unwrap());
    assert!(
        tracker_operation_projection_from_path(&identity_path, &|| false)
            .unwrap_err()
            .to_string()
            .contains("conflicting immutable identity")
    );

    let transition_path = temp.path().join("transition.jsonl");
    append_line(&transition_path, &serde_json::to_value(&intent).unwrap());
    let second_intent = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    append_line(
        &transition_path,
        &serde_json::to_value(second_intent).unwrap(),
    );
    assert!(
        tracker_operation_projection_from_path(&transition_path, &|| false)
            .unwrap_err()
            .to_string()
            .contains("invalid tracker operation")
    );
}

#[test]
fn union_merged_facts_project_independently_of_physical_line_order() {
    let temp = tempdir().unwrap();
    let left = temp.path().join("left.jsonl");
    let right = temp.path().join("right.jsonl");
    let mut intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    intent.timestamp_ms = 1;
    let mut attempt = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Attempt,
    );
    attempt.timestamp_ms = 2;
    let mut acknowledgement = event(
        "tracker-event-4",
        "tracker-operation-1",
        TrackerOperationPhase::Acknowledgement,
    );
    acknowledgement.timestamp_ms = 4;
    acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
    let mut observation = event(
        "tracker-event-3",
        "tracker-operation-1",
        TrackerOperationPhase::Observation,
    );
    observation.timestamp_ms = 3;
    acknowledgement.resolves_event_ids =
        vec![attempt.event_id.clone(), observation.event_id.clone()];
    let mut concurrent_acknowledgement = acknowledgement.clone();
    concurrent_acknowledgement.event_id = "tracker-event-5".into();
    concurrent_acknowledgement.timestamp_ms = 5;

    for fact in [
        &intent,
        &attempt,
        &observation,
        &acknowledgement,
        &concurrent_acknowledgement,
    ] {
        append_line(&left, &serde_json::to_value(fact).unwrap());
    }
    for fact in [
        &concurrent_acknowledgement,
        &observation,
        &attempt,
        &intent,
        &acknowledgement,
    ] {
        append_line(&right, &serde_json::to_value(fact).unwrap());
    }

    let left_projection = tracker_operation_projection_from_path(&left, &|| false).unwrap();
    let right_projection = tracker_operation_projection_from_path(&right, &|| false).unwrap();
    assert_eq!(left_projection, right_projection);
    assert_eq!(
        left_projection.operations["tracker-operation-1"].terminal_outcome,
        Some(TrackerOperationOutcome::Applied)
    );
    assert!(left_projection.pending_retention_roots.plan_ids.is_empty());

    let mut later = event(
        "tracker-event-6",
        "tracker-operation-1",
        TrackerOperationPhase::Observation,
    );
    later.timestamp_ms = 6;
    let before = fs::read(&right).unwrap();
    let error = append_tracker_operation_event_at_path(&right, &later)
        .unwrap_err()
        .to_string();
    assert!(error.contains("terminal acknowledgement"));
    assert_eq!(fs::read(right).unwrap(), before);
}

#[test]
fn replay_rejects_an_acknowledged_attempt_without_reconciliation_evidence() {
    let temp = tempdir().unwrap();
    let path = temp.path().join(TRACKER_OPERATIONS_FILE);
    let intent = event(
        "tracker-event-intent",
        "tracker-operation-invalid",
        TrackerOperationPhase::Intent,
    );
    let attempt = event(
        "tracker-event-attempt",
        "tracker-operation-invalid",
        TrackerOperationPhase::Attempt,
    );
    let mut acknowledgement = event(
        "tracker-event-acknowledgement",
        "tracker-operation-invalid",
        TrackerOperationPhase::Acknowledgement,
    );
    acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
    acknowledgement.resolves_event_ids = vec![attempt.event_id.clone()];
    for fact in [&intent, &attempt, &acknowledgement] {
        append_line(&path, &serde_json::to_value(fact).unwrap());
    }

    let error = tracker_operation_projection_from_path(&path, &|| false)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("resolves an attempt without observation or error evidence"),
        "unexpected error: {error}"
    );
}

#[test]
fn append_transition_uses_the_physical_tail_not_caller_sort_keys() {
    let temp = tempdir().unwrap();
    for (case, retry_timestamp, retry_event_id) in [
        ("backdated", 25, "tracker-event-4"),
        ("same-time-earlier-id", 30, "tracker-event-0"),
    ] {
        let path = temp.path().join(format!("{case}.jsonl"));
        let mut intent = event(
            "tracker-event-1",
            "tracker-operation-1",
            TrackerOperationPhase::Intent,
        );
        intent.timestamp_ms = 10;
        let mut first_attempt = event(
            "tracker-event-2",
            "tracker-operation-1",
            TrackerOperationPhase::Attempt,
        );
        first_attempt.timestamp_ms = 20;
        let mut observation = event(
            "tracker-event-3",
            "tracker-operation-1",
            TrackerOperationPhase::Observation,
        );
        observation.timestamp_ms = 30;
        let mut retry = event(
            retry_event_id,
            "tracker-operation-1",
            TrackerOperationPhase::Attempt,
        );
        retry.timestamp_ms = retry_timestamp;
        for fact in [&intent, &first_attempt, &observation, &retry] {
            append_tracker_operation_event_at_path(&path, fact).unwrap();
        }
        let mut acknowledgement = event(
            "tracker-event-5",
            "tracker-operation-1",
            TrackerOperationPhase::Acknowledgement,
        );
        acknowledgement.timestamp_ms = 40;
        acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
        acknowledgement.resolves_event_ids = vec![
            first_attempt.event_id.clone(),
            observation.event_id.clone(),
            retry.event_id.clone(),
        ];
        let before = fs::read(&path).unwrap();

        let error = append_tracker_operation_event_at_path(&path, &acknowledgement)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("transition from Attempt to Acknowledgement"),
            "{case}: {error}"
        );
        assert_eq!(fs::read(path).unwrap(), before, "{case}");
    }
}

#[test]
fn operation_issue_identity_uses_the_shared_portable_validation() {
    let temp = tempdir().unwrap();
    for (case, workspace_id, issue_id) in [
        ("blank-workspace", " ", "example-123"),
        ("path-shaped-issue", "ExampleProject", "../example-123"),
    ] {
        let path = temp.path().join(format!("{case}.jsonl"));
        let mut invalid = event(
            "tracker-event-1",
            "tracker-operation-1",
            TrackerOperationPhase::Intent,
        );
        invalid.issue.workspace_id = workspace_id.into();
        invalid.issue.issue_id = issue_id.into();
        let error = append_tracker_operation_event_at_path(&path, &invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ASCII alphanumeric"), "{case}: {error}");
        assert!(!path.exists(), "{case} created a journal");
    }
}

#[test]
fn corrupt_future_and_unterminated_records_fail_closed() {
    let temp = tempdir().unwrap();

    let corrupt = temp.path().join("corrupt.jsonl");
    fs::write(&corrupt, b"{not-json}\n").unwrap();
    assert!(
        tracker_operation_projection_from_path(&corrupt, &|| false)
            .unwrap_err()
            .to_string()
            .contains("Failed to parse")
    );

    let duplicate_key = temp.path().join("duplicate-key.jsonl");
    fs::write(
        &duplicate_key,
        b"{\"schema_version\":1,\"schema_version\":1}\n",
    )
    .unwrap();
    let error = tracker_operation_projection_from_path(&duplicate_key, &|| false).unwrap_err();
    assert!(format!("{error:#}").contains("duplicate JSON object key"));

    let future = temp.path().join("future.jsonl");
    fs::write(&future, b"{\"schema_version\":99}\n").unwrap();
    assert!(
        tracker_operation_projection_from_path(&future, &|| false)
            .unwrap_err()
            .to_string()
            .contains("unsupported schema version 99")
    );

    let torn = temp.path().join("torn.jsonl");
    let intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    append_line(&torn, &serde_json::to_value(intent).unwrap());
    let mut acknowledgement = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Acknowledgement,
    );
    acknowledgement.outcome = Some(TrackerOperationOutcome::Applied);
    let mut bytes = fs::read(&torn).unwrap();
    bytes.extend(serde_json::to_vec(&acknowledgement).unwrap());
    fs::write(&torn, bytes).unwrap();
    let original = fs::read(&torn).unwrap();
    let error = tracker_operation_projection_from_path(&torn, &|| false).unwrap_err();
    assert!(error.to_string().contains("unterminated final record"));
    let append_error = append_tracker_operation_event_at_path(
        &torn,
        &event(
            "tracker-event-3",
            "tracker-operation-1",
            TrackerOperationPhase::Error,
        ),
    )
    .unwrap_err();
    assert!(append_error.to_string().contains("not newline-terminated"));
    assert_eq!(fs::read(&torn).unwrap(), original);
}

#[test]
fn acknowledgement_is_the_only_terminal_fact() {
    let mut intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );
    intent.outcome = Some(TrackerOperationOutcome::NoEffect);
    assert!(
        validate_fact(&intent)
            .unwrap_err()
            .to_string()
            .contains("only an acknowledgement")
    );

    let acknowledgement = event(
        "tracker-event-2",
        "tracker-operation-1",
        TrackerOperationPhase::Acknowledgement,
    );
    assert!(
        validate_fact(&acknowledgement)
            .unwrap_err()
            .to_string()
            .contains("requires")
    );
}

#[test]
fn write_contract_gate_starts_at_epoch_eleven() {
    let error = ensure_tracker_operations_write_contract_version(10).unwrap_err();
    assert!(error.to_string().contains("contract version 11"));
    ensure_tracker_operations_write_contract_version(11).unwrap();
    assert!(ensure_tracker_operations_write_contract_version(12).is_err());
}

#[test]
fn public_append_requires_matching_immutable_link_authority() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .repo_name("ExampleProject")
        .contract_version(crate::context::TRACKER_JOURNAL_CONTRACT_VERSION)
        .config(
            r#"[repository]
default_check_profile = "verify"
components = []
actions = []
profiles = []"#,
        )
        .write();
    let contract_path = temp.path().join(".agent/jig-contract.json");
    let mut contract: Value = serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
    contract["default_check_profile"] = json!("verify");
    fs::write(contract_path, serde_json::to_vec_pretty(&contract).unwrap()).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    super::super::plans::seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "Body")
        .unwrap();
    let intent = event(
        "tracker-event-1",
        "tracker-operation-1",
        TrackerOperationPhase::Intent,
    );

    let error = append_tracker_operation_event(&ctx, &intent).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires an immutable work link")
    );
    assert!(!ctx.state_file(TRACKER_OPERATIONS_FILE).exists());

    let link = super::super::work_links::WorkLinkRequest::new(
        "plan_example",
        super::super::work_links::WorkLinkIssueV1::beads("ExampleProject", "example-123").unwrap(),
        super::super::work_links::WorkLinkSnapshotV1::new(
            1,
            "Example task",
            "Acceptance: retain evidence.",
        )
        .unwrap(),
        super::super::work_links::WorkLinkEstablishedBy::Attach,
    )
    .unwrap();
    super::super::work_links::attach_work_link(&ctx, &link).unwrap();
    assert_eq!(
        append_tracker_operation_event(&ctx, &intent).unwrap(),
        TrackerOperationAppendOutcome::Appended
    );

    let before = fs::read(ctx.state_file(TRACKER_OPERATIONS_FILE)).unwrap();
    let mut mismatched = event(
        "tracker-event-2",
        "tracker-operation-2",
        TrackerOperationPhase::Intent,
    );
    mismatched.issue.issue_id = "example-456".into();
    let error = append_tracker_operation_event(&ctx, &mismatched).unwrap_err();
    assert!(error.to_string().contains("does not match"));
    assert_eq!(
        fs::read(ctx.state_file(TRACKER_OPERATIONS_FILE)).unwrap(),
        before
    );
}

#[test]
fn journal_diagnostics_report_pending_and_fail_closed_authority() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing/tracker-operations.jsonl");
    let empty = tracker_operation_journal_diagnostics_from_path(&missing);
    assert_eq!(empty.authority, TrackerOperationJournalAuthority::Empty);
    assert!(!missing.parent().unwrap().exists());

    let supported_path = temp.path().join("supported.jsonl");
    let intent = event(
        "tracker-event-supported",
        "tracker-operation-supported",
        TrackerOperationPhase::Intent,
    );
    append_line(&supported_path, &serde_json::to_value(&intent).unwrap());
    let supported = tracker_operation_journal_diagnostics_from_path(&supported_path);
    assert_eq!(
        supported.authority,
        TrackerOperationJournalAuthority::Supported
    );
    assert_eq!(supported.operations, 1);
    assert_eq!(supported.events, 1);
    assert_eq!(supported.pending_operations, 1);
    assert_eq!(supported.pending_plan_roots, 1);
    assert_eq!(supported.pending_receipt_roots, 1);
    assert_eq!(supported.pending_run_roots, 1);

    let unsupported_path = temp.path().join("unsupported.jsonl");
    fs::write(&unsupported_path, b"{\"schema_version\":99}\n").unwrap();
    let unsupported = tracker_operation_journal_diagnostics_from_path(&unsupported_path);
    assert_eq!(
        unsupported.authority,
        TrackerOperationJournalAuthority::Unsupported
    );

    let conflicting_path = temp.path().join("conflicting.jsonl");
    let first = serde_json::to_value(event(
        "tracker-event-conflict",
        "tracker-operation-conflict",
        TrackerOperationPhase::Intent,
    ))
    .unwrap();
    let mut second = first.clone();
    second["detail"] = json!("different complete semantics");
    append_line(&conflicting_path, &first);
    append_line(&conflicting_path, &second);
    let conflicting = tracker_operation_journal_diagnostics_from_path(&conflicting_path);
    assert_eq!(
        conflicting.authority,
        TrackerOperationJournalAuthority::Conflicting
    );

    let duplicate_intent_path = temp.path().join("duplicate-intent.jsonl");
    append_line(
        &duplicate_intent_path,
        &serde_json::to_value(&intent).unwrap(),
    );
    let second_intent = event(
        "tracker-event-corrupt",
        "tracker-operation-supported",
        TrackerOperationPhase::Intent,
    );
    append_line(
        &duplicate_intent_path,
        &serde_json::to_value(second_intent).unwrap(),
    );
    let duplicate_intent = tracker_operation_journal_diagnostics_from_path(&duplicate_intent_path);
    assert_eq!(
        duplicate_intent.authority,
        TrackerOperationJournalAuthority::Conflicting
    );

    let terminal_conflict_path = temp.path().join("terminal-conflict.jsonl");
    append_line(
        &terminal_conflict_path,
        &serde_json::to_value(&intent).unwrap(),
    );
    let mut applied = event(
        "tracker-event-applied",
        "tracker-operation-supported",
        TrackerOperationPhase::Acknowledgement,
    );
    applied.outcome = Some(TrackerOperationOutcome::Applied);
    let mut no_effect = applied.clone();
    no_effect.event_id = "tracker-event-no-effect".into();
    no_effect.outcome = Some(TrackerOperationOutcome::NoEffect);
    append_line(
        &terminal_conflict_path,
        &serde_json::to_value(applied).unwrap(),
    );
    append_line(
        &terminal_conflict_path,
        &serde_json::to_value(no_effect).unwrap(),
    );
    let terminal_conflict =
        tracker_operation_journal_diagnostics_from_path(&terminal_conflict_path);
    assert_eq!(
        terminal_conflict.authority,
        TrackerOperationJournalAuthority::Conflicting
    );

    let corrupt_path = temp.path().join("corrupt.jsonl");
    let mut corrupt_fact = intent;
    corrupt_fact.issue.workspace_id = "bad/workspace".into();
    append_line(&corrupt_path, &serde_json::to_value(corrupt_fact).unwrap());
    let corrupt = tracker_operation_journal_diagnostics_from_path(&corrupt_path);
    assert_eq!(corrupt.authority, TrackerOperationJournalAuthority::Corrupt);
    assert_eq!(corrupt.error_count, 1);
    assert_eq!(corrupt.errors.len(), 1);

    let torn_path = temp.path().join("torn.jsonl");
    fs::write(&torn_path, b"{").unwrap();
    let torn = tracker_operation_journal_diagnostics_from_path(&torn_path);
    assert_eq!(torn.authority, TrackerOperationJournalAuthority::Torn);
}
