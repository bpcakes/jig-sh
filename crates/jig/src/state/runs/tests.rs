use jig_contract::{
    ActionIntent, ActionRunner, PlannedTarget, RunConclusion, RunPlan, RunStatus, SourceIdentity,
};
use tempfile::tempdir;

use super::*;
use crate::test_env::TestRepoBuilder;

fn context() -> (tempfile::TempDir, RepoContext) {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(["rust_test_command"])
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    (temp, ctx)
}

fn plan() -> RunPlan {
    let target: TargetId = "repo:test".parse().unwrap();
    RunPlan::new(
        "run-plan_1",
        "sha256:config",
        SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        vec![PlannedTarget::new(
            target.clone(),
            ActionIntent::Check,
            ActionRunner::command("test"),
            "sha256:input",
        )],
        vec![vec![target]],
    )
}

#[test]
fn lifecycle_round_trips_from_append_only_events() {
    let (_temp, ctx) = context();
    super::super::seed_open_plan_for_test(&ctx, "plan_work", "Work", "Body").unwrap();
    let (started, _lease) = start_run(&ctx, plan(), Some("plan_work".into())).unwrap();
    let run_id = started.result.run_id;
    let target: TargetId = "repo:test".parse().unwrap();

    mark_run_running(&ctx, &run_id).unwrap();
    mark_target_started(&ctx, &run_id, target.clone()).unwrap();
    let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.started_at_ms = Some(2);
    result.ended_at_ms = Some(3);
    result.exit_code = Some(0);
    record_target_result(&ctx, &run_id, result).unwrap();
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();

    let reloaded = run_by_id(&ctx, &run_id).unwrap();
    assert_eq!(reloaded.work_plan_id.as_deref(), Some("plan_work"));
    assert_eq!(reloaded.result.status, RunStatus::Completed);
    assert_eq!(reloaded.result.conclusion, Some(RunConclusion::Success));
    assert_eq!(
        reloaded.result.targets[0].conclusion,
        Some(RunConclusion::Success)
    );
}

#[test]
fn abandoned_run_recovery_preserves_a_prior_target_failure() {
    let (_temp, ctx) = context();
    let failed_target: TargetId = "repo:lint".parse().unwrap();
    let unfinished_target: TargetId = "repo:test".parse().unwrap();
    let plan = RunPlan::new(
        "run-plan_recovery",
        "sha256:config",
        SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        vec![
            PlannedTarget::new(
                failed_target.clone(),
                ActionIntent::Check,
                ActionRunner::command("test"),
                "sha256:lint-input",
            ),
            PlannedTarget::new(
                unfinished_target.clone(),
                ActionIntent::Check,
                ActionRunner::command("test"),
                "sha256:test-input",
            ),
        ],
        vec![vec![failed_target.clone(), unfinished_target]],
    );
    let (started, _lease) = start_run(&ctx, plan, None).unwrap();
    let run_id = started.result.run_id;
    mark_run_running(&ctx, &run_id).unwrap();
    mark_target_started(&ctx, &run_id, failed_target.clone()).unwrap();
    let mut failed = TargetRunResult::queued(failed_target, "sha256:config", "sha256:lint-input");
    failed.status = RunStatus::Completed;
    failed.conclusion = Some(RunConclusion::Failure);
    failed.ended_at_ms = Some(3);
    failed.exit_code = Some(1);
    record_target_result(&ctx, &run_id, failed).unwrap();

    block_nonterminal_run(&ctx, &run_id, "worker stopped").unwrap();

    let recovered = run_by_id(&ctx, &run_id).unwrap();
    assert_eq!(recovered.result.conclusion, Some(RunConclusion::Failure));
    assert_eq!(
        recovered.result.targets[0].conclusion,
        Some(RunConclusion::Failure)
    );
    assert_eq!(
        recovered.result.targets[1].conclusion,
        Some(RunConclusion::Blocked)
    );
}

