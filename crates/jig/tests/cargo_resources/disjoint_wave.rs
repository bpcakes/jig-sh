use super::{mixed_layer::assert_independent_layer, wave_fixture::*};

#[test]
fn completed_ordinary_target_keeps_success_while_cargo_peer_outlives_its_timeout() {
    let fixture = wave_fixture(WaveKind::OrdinaryTimeout);
    assert_independent_layer(&fixture, 2);
    release(&fixture, "ordinary-a");
    let mut run = fixture.spawn_args("ordinary-timeout", &["check", "--profile", "verify"]);
    run.wait_named_entry("ordinary-a");
    run.wait_named_entry("cargo-a");
    run.wait_signal("completed-ordinary-a");
    // Intentionally cross the ordinary command's two-second timeout only
    // after its child has completed. Its peer still owns the wave barrier.
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(records(&fixture.root.join(".agent/state/receipts.jsonl")).is_empty());
    release(&fixture, "cargo-a");
    run.finish_success();
    let receipts = records(&fixture.root.join(".agent/state/receipts.jsonl"));
    assert_eq!(receipt_for(&receipts, "ordinary-a")["exit_status"], 0);
    let events = records(&fixture.root.join(".agent/state/runs.jsonl"));
    let ordinary = events
        .iter()
        .find(|event| {
            event["event"] == "target_completed" && event["target"]["action"] == "ordinary-a"
        })
        .unwrap();
    assert_eq!(ordinary["result"]["conclusion"], "success", "{ordinary:#}");
    assert_eq!(ordinary["result"]["exit_code"], 0);
}

#[test]
fn blocked_first_resource_does_not_prevent_later_disjoint_target_admission() {
    let fixture = wave_fixture(WaveKind::Disjoint);
    assert_independent_layer(&fixture, 2);
    let owner_root = blocking_owner_repository(&fixture);
    let mut owner = fixture.spawn_in(&owner_root, "external-owner", &[]);
    owner.wait_entered();
    let mut run = fixture.spawn_args("ready-later", &["check", "--profile", "verify"]);
    run.wait_named_entry("cargo-b");
    assert!(
        owner.running(),
        "the first resource must remain unavailable"
    );
    assert!(!fixture.signals.join("entered-cargo-a").exists());
    release(&fixture, "cargo-b");
    run.wait_target_publication("cargo-b");
    let receipts = records(&fixture.root.join(".agent/state/receipts.jsonl"));
    assert_eq!(receipt_for(&receipts, "cargo-b")["exit_status"], 0);
    assert!(
        owner.running(),
        "available work must publish without waiting for an unrelated owner"
    );
    owner.release();
    owner.finish_success();
    run.wait_named_entry("cargo-a");
    release(&fixture, "cargo-a");
    run.finish_success();
    assert!(!fixture.signals.join("overlap").exists());
}

#[test]
fn distinct_cargo_resources_in_one_run_enter_before_either_is_released() {
    let fixture = wave_fixture(WaveKind::Disjoint);
    assert_independent_layer(&fixture, 2);
    let mut run = fixture.spawn_args("disjoint", &["check", "--profile", "verify"]);
    run.wait_named_entry("cargo-a");
    run.wait_named_entry("cargo-b");
    assert!(run.running());
    assert!(
        records(&fixture.root.join(".agent/state/receipts.jsonl")).is_empty(),
        "both resource owners are still held inside their child barriers"
    );
    release(&fixture, "cargo-a");
    release(&fixture, "cargo-b");
    run.finish_success();
    let receipts = records(&fixture.root.join(".agent/state/receipts.jsonl"));
    assert_eq!(receipt_for(&receipts, "cargo-a")["exit_status"], 0);
    assert_eq!(receipt_for(&receipts, "cargo-b")["exit_status"], 0);
    assert!(!fixture.signals.join("overlap").exists());
}

#[test]
fn independent_wave_sibling_source_mutation_invalidates_every_wave_success() {
    let fixture = wave_fixture(WaveKind::Mutating);
    assert_independent_layer(&fixture, 2);
    let mut run = fixture.spawn_args("mutating", &["check", "--profile", "verify"]);
    run.wait_named_entry("cargo-a");
    run.wait_named_entry("cargo-b");
    release(&fixture, "cargo-b");
    run.wait_signal("completed-cargo-b");
    assert!(
        records(&fixture.root.join(".agent/state/receipts.jsonl")).is_empty(),
        "no success may publish while the mutating sibling is still held"
    );
    release(&fixture, "cargo-a");
    run.wait_signal("mutated-cargo-a");
    run.finish_failure();
    let events = records(&fixture.root.join(".agent/state/runs.jsonl"));
    let completed = events
        .iter()
        .filter(|event| event["event"] == "target_completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 2, "{events:#?}");
    let receipts = records(&fixture.root.join(".agent/state/receipts.jsonl"));
    for event in completed {
        assert_ne!(
            event["result"]["conclusion"], "success",
            "common postcondition must reject every provisional success: {event:#}"
        );
        let receipt = receipt_for(&receipts, event["target"]["action"].as_str().unwrap());
        assert_ne!(receipt["exit_status"], 0, "{receipt:#}");
        assert_eq!(
            receipt["target_freshness"]["state"], "incomplete",
            "source drift cannot publish valid proof: {receipt:#}"
        );
    }
    assert!(!fixture.signals.join("overlap").exists());
}
