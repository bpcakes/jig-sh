use super::*;

#[test]
fn receipt_linkage_analysis_rejects_unusable_identities() {
    let mut collector = RunLinkageCollector::default();
    assert!(analyze_receipt_linkage(br#"{"tool_name":"x"}"#, &mut collector).is_err());
    assert!(analyze_receipt_linkage(br#"{"id":"r","run_id":7}"#, &mut collector).is_err());
    analyze_receipt_linkage(br#"{"id":"r","run_id":"run_1"}"#, &mut collector).unwrap();
    assert_eq!(collector.receipts_with_run_id, 1);
    assert_eq!(
        collector.receipt_runs.get("r").map(String::as_str),
        Some("run_1")
    );
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
