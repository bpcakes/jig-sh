#![cfg(unix)]

#[path = "ready_dependents/cancellation.rs"]
mod cancellation;
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
    let mut run = start(&fixture);
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    assert!(run.running());
    assert!(!fixture.signals.join("release-slow").exists());
    assert!(!fixture.signals.join("completed-slow").exists());
    let first = records(&fixture, "runs.jsonl");
    assert_eq!(
        first
            .iter()
            .filter(|event| event["event"] == "target_completed")
            .count(),
        1,
        "dependent must start while slow is still running: {first:#?}"
    );
    let prerequisite = result(&first, "prerequisite").clone();
    assert_eq!(prerequisite["conclusion"], "success");
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    let events = records(&fixture, "runs.jsonl");
    assert_eq!(result(&events, "dependent")["conclusion"], "success");
    assert_eq!(result(&events, "prerequisite"), &prerequisite);
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
        result(&records(&fixture, "runs.jsonl"), "slow")["conclusion"],
        "success"
    );
}
