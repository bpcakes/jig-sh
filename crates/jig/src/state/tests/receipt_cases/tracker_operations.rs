use std::collections::BTreeSet;

use super::*;

fn tracker_operation_fact(
    event_id: &str,
    operation_id: &str,
    plan_id: &str,
    phase: &str,
    outcome: Option<&str>,
    receipt_ids: &[&str],
) -> Value {
    let mut fact = json!({
        "schema_version": 1,
        "event_id": event_id,
        "operation_id": operation_id,
        "plan_id": plan_id,
        "issue": {
            "provider": "beads",
            "workspace_id": "ExampleProject",
            "issue_id": "example-123",
            "tracker_root": ".beads",
        },
        "kind": "export",
        "phase": phase,
        "timestamp_ms": 1,
        "receipt_ids": receipt_ids,
    });
    if let Some(outcome) = outcome {
        fact["outcome"] = json!(outcome);
    }
    fact
}

#[test]
fn pending_tracker_operation_protects_a_closed_plans_explicit_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "# Example plan\n").unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close(
            "plan-event-close-example".into(),
            "plan_example".into(),
            2,
            Some("done".into()),
        ),
    )
    .unwrap();
    let mut receipt = receipt_record(
        "receipt_pending_tracker",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    receipt.plan_id = Some("plan_example".into());
    receipt.ended_at_ms = 10;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &receipt).unwrap();
    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-intent",
            "tracker-operation-export",
            "plan_example",
            "intent",
            None,
            &["receipt_pending_tracker"],
        ),
    )
    .unwrap();

    let retained = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(retained["receipts_archived"], 0);
    assert_eq!(retained["protected_receipts_retained"], 1);
    assert_eq!(
        read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
            .unwrap()
            .into_iter()
            .map(|receipt| receipt.id)
            .collect::<Vec<_>>(),
        ["receipt_pending_tracker"]
    );

    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-acknowledgement",
            "tracker-operation-export",
            "plan_example",
            "acknowledgement",
            Some("no_effect"),
            &["receipt_pending_tracker"],
        ),
    )
    .unwrap();
    let archived = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(archived["receipts_archived"], 1);
    assert!(
        read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn merged_acknowledgement_keeps_an_unresolved_branch_receipt_live() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "# Example plan\n").unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close(
            "plan-event-close-example".into(),
            "plan_example".into(),
            2,
            Some("done".into()),
        ),
    )
    .unwrap();
    let mut receipt = receipt_record(
        "receipt_unresolved_branch",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    receipt.plan_id = Some("plan_example".into());
    receipt.ended_at_ms = 10;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &receipt).unwrap();

    let intent = tracker_operation_fact(
        "tracker-event-intent",
        "tracker-operation-export",
        "plan_example",
        "intent",
        None,
        &[],
    );
    let left_attempt = tracker_operation_fact(
        "tracker-event-left-attempt",
        "tracker-operation-export",
        "plan_example",
        "attempt",
        None,
        &[],
    );
    let left_observation = tracker_operation_fact(
        "tracker-event-left-observation",
        "tracker-operation-export",
        "plan_example",
        "observation",
        None,
        &[],
    );
    let mut left_acknowledgement = tracker_operation_fact(
        "tracker-event-left-acknowledgement",
        "tracker-operation-export",
        "plan_example",
        "acknowledgement",
        Some("no_effect"),
        &[],
    );
    left_acknowledgement["resolves_event_ids"] = json!([
        "tracker-event-left-attempt",
        "tracker-event-left-observation"
    ]);
    let right_attempt = tracker_operation_fact(
        "tracker-event-right-attempt",
        "tracker-operation-export",
        "plan_example",
        "attempt",
        None,
        &["receipt_unresolved_branch"],
    );
    for fact in [
        intent,
        left_attempt,
        left_observation,
        left_acknowledgement,
        right_attempt,
    ] {
        append_jsonl(&ctx.state_file("tracker-operations.jsonl"), &fact).unwrap();
    }

    let retained = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(retained["receipts_archived"], 0);
    assert_eq!(retained["protected_receipts_retained"], 1);

    let mut reconciliation = tracker_operation_fact(
        "tracker-event-reconciliation",
        "tracker-operation-export",
        "plan_example",
        "acknowledgement",
        Some("no_effect"),
        &[],
    );
    reconciliation["resolves_event_ids"] = json!(["tracker-event-right-attempt"]);
    append_jsonl(&ctx.state_file("tracker-operations.jsonl"), &reconciliation).unwrap();

    let archived = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(archived["receipts_archived"], 1);
}

