#[test]
fn work_gates_summary_identifies_retired_plan() {
    let summary = format_work_gates_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "plan_retirement": {
            "disposition": "superseded",
            "reason": "Replaced by the ExampleProject redesign.",
            "superseded_by": "plan_2"
        },
        "overall": "blocked",
        "gates": [],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (retired: superseded)"));
    assert!(summary.contains("Retirement reason: Replaced by the ExampleProject redesign."));
    assert!(summary.contains("Superseded by: plan_2"));
    assert!(!summary.contains("Plan: plan_1 (closed)"));
}

#[test]
fn work_evidence_summary_identifies_retired_plan() {
    let summary = format_work_evidence_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "plan_retirement": {
            "disposition": "obsolete",
            "reason": "The approach no longer applies."
        },
        "overall": "blocked",
        "latest_passing_gates": [],
        "gates": [],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (retired: obsolete)"));
    assert!(summary.contains("Retirement reason: The approach no longer applies."));
    assert!(!summary.contains("Plan: plan_1 (closed)"));
}

#[test]
fn work_receipts_summary_distinguishes_retirement_from_completion() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.plans_close",
            "args": {"operation": "plan_retire", "disposition": "cancelled"},
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": "plan_1",
            "session_id": "session_1",
            "stdout_preview": "",
            "stderr_preview": ""
        }]
    }));

    assert!(summary.contains("jig.plans_close [plan_retire] (receipt_1)"));
}

#[test]
fn work_finish_summary_explains_when_an_unrelated_session_was_left_active() {
    let summary = format_work_finish_summary(&json!({
        "ok": true,
        "plan": {"plan_id": "plan_1"},
        "session": null,
        "session_status": {
            "action": "left_active",
            "detail": "left current session session_2 active; plan plan_1 is owned by session session_1"
        }
    }));

    assert!(summary.contains("Work finish: closed"));
    assert!(summary.contains("left current session session_2 active"));
}
