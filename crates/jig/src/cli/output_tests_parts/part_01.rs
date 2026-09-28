
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
