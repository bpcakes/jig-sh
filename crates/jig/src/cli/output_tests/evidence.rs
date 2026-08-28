use serde_json::json;

use super::*;

#[test]
fn work_check_summary_reports_empty_checks() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": []
    }));

    assert!(summary.contains("Work check: no checks configured"));
    assert!(summary.contains("Checks: 0"));
    assert!(summary.contains("configure work checks"));
    assert!(summary.contains("--tool <tool>"));
}

#[test]
fn work_check_summary_reports_component_target_evidence() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": null,
        "checks": [],
        "run": {
            "conclusion": "success",
            "targets": [{
                "target": {"component": "api", "action": "test"},
                "conclusion": "success",
                "receipt_id": "receipt_api"
            }]
        }
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(summary.contains("Checks: 0"));
    assert!(summary.contains("Targets: 1"));
    assert!(summary.contains("api:test: success, receipt receipt_api"));
    assert!(!summary.contains("no checks configured"));
}

#[test]
fn work_evidence_summary_names_profile_evidence_without_unknown_labels() {
    let summary = format_work_evidence_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "open",
        "overall": "passed",
        "latest_passing_gates": [{
            "tool": null,
            "skill": null,
            "profile": "verify",
            "gate_id": "verify",
            "receipt_id": "receipt_web",
            "run_id": "run_1",
            "matches_current_worktree": true,
            "freshness": "fresh",
            "freshness_reason": "all required target receipts match current inputs"
        }],
        "gates": [{
            "id": "verify",
            "kind": "evidence",
            "profile": "verify",
            "status": "passed"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("profile verify: verify, receipt receipt_web"));
    assert!(!summary.contains("<unknown>"));
}
