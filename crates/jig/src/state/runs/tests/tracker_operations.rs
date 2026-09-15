use super::*;
use crate::state::jsonl::append_jsonl;
use crate::state::{PlanCloseRequest, plans_close, seed_open_plan_for_test};

fn tracker_operation_fact(
    event_id: &str,
    operation_id: &str,
    plan_id: &str,
    phase: &str,
    outcome: Option<&str>,
    run_ids: &[&str],
) -> serde_json::Value {
    let mut fact = serde_json::json!({
        "schema_version": 1,
        "event_id": event_id,
        "operation_id": operation_id,
        "plan_id": plan_id,
        "issue": {
            "provider": "beads",
            "workspace_id": "ExampleProject",
            "issue_id": "example-123",
            "tracker_root": ".beads",
        },
        "kind": "complete_issue",
        "phase": phase,
        "timestamp_ms": 1,
        "run_ids": run_ids,
    });
    if let Some(outcome) = outcome {
        fact["outcome"] = serde_json::json!(outcome);
    }
    fact
}

#[test]
fn pending_tracker_operation_protects_a_closed_plans_completed_run() {
    let (_temp, ctx) = context();
    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "# Example plan\n").unwrap();
    let (started, lease) = start_run(
        &ctx,
        RunPlan::new(
            "run-plan_example",
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            Vec::new(),
            Vec::new(),
        ),
        Some("plan_example".into()),
    )
    .unwrap();
    let run_id = started.result.run_id;
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
    drop(lease);
    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: "plan_example".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();
    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-intent",
            "tracker-operation-complete",
            "plan_example",
            "intent",
            None,
            &[&run_id],
        ),
    )
    .unwrap();

    let retained = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();

    assert_eq!(retained["runs_archived"], 0);
    assert_eq!(retained["protected_runs_retained"], 1);
    assert_eq!(
        run_by_id(&ctx, &run_id).unwrap().result.status,
        RunStatus::Completed
    );

    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-acknowledgement",
            "tracker-operation-complete",
            "plan_example",
            "acknowledgement",
            Some("applied"),
            &[&run_id],
        ),
    )
    .unwrap();
    let archived = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();

    assert_eq!(archived["runs_archived"], 1);
    assert!(run_by_id(&ctx, &run_id).is_err());
}

#[test]
fn invalid_tracker_operation_authority_aborts_run_archive_without_artifacts() {
    let torn = serde_json::to_vec(&tracker_operation_fact(
        "tracker-event-torn",
        "tracker-operation-complete",
        "plan_example",
        "intent",
        None,
        &["run_example"],
    ))
    .unwrap();
    let cases = [
        ("corrupt", b"{not-json}\n".to_vec(), "Failed to parse"),
        (
            "future",
            b"{\"schema_version\":99}\n".to_vec(),
            "unsupported schema version 99",
        ),
        ("torn", torn, "unterminated final record"),
    ];

    for (case, authority, expected_error) in cases {
        let (_temp, ctx) = context();
        let completed_plan = RunPlan::new(
            "run-plan_example",
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            Vec::new(),
            Vec::new(),
        );
        let (completed, lease) = start_run(&ctx, completed_plan, None).unwrap();
        let run_id = completed.result.run_id;
        complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();
        drop(lease);
        let runs_path = ctx.state_file(RUNS_FILE);
        let runs_before = fs::read(&runs_path).unwrap();
        let tracker_path = ctx.state_file("tracker-operations.jsonl");
        fs::write(&tracker_path, &authority).unwrap();

        let error = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap_err();

        assert!(
            format!("{error:#}").contains(expected_error),
            "unexpected {case} error: {error:#}"
        );
        assert_eq!(
            fs::read(&runs_path).unwrap(),
            runs_before,
            "{case} authority changed run source"
        );
        assert_eq!(
            fs::read(&tracker_path).unwrap(),
            authority,
            "{case} authority changed tracker source"
        );
        assert_eq!(
            run_by_id(&ctx, &run_id).unwrap().result.status,
            RunStatus::Completed
        );
        assert!(!ctx.root().join(".agent/.cache/state-archives").exists());
        assert!(!ctx.root().join(".agent/.cache/state-backups").exists());
    }
}

#[test]
fn run_restore_must_preserve_pending_tracker_evidence() {
    let (_temp, ctx) = context();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let runs_path = ctx.state_file(RUNS_FILE);
    fs::write(&runs_path, b"").unwrap();
    let (missing_backup, _) = crate::state::maintenance::create_runs_backup(
        &ctx,
        &runs_path,
        "runs-missing-tracker-evidence",
        None,
    )
    .unwrap();

    seed_open_plan_for_test(&ctx, "plan_example", "Example plan", "Body").unwrap();
    let empty_plan = |id: &str| {
        RunPlan::new(
            id,
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            Vec::new(),
            Vec::new(),
        )
    };
    let (protected, protected_lease) = start_run(
        &ctx,
        empty_plan("run-plan_protected"),
        Some("plan_example".into()),
    )
    .unwrap();
    let protected_run_id = protected.result.run_id;
    complete_run(&ctx, &protected_run_id, RunConclusion::Success).unwrap();
    drop(protected_lease);
    let protected_bytes = fs::read(&runs_path).unwrap();
    let (preserving_backup, _) = crate::state::maintenance::create_runs_backup(
        &ctx,
        &runs_path,
        "runs-preserving-tracker-evidence",
        None,
    )
    .unwrap();

    let (unrelated, unrelated_lease) =
        start_run(&ctx, empty_plan("run-plan_unrelated"), None).unwrap();
    let unrelated_run_id = unrelated.result.run_id;
    complete_run(&ctx, &unrelated_run_id, RunConclusion::Success).unwrap();
    drop(unrelated_lease);
    append_jsonl(
        &ctx.state_file("tracker-operations.jsonl"),
        &tracker_operation_fact(
            "tracker-event-restore-intent",
            "tracker-operation-restore",
            "plan_example",
            "intent",
            None,
            &[&protected_run_id],
        ),
    )
    .unwrap();
    let current_bytes = fs::read(&runs_path).unwrap();

    let error = crate::state::maintenance::restore_backup(
        &ctx,
        crate::command::StateRestoreRequest {
            backup: missing_backup,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("backup omits"));
    assert_eq!(fs::read(&runs_path).unwrap(), current_bytes);

    let restored = crate::state::maintenance::restore_backup(
        &ctx,
        crate::command::StateRestoreRequest {
            backup: preserving_backup,
        },
    )
    .unwrap();
    assert_eq!(restored["changed"], true);
    assert_eq!(fs::read(&runs_path).unwrap(), protected_bytes);
    assert!(run_by_id(&ctx, &protected_run_id).is_ok());
    assert!(run_by_id(&ctx, &unrelated_run_id).is_err());
}
