use std::fs;

use jig_ui::dashboard::{
    CollectionDomain, DashboardSource, LimitId, PlanBasis, PlanSnapshotResult, RecorderMode,
};
use serde_json::json;

use crate::state::{ReceiptInput, record_receipt};

use super::{recorder_request, source_fixture};

#[test]
fn open_and_closed_plan_details_use_the_epoch_basis_and_bounded_single_pass_reads() {
    let (root, source) = source_fixture();
    let decisions = (0..=LimitId::PlanDecisions.ceiling())
        .map(|index| {
            format!(
                "{}\n",
                json!({
                    "id": format!("decision-{index:03}"),
                    "session_id": null,
                    "plan_id": "plan_example",
                    "timestamp_ms": index,
                    "title": format!("Decision {index}"),
                    "selected_option": "A",
                    "alternatives": [],
                    "rationale": "r".repeat(LimitId::TimelineDecisionRationaleChars.ceiling() + 3)
                })
            )
        })
        .collect::<String>();
    fs::write(root.path().join(".agent/state/decisions.jsonl"), decisions).unwrap();
    for index in 0..LimitId::PlanReceipts.ceiling() {
        record_receipt(
            &source.context,
            ReceiptInput {
                tool_name: "jig.example",
                args: json!({}),
                invoked_command_key: Some("example".to_string()),
                plan_id: Some("plan_example".to_string()),
                started_at_ms: 100 + index as u64,
                ended_at_ms: 200 + index as u64,
                exit_status: 0,
                stdout: &"o".repeat(LimitId::ReceiptStdoutChars.ceiling() + 2),
                stderr: &"e".repeat(LimitId::ReceiptStderrChars.ceiling() + 4),
                evidence: None,
                session_override: None,
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();
    }

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    crate::state::reset_dashboard_scan_counts();
    let PlanSnapshotResult::Found(open) = source
        .plan(
            PlanBasis::RecorderEpoch(refresh.recorder.epoch_id),
            "plan_example".to_string(),
            &|| false,
        )
        .unwrap()
    else {
        panic!("open plan should be found");
    };
    assert_eq!(open.basis_epoch, refresh.recorder.epoch_id);
    let encoded = serde_json::to_value(&open).unwrap();
    let _: jig_ui::dashboard::PlanSnapshot = serde_json::from_value(encoded).unwrap();
    assert_eq!(open.gates_observed_at_ms, refresh.recorder.generated_at_ms);
    assert_eq!(open.decisions.len(), LimitId::PlanDecisions.ceiling());
    assert_eq!(open.limits.plan_decisions.omitted, Some(1));
    assert_eq!(open.receipts.len(), LimitId::PlanReceipts.ceiling());
    assert_eq!(open.limits.plan_receipts.omitted, None);
    assert_eq!(
        open.decisions[0].rationale.text().chars().count(),
        LimitId::TimelineDecisionRationaleChars.ceiling()
    );
    assert_eq!(open.decisions[0].rationale.omitted_chars(), Some(3));
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("plans.jsonl")),
        0
    );
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("decisions.jsonl")),
        1
    );

    let plan_path = root.path().join(".agent/state/plans.jsonl");
    let mut plan_events = fs::read_to_string(&plan_path).unwrap();
    plan_events.push_str(&format!(
        "{}\n",
        json!({
            "id": "plan-close-example",
            "plan_id": "plan_example",
            "event": "close",
            "timestamp_ms": 1_000,
            "resolution": "complete"
        })
    ));
    fs::write(plan_path, plan_events).unwrap();
    let closed_epoch = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;
    crate::state::reset_dashboard_scan_counts();
    let PlanSnapshotResult::Found(closed) = source
        .plan(
            PlanBasis::RecorderEpoch(closed_epoch),
            "plan_example".to_string(),
            &|| false,
        )
        .unwrap()
    else {
        panic!("closed plan should be found");
    };
    assert_eq!(closed.limits.plan_receipts.omitted, Some(1));
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("receipts.jsonl")),
        1
    );
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("plans.jsonl")),
        0
    );
}

