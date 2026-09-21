use super::{fixture::*, wave_fixture::*};
use serde_json::Value;
use std::collections::BTreeSet;

fn assert_published_before_sibling(fixture: &Fixture, first: &str, second: &str, failed: bool) {
    let receipts = records(&fixture.root.join(".agent/state/receipts.jsonl"));
    let first_receipt = receipt_for(&receipts, first);
    assert_eq!(first_receipt["exit_status"], if failed { 7 } else { 0 });
    let events = records(&fixture.root.join(".agent/state/runs.jsonl"));
    assert!(
        events
            .iter()
            .any(|event| event["event"] == "target_completed"
                && event["target"]["action"] == first
                && event["result"]["receipt_id"] == first_receipt["id"]),
        "first sibling must publish its receipt and result before second admission: {events:#?}"
    );
    assert!(
        !receipts
            .iter()
            .any(|receipt| receipt["target"]["action"] == second)
    );
}

fn exercise_mixed_layer(fail_first: bool) {
    let fixture = wave_fixture(WaveKind::Mixed);
    assert_independent_layer(&fixture, 4);
    let mut run = fixture.spawn_args("mixed", &["check", "--profile", "verify"]);
    let mut pending = BTreeSet::from(["ordinary-a", "ordinary-b", "cargo-a", "cargo-b"]);
    let mut first_cargo = None;
    while !pending.is_empty() {
        let entered = run.wait_any_entry(&pending.iter().copied().collect::<Vec<_>>());
        if entered.starts_with("ordinary") {
            run.wait_named_entry("ordinary-a");
            run.wait_named_entry("ordinary-b");
            for id in ["ordinary-a", "ordinary-b"] {
                release(&fixture, id);
                pending.remove(id);
            }
        } else {
            release_cargo(&fixture, &entered, &mut first_cargo, fail_first);
            pending.remove(entered.as_str());
        }
    }
    if fail_first {
        run.finish_failure();
    } else {
        run.finish_success();
    }
    assert_mixed_results(&fixture, first_cargo.as_deref().unwrap(), fail_first);
}

fn release_cargo(
    fixture: &Fixture,
    entered: &str,
    first_cargo: &mut Option<String>,
    fail_first: bool,
) {
    if let Some(first) = first_cargo.as_deref() {
        assert_published_before_sibling(fixture, first, entered, fail_first);
    } else {
        let other = if entered == "cargo-a" {
            "cargo-b"
        } else {
            "cargo-a"
        };
        assert!(!fixture.signals.join(format!("entered-{other}")).exists());
        if fail_first {
            signal(fixture, &format!("fail-{entered}"));
        }
        *first_cargo = Some(entered.to_owned());
    }
    release(fixture, entered);
}

fn assert_mixed_results(fixture: &Fixture, first_cargo: &str, failed: bool) {
    let receipts = records(&fixture.root.join(".agent/state/receipts.jsonl"));
    for action in ["ordinary-a", "ordinary-b", "cargo-a", "cargo-b"] {
        let expected = if failed && action == first_cargo {
            7
        } else {
            0
        };
        assert_eq!(
            receipt_for(&receipts, action)["exit_status"],
            expected,
            "a failed Cargo sibling must not propagate through an invented dependency"
        );
    }
    assert!(!fixture.signals.join("overlap").exists());
    let launches = fixture.launches();
    let launched = launches.lines().collect::<BTreeSet<_>>();
    assert_eq!(launches.lines().count(), 4);
    assert_eq!(
        launched,
        BTreeSet::from(["ordinary-a", "ordinary-b", "cargo-a", "cargo-b"])
    );
}

pub(super) fn assert_independent_layer(fixture: &Fixture, count: usize) {
    let preview = jig(&fixture.root)
        .args(["check", "--profile", "verify", "--explain", "--json"])
        .output()
        .unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    let layers = preview["plan"]["execution_layers"].as_array().unwrap();
    assert_eq!(layers.len(), 1, "{preview:#}");
    assert_eq!(layers[0].as_array().unwrap().len(), count, "{preview:#}");
    assert!(
        preview["plan"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|target| target["depends_on"].is_null()),
        "resource coordination must not add graph edges: {preview:#}"
    );
}

#[test]
fn mixed_layer_keeps_ordinary_parallelism_and_publishes_cargo_siblings_independently() {
    exercise_mixed_layer(false);
    exercise_mixed_layer(true);
}
