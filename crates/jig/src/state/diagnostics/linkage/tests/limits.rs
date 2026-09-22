use super::*;

#[test]
fn receipt_linkage_analysis_rejects_unusable_identities() {
    let mut collector = RunLinkageCollector::default();
    assert!(analyze_receipt_linkage(br#"{"tool_name":"x"}"#, &mut collector).is_err());
    assert!(analyze_receipt_linkage(br#"{"id":"r","run_id":7}"#, &mut collector).is_err());
    analyze_receipt_linkage(br#"{"id":"r","run_id":"run_1"}"#, &mut collector).unwrap();
    assert_eq!(collector.receipts_with_run_id, 1);
    assert_eq!(
        collector.receipt_runs.get("r"),
        Some(&BTreeSet::from(["run_1".into()]))
    );
}

fn diagnose_conflicting_receipt_runs(reverse: bool) -> Value {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let present_run = started.result.run_id;
    complete_target(&ctx, &present_run);
    complete_run(&ctx, &present_run, RunConclusion::Success).unwrap();
    drop(lease);
    let missing = target_receipt("receipt_same", "jig.test", "api:test", Some("run_missing"));
    let present = target_receipt("receipt_same", "jig.test", "api:test", Some(&present_run));
    let records = if reverse {
        vec![present, missing]
    } else {
        vec![missing, present]
    };
    write_records(&ctx.state_file("receipts.jsonl"), &records);
    diagnose(&ctx, true)
}

#[test]
fn conflicting_duplicate_receipt_runs_are_order_independent_and_incomplete() {
    for reverse in [false, true] {
        let output = diagnose_conflicting_receipt_runs(reverse);
        let linkage = &output["run_linkage"];
        assert_eq!(linkage["complete"], false);
        assert_ne!(linkage["verdict"], "clean");
        assert_eq!(linkage["referenced_runs"], 2);
        assert_eq!(finding_for(&output, "run_missing")["status"], "missing");
        assert_string_array_contains(
            &linkage["incomplete_reasons"],
            "1 receipt ID(s) reference conflicting runs",
        );
    }
}

#[test]
fn identical_duplicate_receipt_runs_remain_clean() {
    let (_temp, ctx) = fixture_context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    complete_target(&ctx, &run_id);
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    let receipt = target_receipt("receipt_same", "jig.test", "api:test", Some(&run_id));
    write_records(
        &ctx.state_file("receipts.jsonl"),
        &[receipt.clone(), receipt],
    );

    let output = diagnose(&ctx, true);

    assert_eq!(output["run_linkage"]["verdict"], "clean");
    assert_eq!(output["run_linkage"]["complete"], true);
    assert_eq!(output["run_linkage"]["referenced_runs"], 1);
}

#[test]
fn tracked_duplicate_receipt_runs_survive_reference_exhaustion_in_both_orders() {
    for (first_run, second_run) in [(RUN_A, RUN_B), (RUN_B, RUN_A)] {
        let mut collector = RunLinkageCollector {
            receipt_ids: BTreeSet::from(["receipt_same".into()]),
            receipt_runs: BTreeMap::from([(
                "receipt_same".into(),
                BTreeSet::from([first_run.into()]),
            )]),
            tracked_references: MAX_TRACKED_REFERENCES,
            ..RunLinkageCollector::default()
        };
        let receipt = serde_json::to_vec(&json!({
            "id": "receipt_same",
            "run_id": second_run,
        }))
        .unwrap();

        analyze_receipt_linkage(&receipt, &mut collector).unwrap();

        assert!(collector.reference_budget_exceeded);
        assert_eq!(
            collector.receipt_runs["receipt_same"],
            BTreeSet::from([RUN_A.into(), RUN_B.into()])
        );
        assert!(collector.conflicting_receipt_runs.contains("receipt_same"));
        assert_eq!(collect_references(&collector).runs.len(), 2);
    }
}

#[test]
fn receipt_reference_exhaustion_does_not_starve_journal_lifecycles() {
    let mut collector = RunLinkageCollector {
        tracked_references: MAX_TRACKED_REFERENCES,
        ..RunLinkageCollector::default()
    };
    analyze_receipt_linkage(
        br#"{"id":"receipt_over_budget","run_id":"run_over_budget"}"#,
        &mut collector,
    )
    .unwrap();
    assert!(collector.reference_budget_exceeded);

    let queued = serde_json::to_vec(&json!({
        "id": "run_event_queued",
        "run_id": RUN_A,
        "event": "queued",
        "timestamp_ms": 1,
        "plan": plan(),
    }))
    .unwrap();
    collector.observe_run_event(&queued);

    assert!(collector.journal.contains_key(RUN_A));
    assert_eq!(collector.journal[RUN_A].status(), LifecycleStatus::Active);
    assert!(!collector.lifecycle_budget_exceeded);
}

#[test]
fn supported_batch_retains_only_the_reference_budget_prefix() {
    let mut collector = RunLinkageCollector {
        tracked_references: MAX_TRACKED_REFERENCES - 2,
        ..RunLinkageCollector::default()
    };
    let receipt = serde_json::to_vec(&work_check_targets_receipt(
        "receipt_batch",
        &[
            ("api:test", "receipt_test", RUN_A),
            ("api:fmt", "receipt_fmt", RUN_B),
            (
                "api:clippy",
                "receipt_clippy",
                "run_01ARZ3NDEKTSV4RRFFQ69G5FB3",
            ),
        ],
    ))
    .unwrap();

    analyze_receipt_linkage(&receipt, &mut collector).unwrap();

    assert_eq!(collector.tracked_references, MAX_TRACKED_REFERENCES);
    assert!(collector.reference_budget_exceeded);
    assert_eq!(collector.batch_receipts, 1);
    assert_eq!(collector.batch_links, 3);
    assert_eq!(collector.batches.len(), 1);
    let references = collect_references(&collector);
    assert_eq!(references.runs.len(), 1);
    assert!(references.runs.contains_key(RUN_A));
    assert!(!references.runs.contains_key(RUN_B));
}

#[test]
fn archived_receipt_associations_respect_the_remaining_reference_budget() {
    let (_temp, ctx) = fixture_context();
    let archive = ctx
        .root()
        .join(".agent/.cache/state-archives/receipts-before-20260922T000000Z.jsonl.gz");
    let records = [
        target_receipt("receipt_child", "jig.test", "api:test", Some(RUN_A)),
        target_receipt("receipt_child", "jig.test", "api:test", Some(RUN_B)),
    ];
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, &record).unwrap();
        bytes.push(b'\n');
    }
    write_gzip(&archive, &bytes);

    let mut collector = RunLinkageCollector::default();
    let batch = serde_json::to_vec(&work_check_targets_receipt(
        "receipt_batch",
        &[("api:test", "receipt_child", RUN_A)],
    ))
    .unwrap();
    analyze_receipt_linkage(&batch, &mut collector).unwrap();
    collector.tracked_references = MAX_TRACKED_REFERENCES - 1;

    let linkage = resolve(ctx.root(), collector, None, None).to_value();

    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["reference_budget_exceeded"], true);
    assert_eq!(
        linkage["receipt_history"]["reference_budget_exhausted"],
        true
    );
    assert_eq!(linkage["receipt_history"]["error_count"], 1);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "local receipt history scan exhausted the linkage reference budget",
    );
}

#[test]
fn archived_receipt_identity_without_run_respects_the_reference_budget() {
    let (_temp, ctx) = fixture_context();
    let archive = ctx
        .root()
        .join(".agent/.cache/state-archives/receipts-before-20260922T000000Z.jsonl.gz");
    let receipt = target_receipt("receipt_child", "jig.test", "api:test", None);
    let mut bytes = serde_json::to_vec(&receipt).unwrap();
    bytes.push(b'\n');
    write_gzip(&archive, &bytes);

    let mut collector = RunLinkageCollector::default();
    let batch = serde_json::to_vec(&work_check_gates_receipt(
        "receipt_batch",
        &["receipt_child"],
    ))
    .unwrap();
    analyze_receipt_linkage(&batch, &mut collector).unwrap();
    collector.tracked_references = MAX_TRACKED_REFERENCES;

    let linkage = resolve(ctx.root(), collector, None, None).to_value();

    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["reference_budget_exceeded"], true);
    assert_eq!(
        linkage["receipt_history"]["reference_budget_exhausted"],
        true
    );
    assert_eq!(linkage["receipt_history"]["error_count"], 1);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "local receipt history scan exhausted the linkage reference budget",
    );
}