#[test]
fn start_run_rejects_execution_layers_that_do_not_cover_the_plan() {
    let (_temp, ctx) = context();
    let mut invalid = plan();
    invalid.execution_layers.clear();

    let error = start_run(&ctx, invalid, None)
        .err()
        .expect("invalid execution layers must be rejected");

    assert!(
        error
            .to_string()
            .contains("execution layers omit planned target(s): repo:test")
    );
    assert!(!ctx.state_file(RUNS_FILE).exists());
    assert!(!ctx.root().join(RUN_LEASE_DIR).exists());
}

#[test]
fn archive_removes_completed_runs_and_keeps_recovery_artifacts() {
    let (_temp, ctx) = context();
    let (completed, completed_lease) = start_run(&ctx, plan(), None).unwrap();
    let completed_id = completed.result.run_id;
    let lease_path = run_lease_path(&ctx, &completed_id).unwrap();
    let target: TargetId = "repo:test".parse().unwrap();
    mark_run_running(&ctx, &completed_id).unwrap();
    mark_target_started(&ctx, &completed_id, target.clone()).unwrap();
    let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.started_at_ms = Some(1);
    result.ended_at_ms = Some(2);
    result.exit_code = Some(0);
    record_target_result(&ctx, &completed_id, result).unwrap();
    complete_run(&ctx, &completed_id, RunConclusion::Success).unwrap();
    assert!(lease_path.is_file());
    drop(completed_lease);
    let preview = runs_archive(&ctx, &u64::MAX.to_string(), true).unwrap();
    assert_eq!(preview["runs_archived"], 1);
    assert_eq!(preview["runs_retained"], 0);
    assert_eq!(preview["run_leases_pruned"], 0);
    assert!(preview["runs_archive_path"].is_null());
    assert!(lease_path.is_file());
    assert!(run_by_id(&ctx, &completed_id).is_ok());

    let archived = super::super::state_archive(
        &ctx,
        crate::command::StateArchiveRequest {
            before: u64::MAX.to_string(),
            include_runs: true,
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(archived["runs_archived"], 1);
    assert_eq!(archived["run_leases_pruned"], 1);
    assert!(Path::new(archived["runs_archive_path"].as_str().unwrap()).is_file());
    assert!(!lease_path.exists());
    let recovery =
        std::path::PathBuf::from(archived["runs_recovery_backup_path"].as_str().unwrap());
    assert!(recovery.join("manifest.json").is_file());
    assert!(run_by_id(&ctx, &completed_id).is_err());

    super::super::restore_backup(
        &ctx,
        crate::command::StateRestoreRequest { backup: recovery },
    )
    .unwrap();
    assert_eq!(
        run_by_id(&ctx, &completed_id).unwrap().result.status,
        RunStatus::Completed
    );
}

#[test]
fn archive_retains_a_terminal_run_until_its_worker_lease_is_released() {
    let (_temp, ctx) = context();
    let (completed, completed_lease) = start_run(&ctx, plan(), None).unwrap();
    let completed_id = completed.result.run_id;
    let lease_path = run_lease_path(&ctx, &completed_id).unwrap();
    let target: TargetId = "repo:test".parse().unwrap();
    mark_run_running(&ctx, &completed_id).unwrap();
    mark_target_started(&ctx, &completed_id, target.clone()).unwrap();
    let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.started_at_ms = Some(1);
    result.ended_at_ms = Some(2);
    result.exit_code = Some(0);
    record_target_result(&ctx, &completed_id, result).unwrap();
    complete_run(&ctx, &completed_id, RunConclusion::Success).unwrap();

    let retained = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();

    assert_eq!(retained["runs_archived"], 0);
    assert_eq!(retained["active_run_leases_retained"], 1);
    assert!(lease_path.is_file());
    assert!(run_by_id(&ctx, &completed_id).is_ok());

    drop(completed_lease);
    let archived = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();

    assert_eq!(archived["runs_archived"], 1);
    assert_eq!(archived["active_run_leases_retained"], 0);
    assert_eq!(archived["run_leases_pruned"], 1);
    assert!(!lease_path.exists());
    assert!(run_by_id(&ctx, &completed_id).is_err());
}

#[test]
fn archive_refuses_to_shift_the_cursor_of_a_nonterminal_run() {
    let (_temp, ctx) = context();
    let (completed, _completed_lease) = start_run(&ctx, plan(), None).unwrap();
    let completed_id = completed.result.run_id;
    let target: TargetId = "repo:test".parse().unwrap();
    mark_run_running(&ctx, &completed_id).unwrap();
    mark_target_started(&ctx, &completed_id, target.clone()).unwrap();
    let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.started_at_ms = Some(1);
    result.ended_at_ms = Some(2);
    result.exit_code = Some(0);
    record_target_result(&ctx, &completed_id, result).unwrap();
    complete_run(&ctx, &completed_id, RunConclusion::Success).unwrap();
    let (active, _active_lease) = start_run(&ctx, plan(), None).unwrap();

    let error = runs_archive(&ctx, &u64::MAX.to_string(), true).unwrap_err();

    assert!(error.to_string().contains("1 nonterminal run(s)"));
    assert_eq!(
        run_by_id(&ctx, &completed_id).unwrap().result.status,
        RunStatus::Completed
    );
    assert_eq!(
        run_by_id(&ctx, &active.result.run_id)
            .unwrap()
            .result
            .status,
        RunStatus::Queued
    );
}

#[test]
fn run_start_cursor_observes_cancellation_after_an_archive_rewrite() {
    let (_temp, ctx) = context();
    let empty_plan = RunPlan::new(
        "run-plan_completed",
        "sha256:config",
        SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        Vec::new(),
        Vec::new(),
    );
    let (completed, completed_lease) = start_run(&ctx, empty_plan, None).unwrap();
    complete_run(&ctx, &completed.result.run_id, RunConclusion::Success).unwrap();
    drop(completed_lease);
    let archived = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();
    assert_eq!(archived["runs_archived"], 1);

    let (active, _active_lease, mut cursor) =
        start_run_with_event_cursor(&ctx, plan(), None).unwrap();
    request_run_cancel(&ctx, &active.result.run_id).unwrap();

    assert!(
        run_cancel_requested_since(&ctx, &active.result.run_id, &mut cursor, &|| false).unwrap()
    );
}

#[test]
fn restore_refuses_to_replace_a_live_run_journal() {
    let (_temp, ctx) = context();
    let empty_plan = || {
        RunPlan::new(
            "run-plan_empty",
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            Vec::new(),
            Vec::new(),
        )
    };
    let (backed_up, backed_up_lease) = start_run(&ctx, empty_plan(), None).unwrap();
    complete_run(&ctx, &backed_up.result.run_id, RunConclusion::Success).unwrap();
    drop(backed_up_lease);
    let runs_path = ctx.state_file(RUNS_FILE);
    let (backup, _) = super::super::maintenance::create_runs_backup(
        &ctx,
        &runs_path,
        "runs-restore-fixture",
        None,
    )
    .unwrap();

    let (active, active_lease) = start_run(&ctx, empty_plan(), None).unwrap();
    let before_nonterminal_restore = fs::read(&runs_path).unwrap();
    let error = super::super::restore_backup(
        &ctx,
        crate::command::StateRestoreRequest {
            backup: backup.clone(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("nonterminal run(s) exist"));
    assert_eq!(fs::read(&runs_path).unwrap(), before_nonterminal_restore);

    complete_run(&ctx, &active.result.run_id, RunConclusion::Success).unwrap();
    let before_active_lease_restore = fs::read(&runs_path).unwrap();
    let error = super::super::restore_backup(
        &ctx,
        crate::command::StateRestoreRequest {
            backup: backup.clone(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("active worker lease(s) remain"));
    assert_eq!(fs::read(&runs_path).unwrap(), before_active_lease_restore);

    // Restore is also used when the active journal was lost. A live worker's
    // lease remains authoritative even when its run ID can no longer be read
    // from that stream.
    fs::write(&runs_path, b"").unwrap();
    let error = super::super::restore_backup(
        &ctx,
        crate::command::StateRestoreRequest {
            backup: backup.clone(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("active worker lease(s) remain"));
    assert!(fs::read(&runs_path).unwrap().is_empty());

    drop(active_lease);
    let restored =
        super::super::restore_backup(&ctx, crate::command::StateRestoreRequest { backup }).unwrap();
    assert_eq!(restored["changed"], true);
    assert_eq!(
        run_by_id(&ctx, &backed_up.result.run_id)
            .unwrap()
            .result
            .status,
        RunStatus::Completed
    );
    assert!(run_by_id(&ctx, &active.result.run_id).is_err());
}

#[test]
fn archive_rejects_run_ids_that_could_escape_the_lease_directory() {
    let (_temp, ctx) = context();
    let runs_path = ctx.state_file(RUNS_FILE);
    let escaped_path = ctx.root().join(".agent/outside.lock");
    fs::create_dir_all(ctx.root().join(RUN_LEASE_DIR)).unwrap();
    fs::write(&escaped_path, b"keep").unwrap();
    let unsafe_run_id = "../../outside";
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_queued".into(),
            run_id: unsafe_run_id.into(),
            event: EVENT_QUEUED.into(),
            timestamp_ms: 1,
            work_plan_id: None,
            plan: Some(RunPlan::new(
                "run-plan_unsafe-id",
                "sha256:config",
                SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
                Vec::new(),
                Vec::new(),
            )),
            target: None,
            result: None,
            conclusion: None,
        },
    )
    .unwrap();
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_completed".into(),
            run_id: unsafe_run_id.into(),
            event: EVENT_COMPLETED.into(),
            timestamp_ms: 2,
            work_plan_id: None,
            plan: None,
            target: None,
            result: None,
            conclusion: Some(RunConclusion::Success),
        },
    )
    .unwrap();
    let before = fs::read(&runs_path).unwrap();

    let error = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap_err();

    assert!(error.to_string().contains("safe worker lease filename"));
    assert_eq!(fs::read(&runs_path).unwrap(), before);
    assert_eq!(fs::read(&escaped_path).unwrap(), b"keep");
}

#[test]
fn archive_apply_reconciles_an_abandoned_run_before_rewriting() {
    let (_temp, ctx) = context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    drop(lease);

    let archived = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();

    assert_eq!(archived["abandoned_runs_reconciled"], 1);
    assert_eq!(archived["runs_archived"], 1);
    assert!(run_by_id(&ctx, &run_id).is_err());
}

#[test]
fn archive_retains_completed_runs_linked_to_open_work_plans() {
    let (_temp, ctx) = context();
    super::super::seed_open_plan_for_test(&ctx, "plan_open", "Open", "Body").unwrap();
    let (started, _lease) = start_run(
        &ctx,
        RunPlan::new(
            "run-plan_empty",
            "sha256:config",
            SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
            Vec::new(),
            Vec::new(),
        ),
        Some("plan_open".into()),
    )
    .unwrap();
    complete_run(&ctx, &started.result.run_id, RunConclusion::Success).unwrap();

    let archived = runs_archive(&ctx, &u64::MAX.to_string(), true).unwrap();

    assert_eq!(archived["runs_archived"], 0);
    assert_eq!(archived["protected_runs_retained"], 1);
}

#[test]
fn run_lookup_only_deserializes_full_events_for_the_requested_run() {
    let (_temp, ctx) = context();
    let (_first, _first_lease) = start_run(&ctx, plan(), None).unwrap();
    let (second, _second_lease) = start_run(&ctx, plan(), None).unwrap();
    FULL_RUN_EVENT_PARSE_COUNT.with(|counter| counter.set(0));
    RUN_EVENT_IDENTITY_PARSE_COUNT.with(|counter| counter.set(0));

    let loaded = run_by_id(&ctx, &second.result.run_id).unwrap();

    assert_eq!(loaded.result.run_id, second.result.run_id);
    FULL_RUN_EVENT_PARSE_COUNT.with(|counter| assert_eq!(counter.get(), 1));
    RUN_EVENT_IDENTITY_PARSE_COUNT.with(|counter| assert_eq!(counter.get(), 1));
}

#[test]
fn reverse_run_lookup_handles_records_larger_than_the_read_chunk() {
    let (_temp, ctx) = context();
    let mut large_plan = plan();
    large_plan.selectors = vec!["x".repeat(REVERSE_RUN_READ_CHUNK * 2)];
    let (started, _lease) = start_run(&ctx, large_plan, None).unwrap();

    let loaded = run_by_id(&ctx, &started.result.run_id).unwrap();

    assert_eq!(loaded.result.run_id, started.result.run_id);
    assert_eq!(loaded.plan.selectors[0].len(), REVERSE_RUN_READ_CHUNK * 2);
}

#[test]
fn run_lookup_rejects_an_unterminated_final_record() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    let path = ctx.state_file(RUNS_FILE);
    let mut bytes = fs::read(&path).unwrap();
    assert_eq!(bytes.pop(), Some(b'\n'));
    fs::write(&path, bytes).unwrap();

    let error = run_by_id(&ctx, &started.result.run_id).unwrap_err();

    assert!(error.to_string().contains("not newline-terminated"));
}

#[test]
fn reverse_run_lookup_rejects_an_escape_rewritten_key_and_value() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_duplicate_queued".into(),
            run_id: started.result.run_id.clone(),
            event: EVENT_QUEUED.into(),
            timestamp_ms: now_ms(),
            work_plan_id: None,
            plan: Some(plan()),
            target: None,
            result: None,
            conclusion: None,
        },
    )
    .unwrap();
    let path = ctx.state_file(RUNS_FILE);
    let rewritten = fs::read_to_string(&path).unwrap().replacen(
        "\"run_id\":\"run_",
        "\"run\\u005fid\" : \"\\u0072un_",
        1,
    );
    fs::write(&path, rewritten).unwrap();

    let error = run_by_id(&ctx, &started.result.run_id).unwrap_err();

    assert!(error.to_string().contains("more than one queued event"));
}