#[test]
fn pending_tracker_receipt_root_protects_transitive_freshness_evidence() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
rust_test_command = "true"

[[work.gates]]
id = "current-tests"
kind = "check"
tool = "jig.test"
"#,
        )
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": tool::TEST,
            "kind": "command",
            "description": "Run the current tests.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "# Example plan\n").unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close(
            "plan-event-close-example".into(),
            "plan_example".into(),
            2,
            Some("done".into()),
        ),
    )
    .unwrap();

    let dependency_target: jig_contract::TargetId = "example:dependency".parse().unwrap();
    let root_target: jig_contract::TargetId = "example:root".parse().unwrap();
    let mut dependency = receipt_record(
        "receipt_target_dependency",
        "jig.target_run",
        0,
        DiffStat::default(),
    );
    dependency.plan_id = Some("plan_example".into());
    dependency.run_id = Some("run_dependency".into());
    dependency.target = Some(dependency_target.clone());
    dependency.ended_at_ms = 10;
    let mut root = receipt_record(
        "receipt_target_root",
        "jig.target_run",
        0,
        DiffStat::default(),
    );
    root.plan_id = Some("plan_example".into());
    root.run_id = Some("run_root".into());
    root.target = Some(root_target.clone());
    root.ended_at_ms = 20;
    root.target_freshness = Some(
        serde_json::from_value(json!({
            "schema_version": 1,
            "contract_epoch": 8,
            "state": "complete",
            "identity": {
                "contract_epoch": 8,
                "schema_version": 1,
                "digest_domain": jig_contract::freshness::TARGET_IDENTITY_DOMAIN,
                "target": root_target,
                "inputs_policy": "exhaustive",
                "source_state": "git",
                "source_digest": "source-root",
                "authority_digest": "authority-root",
                "dependency_digest": "dependencies-root",
                "identity_digest": "identity-root",
                "configuration_digest": "historical-configuration",
                "runner_digest": "historical-runner",
                "invocation_digest": "historical-invocation",
                "source_preview": [],
                "source_entry_count": 0,
                "source_preview_truncated": false,
                "dependencies": [{
                    "target": dependency_target,
                    "identity_digest": "identity-dependency",
                }],
            },
            "dependency_execution_proof": [{
                "target": dependency_target,
                "receipt_id": "receipt_target_dependency",
                "run_id": "run_dependency",
                "plan_id": "plan_example",
                "identity_digest": "identity-dependency",
                "conclusion": "success",
                "effective_valid_until_ms": null,
                "effective_requires_time_validity": false,
            }],
            "effective_valid_until_ms": null,
            "effective_requires_time_validity": false,
            "global_execution_proof": {"state": "unknown"},
        }))
        .unwrap(),
    );
    let mut unrelated = receipt_record(
        "receipt_unrelated_old",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated.plan_id = Some("plan_example".into());
    unrelated.ended_at_ms = 30;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &dependency).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &root).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated).unwrap();
    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-intent",
            "tracker-operation-export",
            "plan_example",
            "intent",
            None,
            &["receipt_target_root"],
        ),
    )
    .unwrap();

    let archived = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(archived["receipts_archived"], 1);
    assert_eq!(archived["protected_receipts_retained"], 2);
    let retained = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .into_iter()
        .map(|receipt| receipt.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        retained,
        BTreeSet::from([
            "receipt_target_dependency".into(),
            "receipt_target_root".into(),
        ])
    );
}

#[test]
fn pending_tracker_root_preserves_targetless_work_check_children_after_gate_removal() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "# Example plan\n").unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close(
            "plan-event-close-example".into(),
            "plan_example".into(),
            2,
            Some("done".into()),
        ),
    )
    .unwrap();
    let mut child = receipt_record(
        "receipt_removed_gate_child",
        tool::TEST,
        0,
        DiffStat::default(),
    );
    child.plan_id = Some("plan_example".into());
    child.ended_at_ms = 10;
    let mut batch = receipt_record(
        "receipt_pending_work_check",
        tool::WORK_CHECK,
        0,
        DiffStat::default(),
    );
    batch.plan_id = Some("plan_example".into());
    batch.ended_at_ms = 20;
    batch.args = json!({
        "gates": ["removed-gate"],
        "tools": [tool::TEST],
        "receipt_ids": ["receipt_removed_gate_child"]
    });
    let mut unrelated = receipt_record(
        "receipt_unrelated_old",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated.plan_id = Some("plan_example".into());
    unrelated.ended_at_ms = 30;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &child).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &batch).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated).unwrap();
    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-intent",
            "tracker-operation-export",
            "plan_example",
            "intent",
            None,
            &["receipt_pending_work_check"],
        ),
    )
    .unwrap();

    let archived = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(archived["receipts_archived"], 1);
    assert_eq!(archived["protected_receipts_retained"], 2);
    let retained = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .into_iter()
        .map(|receipt| receipt.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        retained,
        BTreeSet::from([
            "receipt_pending_work_check".into(),
            "receipt_removed_gate_child".into(),
        ])
    );
}

