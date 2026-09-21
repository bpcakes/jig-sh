#[test]
fn work_check_phase_summary_distinguishes_iteration_from_final_completion() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "selected_ok": true,
        "phase": "iteration",
        "explain": false,
        "plan_id": "plan_1",
        "final_gates_ok": false,
        "rust_focus_fallbacks": [{"target":{"component":"api","action":"test"},"reason":"unsupported_runner","scope":"configured_default"}],
        "selected_invocations": [{
            "target": {"component": "api", "action": "test"},
            "disposition": "selected",
            "evidence_validity": {
                "status": "passed",
                "receipt_id": "receipt_1",
                "run_id": "run_1"
            }
        }],
        "selected_checks": [],
        "pending_final_requirements": [
            {"id": "full", "kind": "evidence", "status": "missing"},
            {"id": "review", "kind": "codex_review", "status": "missing"}
        ],
        "final_recovery": {
            "inspection": "complete",
            "preview_available": true,
            "execute": [{"component": "api", "action": "test"}],
            "reuse": [],
            "targets": [],
            "message": "Current evidence was inspected.",
            "next_step": {
                "argv": ["scripts/jig", "work", "check", "--plan-id", "plan_1"],
                "read_only": false
            }
        }
    }));

    assert!(
        summary.contains("iteration passed; final validation pending"),
        "{summary}"
    );
    assert!(summary.contains("api:test: passed (selected)"), "{summary}");
    assert!(summary.contains("api:test; scope configured_default; reason unsupported_runner"));
    assert!(summary.contains("Final recovery inspection: complete"));
    assert!(summary.contains("Current evidence was inspected."));
    assert!(summary.contains("work check --plan-id plan_1"));
    assert!(summary.contains("work review --plan-id plan_1"));
}

#[test]
fn work_check_phase_summary_uses_read_only_recovery_when_execution_is_unavailable() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "selected_ok": false,
        "phase": "final",
        "explain": true,
        "plan_id": "plan_2",
        "final_gates_ok": false,
        "selected_invocations": [],
        "selected_checks": [],
        "pending_final_requirements": [
            {"id": "full", "kind": "evidence", "status": "unknown"}
        ],
        "final_recovery": {
            "inspection": "deadline_exhausted",
            "preview_available": false,
            "execute": [],
            "reuse": [],
            "targets": [],
            "message": "Retry read-only inspection before executing checks.",
            "next_step": {
                "argv": [
                    "scripts/jig", "work", "gates", "--plan-id", "plan_2",
                    "--freshness-timeout-ms", "30000"
                ],
                "read_only": true
            }
        }
    }));

    assert!(
        summary.contains("Final recovery inspection: deadline_exhausted"),
        "{summary}"
    );
    assert!(
        summary.contains("Retry read-only inspection before executing checks."),
        "{summary}"
    );
    assert!(
        summary.contains("work gates --plan-id plan_2 --freshness-timeout-ms 30000"),
        "{summary}"
    );
    assert!(!summary.contains("--phase final"), "{summary}");
}

fn rust_phase_report(scope: serde_json::Value) -> serde_json::Value {
    json!({
        "ok": true, "selected_ok": true, "phase": "iteration", "plan_id": "plan_example",
        "final_gates_ok": false,
        "selected_invocations": [{
            "target": {"component": "example", "action": "focused-test"},
            "disposition": "selected", "evidence_validity": {"status": "passed"},
            "invocation": {"prepared_rust_input": scope}
        }],
        "pending_final_requirements": [{"id": "full-test", "kind": "evidence", "status": "missing"}]
    })
}

#[test]
fn work_check_phase_summary_reports_exact_rust_scope_and_final_gaps() {
    let summary = format_work_check_summary(&rust_phase_report(json!({
        "disposition": "narrowed", "packages": ["example-core@1.2.3"],
        "targets": [{"kind": "lib"}, {"kind": "test", "name": "example_integration"}],
        "args": ["nextest", "run", "--filter-expr", "test(=example_case)"],
        "reasons": ["explicit_focus"]
    })));
    for expected in [
        "Rust scope: narrowed",
        "Packages: example-core@1.2.3",
        "Targets: lib, test:example_integration",
        "Explicit test filter (preview): test(=example_case)",
        "Scope reasons: explicit_focus",
        "Pending final requirements: 1",
        "full-test: missing (evidence)",
        "iteration passed; final validation pending",
    ] {
        assert!(
            summary.contains(expected),
            "missing {expected:?}: {summary}"
        );
    }
    assert!(!summary.contains("work finish"), "{summary}");
}

