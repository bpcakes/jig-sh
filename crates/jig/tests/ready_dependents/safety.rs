use super::support::*;

#[test]
fn resource_timeout_does_not_cancel_unrelated_dependency_chain() {
    let fixture = resource_timeout_fixture();
    let mut run = start(&fixture);
    // Keep the ordinary prerequisite active until the resource target has
    // exhausted its budget and published its failed source observation.
    run.wait_target_publication("slow");
    assert!(!fixture.signals.join("completed-prerequisite").exists());
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    release(&fixture, "dependent");
    run.finish_failure();

    let events = records(&fixture, "runs.jsonl");
    for (action, conclusion) in [
        ("slow", "timed_out"),
        ("prerequisite", "success"),
        ("dependent", "success"),
    ] {
        assert_eq!(
            result(&events, action)["conclusion"],
            conclusion,
            "{action}"
        );
    }
}

#[test]
fn timed_out_resource_wave_retries_source_only_once_for_all_members() {
    let fixture = disjoint_resource_timeout_fixture();
    let mut run = fixture.spawn_args(
        "example-one-retry",
        &["--json", "check", "--profile", "verify"],
    );
    run.wait_named_entry("prerequisite");
    run.wait_named_entry("slow");
    run.finish_failure();
    let output: serde_json::Value = serde_json::from_str(&run.output()).unwrap();
    assert_eq!(output["source_observations"]["count"], 3, "{output:#}");
    let events = records(&fixture, "runs.jsonl");
    for action in ["prerequisite", "slow"] {
        assert_ne!(result(&events, action)["conclusion"], "success", "{action}");
    }
}

#[test]
fn source_mutation_during_resource_timeout_still_stops_unrelated_work() {
    let fixture = resource_timeout_fixture();
    let mut run = start(&fixture);
    mutate(&fixture);
    run.wait_target_publication("slow");
    // A successful independent observation must still reject changed source.
    release(&fixture, "prerequisite");
    run.finish_failure();
    assert_dependent_skipped(&fixture);
    let events = records(&fixture, "runs.jsonl");
    for action in ["prerequisite", "slow"] {
        assert_ne!(result(&events, action)["conclusion"], "success", "{action}");
    }
}

#[test]
fn fail_fast_does_not_admit_siblings_or_dependents_after_failure() {
    let fixture = fixture();
    let mut run = fixture.spawn_args(
        "example-fail-fast",
        &["check", "--profile", "verify", "--fail-fast"],
    );
    run.wait_named_entry("prerequisite");
    signal(&fixture, "fail-prerequisite");
    release(&fixture, "prerequisite");
    run.finish_failure();
    assert_dependent_skipped(&fixture);
    assert!(!fixture.signals.join("entered-slow").exists());
}

#[test]
fn failed_prerequisite_never_releases_dependent() {
    let fixture = fixture();
    let mut run = start(&fixture);
    signal(&fixture, "fail-prerequisite");
    release(&fixture, "prerequisite");
    run.wait_target_publication("prerequisite");
    assert_eq!(
        result(&records(&fixture, "runs.jsonl"), "prerequisite")["exit_code"],
        7
    );
    release(&fixture, "slow");
    run.finish_failure();
    assert_dependent_skipped(&fixture);
}

#[test]
fn mutation_before_prerequisite_validation_prevents_dependent_start() {
    let fixture = fixture();
    let mut run = start(&fixture);
    mutate(&fixture);
    release(&fixture, "prerequisite");
    run.wait_target_publication("prerequisite");
    let events = records(&fixture, "runs.jsonl");
    assert_ne!(result(&events, "prerequisite")["conclusion"], "success");
    release(&fixture, "slow");
    run.finish_failure();
    assert_dependent_skipped(&fixture);
}

#[test]
fn mutation_during_dependent_rejects_its_success() {
    let fixture = fixture();
    let mut run = start(&fixture);
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    mutate(&fixture);
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    release(&fixture, "slow");
    run.finish_failure();
    let events = records(&fixture, "runs.jsonl");
    assert_eq!(result(&events, "prerequisite")["conclusion"], "success");
    assert_ne!(result(&events, "dependent")["conclusion"], "success");
}

#[test]
fn late_mutation_preserves_historical_success() {
    let fixture = fixture();
    let mut run = start(&fixture);
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    let originals = records(&fixture, "runs.jsonl");
    for action in ["prerequisite", "dependent"] {
        assert_eq!(result(&originals, action)["conclusion"], "success");
    }
    mutate(&fixture);
    release(&fixture, "slow");
    run.finish_failure();
    let after = records(&fixture, "runs.jsonl");
    for action in ["prerequisite", "dependent"] {
        assert_eq!(result(&after, action), result(&originals, action));
    }
    assert_ne!(result(&after, "slow")["conclusion"], "success");
}

#[test]
fn cancellation_accounts_for_pending_dependent_without_starting_it() {
    let fixture = fixture();
    let mut run = start(&fixture);
    run.cancel();
    run.finish_failure();
    assert!(!fixture.signals.join("entered-dependent").exists());
    let events = records(&fixture, "runs.jsonl");
    let completions = events
        .iter()
        .filter(|e| e["event"] == "target_completed")
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 3, "{events:#?}");
    assert!(
        completions
            .iter()
            .all(|e| e["result"]["conclusion"] != "success")
    );
}
