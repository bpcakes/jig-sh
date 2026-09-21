#![cfg(unix)]

#[path = "cargo_resources/support.rs"]
mod cargo_fixture;
#[path = "playwright_resources/support.rs"]
mod fixture;

use cargo_fixture::jig;
use fixture::{BrowserFixture, records};
use serde_json::{Value, json};
use std::fs;

#[test]
fn same_partial_and_swapped_endpoint_pairs_conflict_across_repositories() {
    for pair in [[0, 1], [1, 2], [1, 0]] {
        let fixture = BrowserFixture::new();
        let other = fixture.other("example-browser-peer", fixture.action("test", pair, " \t "));
        let mut owner = fixture.inner.spawn("owner", &[]);
        owner.wait_entered();
        let mut waiter = fixture.inner.spawn_in(&other, "waiter", &[]);
        fixture.wait_notice(&mut waiter, "waiter");
        assert!(!waiter.entered());
        owner.release();
        owner.finish_success();
        waiter.wait_entered();
        waiter.release();
        waiter.finish_success();
        assert_eq!(fixture.inner.launches(), "owner\nwaiter\n");
        fixture.assert_no_overlap_or_cargo();
    }
}

#[test]
fn distinct_endpoint_pairs_overlap_across_requests() {
    let fixture = BrowserFixture::new();
    let other = fixture.other(
        "example-independent-browser",
        fixture.action("test", [2, 3], ""),
    );
    let mut first = fixture.inner.spawn("first", &[]);
    first.wait_entered();
    let mut second = fixture.inner.spawn_in(&other, "second", &[]);
    second.wait_entered();
    assert!(
        first.running(),
        "independent owner must still hold its barrier"
    );
    assert!(second.running());
    first.release();
    second.release();
    first.finish_success();
    second.finish_success();
    assert_eq!(fixture.inner.launches(), "first\nsecond\n");
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn javascript_numeric_spellings_resolve_to_the_same_endpoints() {
    let fixture = BrowserFixture::new();
    let mut action = fixture.action("test", [0, 1], "");
    action["runner"]["environment"]["E2E_WEB_PORT"] =
        json!(format!(" \t0x{:x}\u{feff}", fixture.port(0)));
    action["runner"]["environment"]["E2E_API_PORT"] = json!(format!(" {}.0e0 ", fixture.port(1)));
    let other = fixture.other("example-numeric-browser", action);
    let mut owner = fixture.inner.spawn("owner", &[]);
    owner.wait_entered();
    let mut waiter = fixture.inner.spawn_in(&other, "numeric", &[]);
    fixture.wait_notice(&mut waiter, "numeric");
    assert!(!waiter.entered());
    owner.release();
    owner.finish_success();
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
    assert_eq!(fixture.inner.launches(), "owner\nnumeric\n");
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn external_url_owns_no_endpoints_while_local_owner_is_active() {
    let fixture = BrowserFixture::new();
    let other = fixture.other(
        "example-external-browser",
        fixture.action(
            "test",
            [0, 1],
            " \t https://example.invalid/private-fixture \n ",
        ),
    );
    let mut owner = fixture.inner.spawn("owner", &[]);
    owner.wait_entered();
    let mut external = fixture.inner.spawn_in(&other, "external", &[]);
    external.wait_entered();
    assert!(owner.running());
    external.release();
    external.finish_success();
    assert!(owner.running());
    owner.release();
    owner.finish_success();
    let output = external.output();
    assert!(!output.contains("private-fixture"));
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn one_layer_overlaps_distinct_pairs_and_publishes_before_conflicting_sibling() {
    let fixture = BrowserFixture::new();
    let actions = [("first", [0, 1]), ("independent", [2, 3]), ("last", [1, 0])]
        .into_iter()
        .map(|(name, pair)| {
            let mut action = fixture.action(name, pair, "");
            action["runner"]["environment"]["EXAMPLE_RUN_ID"] = json!(name);
            action
        })
        .collect::<Vec<_>>();
    fixture.configure(&fixture.inner.root, &actions);
    let preview = jig(&fixture.inner.root)
        .args(["check", "--profile", "verify", "--explain", "--json"])
        .output()
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(
        preview["plan"]["execution_layers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        preview["plan"]["execution_layers"][0]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        preview["plan"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|target| target["depends_on"].is_null())
    );
    let mut run = fixture
        .inner
        .spawn_args("wave", &["check", "--profile", "verify"]);
    let first = run.wait_any_entry(&["first", "last"]);
    let next = if first == "first" { "last" } else { "first" };
    run.wait_named_entry("independent");
    assert!(
        !fixture
            .inner
            .signals
            .join(format!("entered-{next}"))
            .exists()
    );
    fixture.release("independent");
    fixture.release(&first);
    run.wait_named_entry(next);
    let events = records(&fixture.inner.root, "runs.jsonl");
    assert!(
        events
            .iter()
            .any(|event| event["event"] == "target_completed"
                && event["target"]["action"] == first
                && event["result"]["receipt_id"].is_string())
    );
    fixture.release(next);
    run.finish_success();
    assert_eq!(fixture.inner.launches().lines().count(), 3);
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn invalid_ports_prevent_target_launch_even_in_external_mode() {
    for (web, api) in [
        ("0", "4174"),
        ("65536", "4174"),
        ("4.5", "4174"),
        ("NaN", "4174"),
        ("Infinity", "4174"),
        ("4174", "4174"),
    ] {
        let fixture = BrowserFixture::new();
        let mut action = fixture.action("test", [0, 1], "https://example.invalid");
        action["runner"]["environment"]["E2E_WEB_PORT"] = json!(web);
        action["runner"]["environment"]["E2E_API_PORT"] = json!(api);
        fixture.configure(&fixture.inner.root, &[action]);
        let mut run = fixture.inner.spawn("invalid", &[]);
        run.finish_failure();
        assert_eq!(
            fixture.inner.launches(),
            "",
            "invalid ports reached the target"
        );
        assert!(!run.entered());
        run.assert_single_completion("blocked");
        fixture.assert_no_overlap_or_cargo();
    }
}

#[test]
fn canceled_waiter_starts_nothing_and_does_not_unlock_owner() {
    let fixture = BrowserFixture::new();
    let mut owner = fixture.inner.spawn("owner", &[]);
    owner.wait_entered();
    let mut canceled = fixture.inner.spawn("canceled", &[]);
    fixture.wait_notice(&mut canceled, "canceled");
    canceled.cancel();
    canceled.finish_failure();
    assert!(!canceled.entered());
    let mut next = fixture.inner.spawn("next", &[]);
    fixture.wait_notice(&mut next, "next");
    assert!(!next.entered());
    assert!(owner.running());
    owner.release();
    owner.finish_success();
    next.wait_entered();
    next.release();
    next.finish_success();
    assert_eq!(fixture.inner.launches(), "owner\nnext\n");
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn source_change_while_waiting_rejects_browser_child() {
    let fixture = BrowserFixture::new();
    let other = fixture.other("example-browser-source", fixture.action("test", [0, 1], ""));
    let mut owner = fixture.inner.spawn("owner", &[]);
    owner.wait_entered();
    let mut waiter = fixture.inner.spawn_in(&other, "changed", &[]);
    fixture.wait_notice(&mut waiter, "changed");
    fs::write(other.join("src/lib.rs"), "pub fn changed_example() {}\n").unwrap();
    owner.release();
    owner.finish_success();
    waiter.finish_failure();
    assert!(!waiter.entered());
    waiter.assert_single_completion("blocked");
    assert_eq!(fixture.inner.launches(), "owner\n");
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn browser_admission_timeout_starts_nothing_and_retains_owner() {
    let fixture = BrowserFixture::new();
    let mut action = fixture.action("test", [0, 1], "");
    action["timeout_seconds"] = json!(8);
    let other = fixture.other("example-browser-timeout", action);
    let mut owner = fixture.inner.spawn("owner", &[]);
    owner.wait_entered();
    let mut waiter = fixture.inner.spawn_in(&other, "timeout", &[]);
    fixture.wait_notice(&mut waiter, "timeout");
    waiter.finish_failure();
    assert!(!waiter.entered());
    waiter.assert_single_completion("timed_out");
    assert!(owner.running());
    owner.release();
    owner.finish_success();
    assert_eq!(fixture.inner.launches(), "owner\n");
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn browser_work_waiter_executes_live_validator_after_passing_receipt_publication() {
    let fixture = BrowserFixture::new();
    let plan = fixture.open_plan();
    let mut owner = fixture.inner.spawn("publisher", &["--plan-id", &plan]);
    owner.wait_entered();
    let mut waiter = fixture
        .inner
        .spawn_args("waiter", &["work", "check", "--plan-id", &plan]);
    fixture.wait_notice(&mut waiter, "waiter");
    assert!(!waiter.entered());
    owner.release();
    owner.finish_success();
    waiter.wait_entered();
    waiter.release();
    waiter.finish_success();
    assert_eq!(fixture.inner.launches(), "publisher\nwaiter\n");
    let receipts = records(&fixture.inner.root, "receipts.jsonl");
    let receipts = receipts
        .iter()
        .filter(|receipt| receipt["tool_name"] == "jig.target_run")
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 2);
    assert_ne!(receipts[0]["id"], receipts[1]["id"]);
    assert_ne!(receipts[0]["run_id"], receipts[1]["run_id"]);
    let events = records(&fixture.inner.root, "runs.jsonl");
    assert!(
        events
            .iter()
            .all(|event| event["result"]["reused_from"].is_null())
    );
    fixture.assert_no_overlap_or_cargo();
}

#[test]
fn current_wrapper_readiness_is_checked_after_wait_and_repair_runs_validator() {
    // This is a generic owning-wrapper guard oracle. Actual SQLx/database
    // behavior remains bounded by the recorded T-03 evidence, not this fixture.
    let fixture = BrowserFixture::new();
    let plan = fixture.open_plan();
    let mut owner = fixture.inner.spawn("publisher", &["--plan-id", &plan]);
    owner.wait_entered();
    let mut waiter = fixture
        .inner
        .spawn_args("waiter", &["work", "check", "--plan-id", &plan]);
    fixture.wait_notice(&mut waiter, "waiter");
    assert!(!waiter.entered());

    let denied = fixture.inner.signals.join("readiness-denied");
    fs::write(&denied, "current prerequisite is unavailable\n").unwrap();
    owner.release();
    owner.finish_success();
    waiter.finish_failure();
    assert!(
        !waiter.entered(),
        "failed readiness must prevent validator entry"
    );
    assert_eq!(fixture.inner.launches(), "publisher\nwaiter\n");
    let expensive = fixture.inner.signals.join("expensive-launches");
    assert_eq!(fs::read_to_string(&expensive).unwrap(), "publisher\n");

    fs::remove_file(denied).unwrap();
    let mut repaired = fixture.inner.spawn("repaired", &["--plan-id", &plan]);
    repaired.wait_entered();
    repaired.release();
    repaired.finish_success();
    assert_eq!(fixture.inner.launches(), "publisher\nwaiter\nrepaired\n");
    assert_eq!(
        fs::read_to_string(expensive).unwrap(),
        "publisher\nrepaired\n"
    );
    let receipts = records(&fixture.inner.root, "receipts.jsonl");
    let statuses = receipts
        .iter()
        .filter(|receipt| receipt["tool_name"] == "jig.target_run")
        .map(|receipt| receipt["exit_status"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(statuses, [0, 42, 0]);
    let events = records(&fixture.inner.root, "runs.jsonl");
    assert!(
        events
            .iter()
            .all(|event| event["result"]["reused_from"].is_null())
    );
    fixture.assert_no_overlap_or_cargo();
}