#[test]
fn missing_pending_tracker_receipt_root_does_not_block_active_archive() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "# Example plan\n").unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close(
            "plan-event-close-example".into(),
            "plan_example".into(),
            2,
            Some("done".into()),
        ),
    )
    .unwrap();
    let mut unrelated = receipt_record(
        "receipt_unrelated_old",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated.ended_at_ms = 10;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated).unwrap();
    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-intent",
            "tracker-operation-export",
            "plan_example",
            "intent",
            None,
            &["receipt_never_committed"],
        ),
    )
    .unwrap();

    let archived = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(archived["receipts_archived"], 1);
    assert_eq!(archived["protected_receipts_retained"], 0);
    assert!(
        read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_tracker_operation_authority_aborts_receipt_archive_without_artifacts() {
    let torn = serde_json::to_vec(&tracker_operation_fact(
        "tracker-event-torn",
        "tracker-operation-export",
        "plan_example",
        "intent",
        None,
        &["receipt_pending_tracker"],
    ))
    .unwrap();
    let transition_intent = tracker_operation_fact(
        "tracker-event-intent",
        "tracker-operation-invalid-transition",
        "plan_example",
        "intent",
        None,
        &["receipt_pending_tracker"],
    );
    let transition_attempt = tracker_operation_fact(
        "tracker-event-attempt",
        "tracker-operation-invalid-transition",
        "plan_example",
        "attempt",
        None,
        &["receipt_pending_tracker"],
    );
    let mut transition_acknowledgement = tracker_operation_fact(
        "tracker-event-acknowledgement",
        "tracker-operation-invalid-transition",
        "plan_example",
        "acknowledgement",
        Some("applied"),
        &["receipt_pending_tracker"],
    );
    transition_acknowledgement["resolves_event_ids"] = json!(["tracker-event-attempt"]);
    let invalid_transition = [
        transition_intent,
        transition_attempt,
        transition_acknowledgement,
    ]
    .into_iter()
    .map(|fact| format!("{fact}\n"))
    .collect::<String>()
    .into_bytes();
    let cases = [
        ("corrupt", b"{not-json}\n".to_vec(), "Failed to parse"),
        (
            "future",
            b"{\"schema_version\":99}\n".to_vec(),
            "unsupported schema version 99",
        ),
        ("torn", torn, "unterminated final record"),
        (
            "invalid-transition",
            invalid_transition,
            "without observation or error evidence",
        ),
    ];

    for (case, authority, expected_error) in cases {
        let temp = tempdir().unwrap();
        write_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        ensure_state_layout(&ctx).unwrap();
        let mut receipt = receipt_record(
            "receipt_pending_tracker",
            tool::CLIPPY,
            0,
            DiffStat::default(),
        );
        receipt.ended_at_ms = 10;
        append_jsonl(&ctx.state_file("receipts.jsonl"), &receipt).unwrap();
        let receipt_before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
        let tracker_path = ctx.state_file("tracker-operations.jsonl");
        fs::write(&tracker_path, &authority).unwrap();

        let error = receipts_archive(
            &ctx,
            StateArchiveRequest {
                before: "1000".into(),
                dry_run: false,
            },
        )
        .unwrap_err();

        assert!(
            format!("{error:#}").contains(expected_error),
            "unexpected {case} error: {error:#}"
        );
        assert_eq!(
            fs::read(ctx.state_file("receipts.jsonl")).unwrap(),
            receipt_before,
            "{case} authority changed receipt source"
        );
        assert_eq!(
            fs::read(&tracker_path).unwrap(),
            authority,
            "{case} authority changed tracker source"
        );
        assert!(!ctx.root().join(".agent/.cache/state-archives").exists());
        assert!(!ctx.root().join(".agent/.cache/state-backups").exists());
    }
}
