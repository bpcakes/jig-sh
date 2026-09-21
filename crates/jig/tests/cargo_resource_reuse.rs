#![cfg(unix)]

#[path = "cargo_resources/support.rs"]
mod fixture;

use fixture::{Fixture, jig};
use serde_json::Value;
use std::{fs, path::Path};

fn open_plan(fixture: &Fixture) -> String {
    let output = jig(&fixture.root)
        .args([
            "work",
            "start",
            "--title",
            "Example resource reuse",
            "--body",
            "Validate original evidence after a coordinated wait.",
            "--print-plan-id",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn records(root: &Path, file: &str) -> Vec<Value> {
    fs::read_to_string(root.join(".agent/state").join(file))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn target_receipts(root: &Path) -> Vec<Value> {
    records(root, "receipts.jsonl")
        .into_iter()
        .filter(|receipt| receipt["tool_name"] == "jig.target_run")
        .collect()
}

#[test]
fn ordinary_work_waiter_reuses_published_original_evidence_without_target_start() {
    let fixture = Fixture::new(true, 60);
    let plan = open_plan(&fixture);
    let mut owner = fixture.spawn("publisher", &["--plan-id", &plan]);
    owner.wait_entered();
    assert!(target_receipts(&fixture.root).is_empty());
    let mut waiter = fixture.spawn_args("waiter", &["work", "check", "--plan-id", &plan]);
    waiter.wait_resource_notice();
    assert!(!waiter.entered(), "waiter must not start before admission");
    owner.release();
    owner.finish_success();
    waiter.finish_success();
    assert!(
        !waiter.entered(),
        "verified evidence must avoid a second child"
    );
    assert_eq!(fixture.launches(), "publisher\n");

    let receipts = target_receipts(&fixture.root);
    assert_eq!(
        receipts.len(),
        1,
        "reuse must not fabricate a target receipt: {receipts:#?}"
    );
    let original = &receipts[0];
    assert_eq!(original["plan_id"], plan);
    assert_eq!(original["exit_status"], 0);
    let events = records(&fixture.root, "runs.jsonl");
    let reused: Vec<_> = events
        .iter()
        .filter(|event| event["result"]["reused_from"].is_object())
        .collect();
    assert_eq!(
        reused.len(),
        1,
        "exactly one reused target result: {events:#?}"
    );
    let run = reused[0];
    assert_ne!(run["run_id"], original["run_id"]);
    let result = &run["result"];
    assert_eq!(result["reused_from"]["receipt_id"], original["id"]);
    assert_eq!(result["reused_from"]["run_id"], original["run_id"]);
    assert_eq!(result["reused_from"]["plan_id"], original["plan_id"]);
    assert_eq!(result["receipt_id"], original["id"]);
    assert_eq!(result["conclusion"], "success");
    assert!(result.get("started_at_ms").is_none());
    assert!(result.get("exit_code").is_none());
    assert!(
        !events
            .iter()
            .any(|event| event["run_id"] == run["run_id"] && event["event"] == "target_started")
    );
}

fn assert_forced_waiter_executes(force_gate: bool) {
    let fixture = Fixture::new(true, 60);
    let plan = open_plan(&fixture);
    let mut owner = fixture.spawn("publisher", &["--plan-id", &plan]);
    owner.wait_entered();
    let mut waiter = if force_gate {
        fixture.spawn_args(
            "forced",
            &["work", "check", "--plan-id", &plan, "--gate", "full"],
        )
    } else {
        fixture.spawn("direct", &["--plan-id", &plan])
    };
    waiter.wait_resource_notice();
    assert!(!waiter.entered());
    owner.release();
    owner.finish_success();
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
    let id = if force_gate { "forced" } else { "direct" };
    assert_eq!(fixture.launches(), format!("publisher\n{id}\n"));
    let receipts = target_receipts(&fixture.root);
    assert_eq!(
        receipts.len(),
        2,
        "explicit execution must record its own target evidence"
    );
    assert_ne!(receipts[0]["id"], receipts[1]["id"]);
    assert_ne!(receipts[0]["run_id"], receipts[1]["run_id"]);
    let events = records(&fixture.root, "runs.jsonl");
    assert!(
        !events
            .iter()
            .any(|event| event["result"]["reused_from"].is_object()),
        "forced/direct request reused evidence: {events:#?}"
    );
}

#[test]
fn direct_check_still_executes_after_equivalent_success_is_published() {
    assert_forced_waiter_executes(false);
}

#[test]
fn explicit_work_gate_still_executes_after_equivalent_success_is_published() {
    assert_forced_waiter_executes(true);
}

#[test]
fn newer_failure_during_resource_wait_cannot_resurrect_an_older_success() {
    let fixture = Fixture::new(true, 60);
    let config_path = fixture.root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let command = config["commands"]["example_check_command"]
        .as_str()
        .unwrap();
    config["commands"]["example_check_command"] = toml::Value::String(format!(
        "{command}\ncase \"$EXAMPLE_RUN_ID\" in failure-*) exit 7 ;; esac\n"
    ));
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let plan = open_plan(&fixture);
    // Leave a real older successful receipt in the journal; newer failures
    // must supersede it rather than selecting the last convenient success.
    let mut original = fixture.spawn("original", &["--plan-id", &plan]);
    original.wait_entered();
    original.release();
    original.finish_success();
    let original_id = target_receipts(&fixture.root)[0]["id"].clone();
    let mut first_failure = fixture.spawn("failure-before", &["--plan-id", &plan]);
    first_failure.wait_entered();
    first_failure.release();
    first_failure.finish_failure();

    let mut publisher = fixture.spawn("failure-during", &["--plan-id", &plan]);
    publisher.wait_entered();
    let mut waiter = fixture.spawn_args("necessary", &["work", "check", "--plan-id", &plan]);
    waiter.wait_resource_notice();
    assert!(!waiter.entered());
    publisher.release();
    publisher.finish_failure();
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
    assert_eq!(
        fixture.launches(),
        "original\nfailure-before\nfailure-during\nnecessary\n"
    );
    let receipts = target_receipts(&fixture.root);
    assert_eq!(receipts.len(), 4, "{receipts:#?}");
    assert_eq!(
        receipts
            .iter()
            .map(|receipt| receipt["exit_status"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 7, 7, 0]
    );
    assert_eq!(
        receipts[0]["id"], original_id,
        "the original proof must remain unchanged"
    );
    let events = records(&fixture.root, "runs.jsonl");
    assert!(
        !events
            .iter()
            .any(|event| event["result"]["reused_from"].is_object()),
        "newer failure must not authorize reuse: {events:#?}"
    );
}
