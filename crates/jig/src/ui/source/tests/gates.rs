use std::collections::{BTreeMap, BTreeSet};

use jig_ui::dashboard::{DashboardSource, LimitId, RecorderMode};
use serde_json::json;
use tempfile::tempdir;

use crate::context::RepoContext;
use crate::state::{ReceiptInput, record_receipt, seed_open_plan_for_test};
use crate::test_env::TestRepoBuilder;

use super::{super::RepoDashboardSource, recorder_request};

#[test]
fn review_findings_and_remediation_are_preserved_and_bounded() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .config(
            r#"
[[work.gates]]
id = "rust-review"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
"#,
        )
        .write();
    let context = RepoContext::load_from(root.path()).unwrap();
    seed_open_plan_for_test(&context, "plan_example", "Example plan", "# Example plan\n").unwrap();
    let findings = (0..=LimitId::GateFindings.ceiling())
        .map(|index| {
            json!({
                "fingerprint": format!("finding-{index}"),
                "severity": "warning",
                "path": "src/example.rs",
                "line": index + 1,
                "issue": format!("Issue {index}"),
                "evidence": "evidence",
                "recommendation": "recommendation"
            })
        })
        .collect::<Vec<_>>();
    record_receipt(
        &context,
        ReceiptInput {
            tool_name: crate::tool_defs::tool::WORK_REVIEW,
            args: json!({"gate_id": "rust-review"}),
            invoked_command_key: None,
            plan_id: Some("plan_example".to_string()),
            started_at_ms: 10,
            ended_at_ms: 20,
            exit_status: 1,
            stdout: "",
            stderr: "",
            evidence: Some(json!({
                "status": "failed",
                "raw_finding_count": findings.len(),
                "raw_actionable_count": findings.len(),
                "findings_truncated": false,
                "actionable_findings_truncated": false,
                "threshold": "warning",
                "findings": findings,
                "actionable_findings": []
            })),
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();

    let raw = std::fs::read_to_string(context.state_file("receipts.jsonl")).unwrap();
    let receipt: crate::state::DashboardReceiptRecord = serde_json::from_str(raw.trim()).unwrap();
    assert_eq!(receipt.tool_name, crate::tool_defs::tool::WORK_REVIEW);
    assert_eq!(receipt.plan_id.as_deref(), Some("plan_example"));
    assert_eq!(receipt.args["gate_id"], "rust-review");

    let index = crate::state::work_gate_receipt_index(
        &context,
        "plan_example",
        &BTreeSet::new(),
        &BTreeSet::from(["rust-review".to_string()]),
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(index.review_receipt("rust-review").is_some(), "{}", raw);

    let source = RepoDashboardSource::new(context);
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let gate = &refresh.recorder.open_plans[0]
        .gates
        .as_ref()
        .unwrap()
        .gates
        .items()[0];
    assert_eq!(
        gate.findings.items().len(),
        LimitId::GateFindings.ceiling(),
        "{gate:#?}"
    );
    assert_eq!(gate.findings.omitted(), Some(1));
    assert_eq!(gate.findings.items()[0].code, "finding-0");
    assert_eq!(gate.findings.items()[0].message, "Issue 0");
    assert_eq!(
        gate.remediation.as_ref().unwrap().argv,
        [
            "scripts/jig",
            "work",
            "review",
            "--plan-id",
            "plan_example",
            "--gate",
            "rust-review"
        ]
    );
}

#[test]
fn externally_truncated_review_findings_preserve_both_gate_projections() {
    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .config(
            r#"
[[work.gates]]
id = "rust-review"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
"#,
        )
        .write();
    let context = RepoContext::load_from(root.path()).unwrap();
    seed_open_plan_for_test(&context, "plan_example", "Example plan", "# Example plan\n").unwrap();
    let mut findings = (0..10)
        .map(|index| {
            json!({
                "fingerprint": format!("finding-{index}"),
                "issue": format!("Issue {index}")
            })
        })
        .collect::<Vec<_>>();
    findings.push(json!({"severity": "warning"}));
    record_receipt(
        &context,
        ReceiptInput {
            tool_name: crate::tool_defs::tool::WORK_REVIEW,
            args: json!({"gate_id": "rust-review"}),
            invoked_command_key: None,
            plan_id: Some("plan_example".to_string()),
            started_at_ms: 10,
            ended_at_ms: 20,
            exit_status: 1,
            stdout: "",
            stderr: "",
            evidence: Some(json!({
                "status": "failed",
                "raw_finding_count": 50,
                "raw_actionable_count": 50,
                "findings_truncated": true,
                "actionable_findings_truncated": true,
                "threshold": "warning",
                "findings": findings,
                "actionable_findings": []
            })),
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();

    let source = RepoDashboardSource::new(context);
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let status_gate = &refresh.status_local.work.gates[0];
    assert!(status_gate.snapshot.is_some());
    assert!(status_gate.error.is_none());
    let jig_ui::dashboard::StatusGate::CodexReview(status_review) =
        &status_gate.snapshot.as_ref().unwrap().gates[0]
    else {
        panic!("expected a review gate");
    };
    assert!(status_review.parse_error.is_none());
    let findings = &refresh.recorder.open_plans[0]
        .gates
        .as_ref()
        .unwrap()
        .gates
        .items()[0]
        .findings;
    assert_eq!(findings.items().len(), 10);
    assert_eq!(findings.applied(), LimitId::GateFindings.ceiling());
    assert_eq!(findings.omitted(), None);
}