#[test]
fn receipt_skipped_by_reference_budget_is_not_reported_as_missing() {
    let (_temp, ctx) = fixture_context();
    let mut collector = RunLinkageCollector {
        tracked_references: MAX_TRACKED_REFERENCES - 2,
        ..RunLinkageCollector::default()
    };
    let batch = serde_json::to_vec(&work_check_targets_receipt(
        "receipt_batch",
        &[("api:test", "receipt_child", RUN_A)],
    ))
    .unwrap();
    analyze_receipt_linkage(&batch, &mut collector).unwrap();
    let child = serde_json::to_vec(&target_receipt(
        "receipt_child",
        "jig.test",
        "api:test",
        Some(RUN_A),
    ))
    .unwrap();
    analyze_receipt_linkage(&child, &mut collector).unwrap();

    assert!(collector.reference_budget_exceeded);
    assert!(!collector.receipt_ids.contains("receipt_child"));
    assert_eq!(collect_references(&collector).unresolved_batch_links, 0);

    let linkage = resolve(ctx.root(), collector, None, None).to_value();
    assert_eq!(linkage["complete"], false);
    assert_eq!(linkage["unresolved_batch_links"], 0);
    assert_string_array_contains(
        &linkage["incomplete_reasons"],
        "later references were not tracked",
    );
    assert!(
        linkage["incomplete_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|reason| !reason
                .as_str()
                .unwrap()
                .contains("missing receipt identities"))
    );
}

#[test]
fn reference_exhaustion_marks_preservation_recommendations_as_truncated() {
    let report = RunLinkageReport {
        checked: true,
        complete: false,
        reference_budget_exceeded: true,
        runs: RunLinkageCounts {
            missing: 1,
            ..RunLinkageCounts::default()
        },
        findings: vec![RunLinkageFinding {
            run_id: RUN_A.into(),
            status: "missing".into(),
            detail: "test fixture".into(),
            receipt_ids: vec!["receipt_test".into()],
            receipt_count: 1,
            receipt_ids_truncated: false,
            batch_receipt_ids: Vec::new(),
            batch_receipt_count: 0,
            batch_receipt_ids_truncated: false,
            journal_events: 0,
            journal_anomalies: Vec::new(),
            lease_file_present: None,
            history_sources: Vec::new(),
            recovery: None,
        }],
        finding_count: 1,
        ..RunLinkageReport::default()
    };

    let recommendation = &recommendations(&report)[0];
    assert_eq!(recommendation["affected_run_ids_truncated"], true);
    assert!(
        recommendation["reason"]
            .as_str()
            .unwrap()
            .starts_with("At least 1 retained receipt(s) reference at least 1 retained run(s)")
    );
}
