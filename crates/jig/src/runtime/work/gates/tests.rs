use super::{
    CheckGateEvaluation, EvaluatedReceipt, GateEvaluation, GateFreshness, GateOutcome, GateReport,
    RequiredGateFailures, concise_error, latest_passing_gates,
};

fn passing_check(id: &str, receipt_id: &str) -> GateEvaluation {
    GateEvaluation::Check(CheckGateEvaluation {
        id: id.to_string(),
        required: true,
        tool: "jig.test".into(),
        outcome: GateOutcome::Passed,
        receipt: EvaluatedReceipt {
            receipt_id: Some(receipt_id.to_string()),
            freshness_receipt_id: Some(receipt_id.to_string()),
            exit_status: Some(0),
            ended_at_ms: Some(42),
            freshness: GateFreshness::Fresh,
            freshness_reason: "receipt matches current worktree fingerprint".into(),
            changed_paths: Vec::new(),
            changed_path_count: 0,
            changed_paths_truncated: false,
            changed_paths_digest: None,
            diff_summary: None,
            receipt_worktree_fingerprint_error: None,
            current_worktree_fingerprint_error: None,
        },
    })
}

#[test]
fn concise_error_reserves_room_for_ellipsis() {
    let error = "x".repeat(300);
    let concise = concise_error(&error);

    assert_eq!(concise.chars().count(), 240);
    assert!(concise.ends_with("..."));
}

#[test]
fn latest_passing_gates_uses_gate_id_tie_breaker() {
    let report = GateReport {
        plan_id: "plan-test".into(),
        plan_state: "open",
        current_worktree_fingerprint: Some("fingerprint".into()),
        current_worktree_fingerprint_error: None,
        gates: vec![
            passing_check("alpha", "receipt-alpha"),
            passing_check("zeta", "receipt-zeta"),
        ],
        required_failures: RequiredGateFailures::default(),
    };

    let latest = latest_passing_gates(&report);

    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0]["gate_id"], "zeta");
    assert_eq!(latest[0]["receipt_id"], "receipt-zeta");
}

#[test]
fn gate_outcome_uses_closed_freshness_mapping() {
    assert_eq!(
        GateOutcome::Passed
            .with_freshness(GateFreshness::Fresh)
            .as_str(),
        "passed"
    );
    assert_eq!(
        GateOutcome::Passed
            .with_freshness(GateFreshness::Missing)
            .as_str(),
        "missing"
    );
    assert_eq!(
        GateOutcome::Passed
            .with_freshness(GateFreshness::Stale)
            .as_str(),
        "stale"
    );
    assert_eq!(
        GateOutcome::Passed
            .with_freshness(GateFreshness::Unknown)
            .as_str(),
        "unknown"
    );
    assert_eq!(
        GateOutcome::Failed
            .with_freshness(GateFreshness::Unknown)
            .as_str(),
        "failed"
    );
}