#[test]
fn cancellation_requests_are_idempotent() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;

    let first = request_run_cancel(&ctx, &run_id).unwrap();
    let second = request_run_cancel(&ctx, &run_id).unwrap();

    assert!(first.cancel_requested);
    assert!(second.cancel_requested);
    let events = std::fs::read_to_string(ctx.state_file(RUNS_FILE)).unwrap();
    assert_eq!(events.matches(EVENT_CANCEL_REQUESTED).count(), 1);
}

#[test]
fn queued_runs_keep_a_stable_lease_inode_after_reconciliation() {
    let (_temp, ctx) = context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    let lease_path = ctx
        .root()
        .join(RUN_LEASE_DIR)
        .join(format!("{run_id}.lock"));

    assert!(lease_path.exists());
    let inspected = reconcile_run_for_inspection(&ctx, &run_id).unwrap();
    assert_eq!(inspected.result.status, RunStatus::Queued);

    drop(lease);
    assert!(lease_path.exists());
    let recovered = reconcile_run_for_inspection(&ctx, &run_id).unwrap();
    assert_eq!(recovered.result.status, RunStatus::Completed);
    assert_eq!(recovered.result.conclusion, Some(RunConclusion::Blocked));
    assert!(lease_path.exists());
}

#[test]
fn concurrent_reconciliation_appends_one_terminal_event() {
    use std::sync::{Arc, Barrier};

    let (_temp, ctx) = context();
    let (started, lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    drop(lease);

    let worker_count = 8;
    let barrier = Arc::new(Barrier::new(worker_count));
    let handles = (0..worker_count)
        .map(|_| {
            let ctx = ctx.clone();
            let run_id = run_id.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                reconcile_run_for_inspection(&ctx, &run_id).unwrap()
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let run = handle.join().unwrap();
        assert!(matches!(
            run.result.status,
            RunStatus::Queued | RunStatus::Completed
        ));
    }
    let recovered = reconcile_run_for_inspection(&ctx, &run_id).unwrap();
    assert_eq!(recovered.result.status, RunStatus::Completed);
    assert_eq!(recovered.result.conclusion, Some(RunConclusion::Blocked));
    let terminal_events = fs::read_to_string(ctx.state_file(RUNS_FILE))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<RunEventRecord>(line).unwrap())
        .filter(|event| event.run_id == run_id && event.event == EVENT_COMPLETED)
        .count();
    assert_eq!(terminal_events, 1);
}

#[test]
fn cancellation_observed_after_completion_does_not_corrupt_the_run() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    let run_id = started.result.run_id;
    let target: TargetId = "repo:test".parse().unwrap();
    let mut result = TargetRunResult::queued(target, "sha256:config", "sha256:input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    record_target_result(&ctx, &run_id, result).unwrap();
    complete_run(&ctx, &run_id, RunConclusion::Success).unwrap();

    append_simple_event(&ctx, &run_id, EVENT_CANCEL_REQUESTED, None, None).unwrap();
    let reloaded = run_by_id(&ctx, &run_id).unwrap();

    assert_eq!(reloaded.result.status, RunStatus::Completed);
    assert_eq!(reloaded.result.conclusion, Some(RunConclusion::Success));
    assert!(!reloaded.cancel_requested);
}

#[test]
fn unknown_future_events_are_ignored() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_future".into(),
            run_id: started.result.run_id.clone(),
            event: "future_annotation".into(),
            timestamp_ms: now_ms(),
            work_plan_id: None,
            plan: None,
            target: None,
            result: None,
            conclusion: None,
        },
    )
    .unwrap();

    assert_eq!(
        run_by_id(&ctx, &started.result.run_id)
            .unwrap()
            .result
            .status,
        RunStatus::Queued
    );
}