#[test]
fn work_check_phase_summary_distinguishes_full_scope_and_declared_broad_fallback() {
    for disposition in ["full", "broad_fallback"] {
        let summary = format_work_check_summary(&rust_phase_report(json!({
            "disposition": disposition, "packages": [], "targets": [], "args": [],
            "reasons": ["comparison_unavailable"]
        })));
        assert!(
            summary.contains(&format!("Rust scope: {disposition}")),
            "{summary}"
        );
        assert!(summary.contains("Packages: workspace"), "{summary}");
        assert!(summary.contains("Targets: all targets"), "{summary}");
        assert!(!summary.contains("Explicit test filter"), "{summary}");
    }
}

#[test]
fn work_check_phase_summary_reports_custom_runner_configured_default_fallback() {
    let mut value = rust_phase_report(serde_json::Value::Null);
    value["rust_focus_fallbacks"] = json!([{
        "target": {"component": "example", "action": "custom-test"},
        "reason": "unsupported_runner", "scope": "configured_default"
    }]);
    let summary = format_work_check_summary(&value);
    assert!(summary.contains("Rust focus fallback: example:custom-test; scope configured_default; reason unsupported_runner"), "{summary}");
    assert!(!summary.contains("Rust scope: narrowed"), "{summary}");
}

#[test]
fn work_check_phase_summary_bounds_rust_scope_lists_and_sanitizes_filter_preview() {
    let packages = (0..12)
        .map(|index| format!("example-{index}@1.0.0"))
        .collect::<Vec<_>>();
    let targets = (0..10)
        .map(|index| json!({"kind": "test", "name": format!("example_{index}")}))
        .collect::<Vec<_>>();
    let reasons = (0..11)
        .map(|index| format!("reason_{index}"))
        .collect::<Vec<_>>();
    let filter = format!("test(\u{1b}[31mexample\u{1b}[0m)\n{}", "x".repeat(500));
    let summary = format_work_check_summary(&rust_phase_report(json!({
        "disposition": "narrowed", "packages": packages, "targets": targets,
        "reasons": reasons, "args": ["--filter-expr", filter]
    })));
    for expected in [
        "4 more Packages",
        "2 more Targets",
        "3 more Scope reasons",
        "--json for details",
    ] {
        assert!(summary.contains(expected), "{summary}");
    }
    for omitted in [
        "example-8@",
        "test:example_8",
        "reason_8",
        "\u{1b}",
        &"x".repeat(141),
    ] {
        assert!(
            !summary.contains(omitted),
            "unexpected {omitted:?}: {summary}"
        );
    }
    let filter_line = summary
        .lines()
        .find(|line| line.contains("Explicit test filter"))
        .unwrap();
    assert!(filter_line.chars().count() < 185, "{filter_line}");
}

#[test]
fn work_check_phase_summary_empty_test_selection_remains_failed_evidence() {
    let mut value = rust_phase_report(json!({
        "disposition": "narrowed", "packages": ["example@1.0.0"], "targets": [{"kind": "lib"}],
        "args": ["--filter-expr", "none()"]
    }));
    value["ok"] = json!(false);
    value["selected_ok"] = json!(false);
    value["error"] = json!("Rust test selection was empty; no behavioral validation passed.");
    value["selected_invocations"][0]["evidence_validity"]["status"] = json!("failed");
    let summary = format_work_check_summary(&value);
    assert!(summary.contains("iteration selection failed"), "{summary}");
    assert!(summary.contains("test selection was empty"), "{summary}");
    assert!(summary.contains("focused-test: failed"), "{summary}");
    assert!(!summary.contains("selection passed"), "{summary}");
    assert!(!summary.contains("all tests passed"), "{summary}");
}

#[test]
fn work_check_phase_summary_bounds_invocations_and_custom_fallbacks() {
    let mut value = rust_phase_report(serde_json::Value::Null);
    let invocation = value["selected_invocations"][0].clone();
    value["selected_invocations"] = json!(vec![invocation; 10]);
    value["rust_focus_fallbacks"] = json!(
        (0..11)
            .map(|index| json!({
                "target": {"component": "example", "action": format!("custom-{index}")},
                "reason": "unsupported_runner", "scope": "configured_default"
            }))
            .collect::<Vec<_>>()
    );
    let summary = format_work_check_summary(&value);
    assert!(summary.contains("Selected invocations: 10"), "{summary}");
    assert_eq!(summary.matches("example:focused-test:").count(), 8);
    assert!(summary.contains("2 more invocations"), "{summary}");
    assert!(summary.contains("3 more Rust focus fallbacks"), "{summary}");
    assert!(!summary.contains("example:custom-8"), "{summary}");
}