#[test]
fn corrupt_receipts_leave_open_and_closed_plan_metadata_available() {
    let (root, source) = source_fixture();
    let open_epoch = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;
    let receipts_path = root.path().join(".agent/state/receipts.jsonl");
    let valid_receipts = fs::read_to_string(&receipts_path).unwrap();
    fs::write(&receipts_path, format!("{valid_receipts}not-json\n")).unwrap();

    let PlanSnapshotResult::Found(open) = source
        .plan(
            PlanBasis::RecorderEpoch(open_epoch),
            "plan_example".to_string(),
            &|| false,
        )
        .unwrap()
    else {
        panic!("open plan should remain available");
    };
    assert!(open.body.is_some());
    assert!(
        open.gates.is_some(),
        "retained epoch gates remain trustworthy"
    );
    assert!(open.receipts.is_empty());
    assert!(open.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Receipts.as_str()
            && error.code() == "record_decode_failed"
    }));

    let plans_path = root.path().join(".agent/state/plans.jsonl");
    let mut plans = fs::read_to_string(&plans_path).unwrap();
    plans.push_str(&format!(
        "{}\n",
        json!({
            "id": "plan-close-corrupt-receipts",
            "plan_id": "plan_example",
            "event": "close",
            "timestamp_ms": 1_000,
            "resolution": "complete"
        })
    ));
    fs::write(plans_path, plans).unwrap();
    fs::write(&receipts_path, &valid_receipts).unwrap();
    let closed_epoch = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;
    fs::write(&receipts_path, format!("{valid_receipts}not-json\n")).unwrap();

    let PlanSnapshotResult::Found(closed) = source
        .plan(
            PlanBasis::RecorderEpoch(closed_epoch),
            "plan_example".to_string(),
            &|| false,
        )
        .unwrap()
    else {
        panic!("closed plan should remain available");
    };
    assert!(closed.body.is_some());
    assert!(closed.receipts.is_empty());
    assert!(closed.gates.is_none());
    assert!(closed.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Receipts.as_str()
            && error.code() == "record_decode_failed"
    }));
    assert!(closed.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Gates.as_str()
            && error.code() == "gate_observation_failed"
    }));
}

#[test]
fn retained_and_fresh_plan_receipts_share_append_order() {
    let (_root, source) = source_fixture();
    let newer_timestamp_id = record_receipt(
        &source.context,
        ReceiptInput {
            tool_name: "jig.example",
            args: json!({}),
            invoked_command_key: Some("example".to_string()),
            plan_id: Some("plan_example".to_string()),
            started_at_ms: 90,
            ended_at_ms: 100,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();
    let appended_last_id = record_receipt(
        &source.context,
        ReceiptInput {
            tool_name: "jig.example",
            args: json!({}),
            invoked_command_key: Some("example".to_string()),
            plan_id: Some("plan_example".to_string()),
            started_at_ms: 4,
            ended_at_ms: 5,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();
    let epoch = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;
    let PlanSnapshotResult::Found(retained) = source
        .plan(
            PlanBasis::RecorderEpoch(epoch),
            "plan_example".to_string(),
            &|| false,
        )
        .unwrap()
    else {
        panic!("retained plan should exist");
    };
    let PlanSnapshotResult::Found(fresh) = source
        .plan(PlanBasis::Fresh, "plan_example".to_string(), &|| false)
        .unwrap()
    else {
        panic!("fresh plan should exist");
    };
    let retained_ids = retained
        .receipts
        .iter()
        .map(|receipt| receipt.id.as_str())
        .collect::<Vec<_>>();
    let fresh_ids = fresh
        .receipts
        .iter()
        .map(|receipt| receipt.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(fresh_ids, retained_ids);
    assert_eq!(retained_ids[0], appended_last_id);
    assert_eq!(retained_ids[1], newer_timestamp_id);
}
