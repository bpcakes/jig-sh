use super::fixture::{Fixture, Running};
use super::support::*;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

#[test]
fn contended_resource_waiters_leave_slots_for_an_ordinary_dependency_chain() {
    let fixture = resource_batch_fixture(false);
    let plan = open_plan(&fixture);
    let mut run = fixture.spawn_args(
        "example-contended",
        &["check", "--profile", "verify", "--plan-id", &plan],
    );
    run.wait_named_entry("cargo-0");
    run.wait_named_entry("prerequisite");
    assert!(
        fixture.signals.join("active-cargo-0").exists(),
        "ordinary prerequisite waited for the resource holder to finish"
    );
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    assert!(run.running());
    assert!(fixture.signals.join("active-cargo-0").exists());
    for index in 1..8 {
        assert!(
            !fixture
                .signals
                .join(format!("entered-cargo-{index}"))
                .exists()
        );
    }
    let published = records(&fixture, "receipts.jsonl");
    for action in ["prerequisite", "dependent"] {
        assert_eq!(receipt(&published, action)["exit_status"], 0);
        assert_eq!(
            receipt(&published, action)["target_freshness"]["state"],
            "complete"
        );
    }
    for index in 0..8 {
        release(&fixture, &format!("cargo-{index}"));
    }
    run.finish_success();
    assert!(!fixture.signals.join("capacity-exceeded").exists());
    assert_eq!(
        records(&fixture, "receipts.jsonl")
            .iter()
            .filter(|receipt| receipt["target"].is_object())
            .count(),
        10
    );
}

#[test]
fn ready_resource_starts_before_ordinary_workers_fill_every_slot() {
    let fixture = wide_resource_timeout_fixture();
    let mut run = fixture.spawn_args("example-resource-fair", &["check", "--profile", "verify"]);
    run.wait_named_entry("slow");
    assert!(fixture.signals.join("active-slow").exists());
    for action in ["prerequisite", "dependent", "slow"]
        .into_iter()
        .map(str::to_owned)
        .chain((0..8).map(|index| format!("sibling-{index}")))
    {
        release(&fixture, &action);
    }
    run.finish_success();
    assert_eight_target_bound(&fixture);
}

#[test]
fn disjoint_ninth_resource_runs_while_first_eight_wait_for_an_external_claim() {
    let fixture = resource_batch_with_disjoint_ninth();
    let owner_root = fixture.other_repository("example-external-owner", 30, false);
    let mut owner = fixture.spawn_in(&owner_root, "example-external-owner", &[]);
    owner.wait_entered();
    let mut run = fixture.spawn_args("example-disjoint-ninth", &["check", "--profile", "verify"]);
    run.wait_named_entry("cargo-8");
    for index in 0..8 {
        assert!(
            !fixture
                .signals
                .join(format!("entered-cargo-{index}"))
                .exists()
        );
    }
    release(&fixture, "cargo-8");
    run.wait_target_publication("cargo-8");
    release(&fixture, "prerequisite");
    release(&fixture, "dependent");
    owner.release();
    owner.finish_success();
    for index in 0..8 {
        release(&fixture, &format!("cargo-{index}"));
    }
    run.finish_success();
}

#[test]
fn ordinary_and_resource_workers_share_eight_execution_slots() {
    let fixture = wide_fixture();
    let mut run = fixture.spawn_args("example-wide", &["check", "--profile", "verify"]);
    wait_until_full(&fixture, &mut run);
    assert!(!fixture.signals.join("entered-dependent").exists());
    // Every admitted child remains held until this point. Later children also
    // measure live peers on entry, including across resource/ordinary workers.
    for action in ["prerequisite", "dependent", "slow"]
        .into_iter()
        .map(str::to_owned)
        .chain((0..8).map(|index| format!("sibling-{index}")))
    {
        release(&fixture, &action);
    }
    run.finish_success();
    assert_eight_target_bound(&fixture);
    let receipts = records(&fixture, "receipts.jsonl");
    assert_eq!(receipts.len(), 11);
    assert!(receipts.iter().all(|receipt| receipt["exit_status"] == 0));
}

#[test]
fn disjoint_resource_waves_share_slots_with_an_ordinary_dependency_chain() {
    let fixture = resource_batch_fixture(true);
    let mut run = fixture.spawn_args("example-disjoint", &["check", "--profile", "verify"]);
    wait_until_full(&fixture, &mut run);
    run.wait_named_entry("prerequisite");
    assert!(!fixture.signals.join("entered-cargo-7").exists());
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    for index in 0..7 {
        assert!(
            fixture
                .signals
                .join(format!("active-cargo-{index}"))
                .exists()
        );
        release(&fixture, &format!("cargo-{index}"));
    }
    // Reclaimed slots admit the next resource wave beside the held dependent.
    run.wait_named_entry("cargo-7");
    assert!(fixture.signals.join("active-dependent").exists());
    release(&fixture, "cargo-7");
    release(&fixture, "dependent");
    run.finish_success();
    assert_eight_target_bound(&fixture);
    let receipts = records(&fixture, "receipts.jsonl");
    assert_eq!(receipts.len(), 10);
    assert!(receipts.iter().all(|receipt| receipt["exit_status"] == 0));
}

#[test]
fn cancellation_stops_resource_targets_waiting_for_an_execution_slot() {
    let fixture = resource_batch_fixture(true);
    let mut run = fixture.spawn_args("example-cancel-slots", &["check", "--profile", "verify"]);
    wait_until_full(&fixture, &mut run);
    run.wait_named_entry("prerequisite");
    run.cancel();
    run.finish_failure();
    for action in ["cargo-7", "dependent"] {
        assert!(!fixture.signals.join(format!("entered-{action}")).exists());
    }
    let events = records(&fixture, "runs.jsonl");
    let completed = events
        .iter()
        .filter(|event| event["event"] == "target_completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 10);
    assert!(
        completed
            .iter()
            .all(|event| event["result"]["conclusion"] != "success")
    );
}

fn wait_until_full(fixture: &Fixture, run: &mut Running) {
    let started = Instant::now();
    loop {
        let entered = fs::read_dir(&fixture.signals)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|file| file.file_name().to_string_lossy().starts_with("entered-"))
            .count();
        assert!(
            entered <= 8,
            "more than eight children admitted at held barriers"
        );
        if entered == 8 {
            break;
        }
        assert!(
            run.running(),
            "run ended before filling available execution slots: {}",
            run.output()
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "fewer than eight ready targets admitted"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn assert_eight_target_bound(fixture: &Fixture) {
    assert!(!fixture.signals.join("capacity-exceeded").exists());
    let mut active = std::collections::BTreeSet::new();
    for event in records(fixture, "runs.jsonl") {
        let Some(action) = event["target"]["action"].as_str() else {
            continue;
        };
        match event["event"].as_str() {
            Some("target_started") => {
                assert!(active.insert(action.to_owned()));
                assert!(
                    active.len() <= 8,
                    "admitted more than eight unfinished targets: {active:?}"
                );
            }
            Some("target_completed") => assert!(active.remove(action)),
            _ => {}
        }
    }
    assert!(active.is_empty());
}
