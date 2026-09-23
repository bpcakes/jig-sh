use super::support::*;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

#[test]
fn ordinary_and_resource_workers_share_eight_execution_slots() {
    let fixture = wide_fixture();
    let mut run = fixture.spawn_args("example-wide", &["check", "--profile", "verify"]);
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
    assert!(!fixture.signals.join("capacity-exceeded").exists());
    let mut active = std::collections::BTreeSet::new();
    for event in records(&fixture, "runs.jsonl") {
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
    let receipts = records(&fixture, "receipts.jsonl");
    assert_eq!(receipts.len(), 11);
    assert!(receipts.iter().all(|receipt| receipt["exit_status"] == 0));
}
