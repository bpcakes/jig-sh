#[test]
fn work_receipts_summary_handles_empty_results() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": []
    }));

    assert!(summary.contains("Showing: 0"));
    assert!(summary.contains("No receipts matched"));
}

#[test]
fn work_receipts_summary_omits_output_line_without_preview() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.test",
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": null
        }]
    }));

    assert!(summary.contains("jig.test (receipt_1): exit 0, no changes"));
    assert!(summary.contains("plan: none; session: none"));
    assert!(!summary.contains("output:"));
}

#[test]
fn explain_summary_labels_cargo_scope_as_candidate_and_preserves_target_authority() {
    let summary = format_repository_execution_summary(
        &json!({
            "executed": false,
            "plan": {
                "id": "run-plan_example",
                "targets": [],
                "cargo_impacts": [{
                    "component": "api",
                    "disposition": "narrowed",
                    "build_packages": [{"selector": "example@0.1.0"}],
                    "test_targets": [{"name": "example", "kinds": ["lib"]}]
                }]
            }
        }),
        "Run",
        "run",
    );

    assert!(summary.contains("Cargo candidate build scope"));
    assert!(summary.contains("configured target selection remains authoritative"));
    assert!(summary.contains("example@0.1.0"));
    assert!(!summary.contains("path+file://"));
}