#[test]
fn archive_retains_unknown_only_future_runs_without_treating_them_as_nonterminal() {
    let (_temp, ctx) = context();
    let run_id = "run_future_only";
    append_event(
        &ctx,
        RunEventRecord {
            id: "run_event_future_only".into(),
            run_id: run_id.into(),
            event: "future_annotation".into(),
            timestamp_ms: 1,
            work_plan_id: None,
            plan: None,
            target: None,
            result: None,
            conclusion: None,
        },
    )
    .unwrap();

    let preview = runs_archive(&ctx, &u64::MAX.to_string(), true).unwrap();
    assert_eq!(preview["runs_archived"], 0);
    assert_eq!(preview["runs_retained"], 1);

    let applied = runs_archive(&ctx, &u64::MAX.to_string(), false).unwrap();
    assert_eq!(applied["runs_archived"], 0);
    assert_eq!(applied["runs_retained"], 1);
    assert!(
        fs::read_to_string(ctx.state_file(RUNS_FILE))
            .unwrap()
            .contains(run_id)
    );
}

#[test]
fn corrupt_known_lifecycle_is_rejected() {
    let (_temp, ctx) = context();
    let (started, _lease) = start_run(&ctx, plan(), None).unwrap();
    complete_run(&ctx, &started.result.run_id, RunConclusion::Success).unwrap();

    let error = run_by_id(&ctx, &started.result.run_id).unwrap_err();
    assert!(error.to_string().contains("before every target"));
}
