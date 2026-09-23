#![cfg(unix)]

#[path = "ready_dependents/capacity.rs"]
mod capacity;
#[path = "cargo_resources/support.rs"]
mod fixture;
#[path = "ready_dependents/safety.rs"]
mod safety;
#[path = "ready_dependents/support.rs"]
mod support;

use support::*;

#[test]
fn validated_prerequisite_releases_dependent_while_sibling_is_held() {
    assert_ready_dependent(fixture());
}

#[test]
fn cargo_sibling_does_not_hold_ordinary_dependency_chain() {
    assert_ready_dependent(resource_fixture());
}

fn assert_ready_dependent(fixture: fixture::Fixture) {
    let plan = open_plan(&fixture);
    let mut run = start(&fixture, &["--plan-id", &plan]);
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    assert!(run.running());
    assert!(!fixture.signals.join("release-slow").exists());
    assert!(!fixture.signals.join("completed-slow").exists());
    let first = records(&fixture, "receipts.jsonl");
    assert_eq!(
        first
            .iter()
            .filter(|receipt| receipt["target"].is_object())
            .count(),
        1,
        "dependent must start while slow is still running: {first:#?}"
    );
    let prerequisite = receipt(&first, "prerequisite");
    assert_eq!(prerequisite["exit_status"], 0);
    assert_eq!(prerequisite["target_freshness"]["state"], "complete");
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    let published = records(&fixture, "receipts.jsonl");
    let dependent = receipt(&published, "dependent");
    assert_eq!(dependent["exit_status"], 0);
    let proofs = dependent["target_freshness"]["dependency_execution_proof"]
        .as_array()
        .unwrap();
    assert_eq!(proofs.len(), 1);
    assert_eq!(proofs[0]["receipt_id"], prerequisite["id"]);
    assert_eq!(proofs[0]["run_id"], prerequisite["run_id"]);
    assert_eq!(proofs[0]["plan_id"], prerequisite["plan_id"]);
    assert_eq!(
        proofs[0]["identity_digest"],
        prerequisite["target_freshness"]["identity"]["identity_digest"]
    );
    assert_eq!(proofs[0]["conclusion"], "success");
    assert_eq!(receipt(&published, "prerequisite"), prerequisite);
    let events = records(&fixture, "runs.jsonl");
    let at = |event: &str, action: &str| {
        events
            .iter()
            .position(|r| r["event"] == event && r["target"]["action"] == action)
            .unwrap()
    };
    assert!(at("target_completed", "prerequisite") < at("target_started", "dependent"));
    assert!(at("target_started", "dependent") < at("target_completed", "dependent"));
    assert!(
        !events
            .iter()
            .any(|r| r["event"] == "target_completed" && r["target"]["action"] == "slow")
    );
    release(&fixture, "slow");
    run.finish_success();
    assert_eq!(
        receipt(&records(&fixture, "receipts.jsonl"), "slow")["exit_status"],
        0
    );
}
