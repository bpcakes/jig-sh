use super::{fixture::jig, support::*};

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
    let mut run = start(&fixture, &[]);
    signal(&fixture, "fail-prerequisite");
    release(&fixture, "prerequisite");
    run.wait_target_publication("prerequisite");
    assert_eq!(
        receipt(&records(&fixture, "receipts.jsonl"), "prerequisite")["exit_status"],
        7
    );
    release(&fixture, "slow");
    run.finish_failure();
    assert_dependent_skipped(&fixture);
}

#[test]
fn mutation_before_prerequisite_validation_prevents_dependent_start() {
    let fixture = fixture();
    let mut run = start(&fixture, &[]);
    mutate(&fixture);
    release(&fixture, "prerequisite");
    run.wait_target_publication("prerequisite");
    let receipts = records(&fixture, "receipts.jsonl");
    let prerequisite = receipt(&receipts, "prerequisite");
    assert_ne!(prerequisite["exit_status"], 0);
    assert_eq!(prerequisite["target_freshness"]["state"], "incomplete");
    release(&fixture, "slow");
    run.finish_failure();
    assert_dependent_skipped(&fixture);
}

#[test]
fn mutation_during_dependent_rejects_its_success() {
    let fixture = fixture();
    let mut run = start(&fixture, &[]);
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    mutate(&fixture);
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    release(&fixture, "slow");
    run.finish_failure();
    let receipts = records(&fixture, "receipts.jsonl");
    assert_eq!(receipt(&receipts, "prerequisite")["exit_status"], 0);
    let dependent = receipt(&receipts, "dependent");
    assert_ne!(dependent["exit_status"], 0);
    assert_eq!(dependent["target_freshness"]["state"], "incomplete");
}

#[test]
fn late_mutation_preserves_historical_success_but_prevents_reuse() {
    let fixture = fixture();
    let plan = open_plan(&fixture);
    let mut run = start(&fixture, &["--plan-id", &plan]);
    release(&fixture, "prerequisite");
    run.wait_named_entry("dependent");
    release(&fixture, "dependent");
    run.wait_target_publication("dependent");
    let originals = records(&fixture, "receipts.jsonl");
    for action in ["prerequisite", "dependent"] {
        assert_eq!(receipt(&originals, action)["exit_status"], 0);
    }
    mutate(&fixture);
    release(&fixture, "slow");
    run.finish_failure();
    let after = records(&fixture, "receipts.jsonl");
    for action in ["prerequisite", "dependent"] {
        assert_eq!(receipt(&after, action), receipt(&originals, action));
    }
    assert_ne!(receipt(&after, "slow")["exit_status"], 0);
    let output = jig(&fixture.root)
        .args(["work", "check", "--plan-id", &plan, "--explain", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let invocations = preview["selected_invocations"].as_array().unwrap();
    for action in ["prerequisite", "dependent"] {
        let invocation = invocations
            .iter()
            .find(|i| i["target"]["action"] == action)
            .unwrap();
        assert_eq!(invocation["disposition"], "selected", "{preview:#}");
    }
}

#[test]
fn cancellation_accounts_for_pending_dependent_without_starting_it() {
    let fixture = fixture();
    let mut run = start(&fixture, &[]);
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
