use std::fs;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::super::{dispatch_due_at, dispatch_receipt_action};
use crate::context::RepoContext;
use crate::test_env::TestRepoBuilder;

#[test]
fn dispatch_receipt_action_references_the_tick_without_copying_observation() {
    let action = json!({
        "workflow_id": "ExampleProject",
        "status": "succeeded",
        "tick": {
            "receipt_id": "receipt-tick",
            "status": "acted",
            "workflow": {"id": "ExampleProject"},
            "item_key": "pr-17",
            "observed": {"body": "large observation"},
            "actions": [{"worker_receipt_id": "receipt-worker"}],
        },
    });

    let receipt_action = dispatch_receipt_action(&action);

    assert_eq!(
        receipt_action["tick"]["kind"],
        "loop_tick_receipt_reference"
    );
    assert_eq!(receipt_action["tick"]["receipt_id"], "receipt-tick");
    assert_eq!(receipt_action["tick"]["status"], "acted");
    assert_eq!(receipt_action["tick"]["workflow_id"], "ExampleProject");
    assert_eq!(receipt_action["tick"]["item_key"], "pr-17");
    assert!(receipt_action["tick"].get("observed").is_none());
    assert_eq!(action["tick"]["observed"]["body"], "large observation");
}

#[test]
fn persisted_dispatch_receipt_references_the_detailed_tick_receipt() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "scheduled-noop"
kind = "noop_status"
schedule = "* * * * *"
timezone = "UTC"
"#,
        ),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch_due_at(&ctx, super::timestamp("2026-08-21T08:42:30Z")).unwrap();

    assert_eq!(output["actions"][0]["tick"]["command"], "loop tick");
    let receipt = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|receipt| receipt["id"] == output["receipt_id"])
        .unwrap();
    let tick = &receipt["evidence"]["actions"][0]["tick"];
    assert_eq!(tick["kind"], "loop_tick_receipt_reference");
    assert_eq!(
        tick["receipt_id"],
        output["actions"][0]["tick"]["receipt_id"]
    );
    assert!(tick.get("observed").is_none());
}
