use super::*;
use crate::test_env::TestRepoBuilder;
use tempfile::tempdir;

#[test]
fn working_directory_rejects_parent_escape() {
    let temp = tempdir().unwrap();
    assert!(
        resolve_repository_working_directory(temp.path(), Some("../outside"))
            .unwrap_err()
            .to_string()
            .contains("relative repository path")
    );
}

#[test]
fn json_lines_parser_normalizes_findings_and_rejects_bad_lines() {
    let valid = r#"{"severity":"warning","message":"unused","source":"lint"}"#;
    let parsed = parse_findings(ResultParser::JsonLines, valid);
    assert_eq!(parsed.findings[0].severity, FindingSeverity::Warning);
    assert!(parsed.succeeded);

    let parsed = parse_findings(ResultParser::JsonLines, "not-json");
    assert!(!parsed.succeeded);

    let unknown_field = r#"{"severity":"warning","message":"unused","progress":50}"#;
    let parsed = parse_findings(ResultParser::JsonLines, unknown_field);
    assert!(!parsed.succeeded);
    assert_eq!(parsed.findings[0].source.as_deref(), Some("result_parser"));
}

#[test]
fn tool_findings_cannot_spoof_result_parser_failure() {
    let valid =
        r#"{"severity":"warning","message":"named like the parser","source":"result_parser"}"#;
    let capture =
        TargetCapture::from_process(0, valid.into(), String::new(), ResultParser::JsonLines);

    assert_eq!(capture.conclusion, RunConclusion::Success);
    assert_eq!(capture.findings[0].source.as_deref(), Some("result_parser"));
}

#[test]
fn json_lines_error_finding_fails_a_zero_exit_target() {
    let output = r#"{"severity":"error","message":"tests failed","source":"test"}"#;
    let capture =
        TargetCapture::from_process(0, output.into(), String::new(), ResultParser::JsonLines);

    assert_eq!(capture.conclusion, RunConclusion::Failure);
    assert_eq!(capture.exit_code, Some(0));
    assert_eq!(capture.receipt_exit_status, 1);
    assert_eq!(capture.findings.len(), 1);
}

#[test]
fn cancellation_poll_failures_block_poll_induced_cancellation_without_masking_a_target_failure() {
    let target: TargetId = "repo:test".parse().unwrap();
    let planned = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(["rust_test_command"])
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let cancelled = || Err(anyhow::anyhow!("durable state is unavailable"));
    let mut run_control = CancellationOnlyRunControl {
        cancelled: &cancelled,
    };
    let control = TargetExecutionControl::new(&ctx, &planned, &mut run_control);

    let stop = control.remaining().unwrap_err();
    assert!(matches!(stop, TargetStop::Blocked(_)));
    let capture = control.enforce_poll_health(TargetCapture::from_process(
        0,
        String::new(),
        String::new(),
        ResultParser::ExitCode,
    ));

    assert_eq!(capture.conclusion, RunConclusion::Blocked);
    assert_eq!(capture.receipt_exit_status, 1);
    assert!(capture.stderr.contains("durable state is unavailable"));
    assert_eq!(capture.findings[0].source.as_deref(), Some("cancellation"));

    let cancelled = control.enforce_poll_health(TargetCapture::stopped_after_start(
        RunConclusion::Cancelled,
        "command was cancelled",
    ));

    assert_eq!(cancelled.conclusion, RunConclusion::Blocked);
    assert_eq!(cancelled.receipt_exit_status, 1);
    assert!(cancelled.stderr.contains("durable state is unavailable"));

    let failed = control.enforce_poll_health(TargetCapture::from_process(
        7,
        String::new(),
        "test failure\n".into(),
        ResultParser::ExitCode,
    ));

    assert_eq!(failed.conclusion, RunConclusion::Failure);
    assert_eq!(failed.receipt_exit_status, 7);
    assert!(failed.stderr.contains("test failure"));
    assert!(failed.stderr.contains("durable state is unavailable"));
}

#[test]
fn native_cancellation_preserves_whether_a_child_started() {
    let planned = PlannedTarget::new(
        "repo:schema".parse().unwrap(),
        jig_contract::ActionIntent::Check,
        ActionRunner::native("jig.schema_check"),
        "sha256:input",
    );

    let before_start = native_runner_error_capture(
        &planned,
        "jig.schema_check",
        Duration::from_secs(30),
        OwnedProcessTreeError::CancelledBeforeStart.into(),
    );
    let after_start = native_runner_error_capture(
        &planned,
        "jig.schema_check",
        Duration::from_secs(30),
        OwnedProcessTreeError::Cancelled.into(),
    );

    assert_eq!(before_start.conclusion, RunConclusion::Cancelled);
    assert!(!before_start.may_have_executed);
    assert_eq!(before_start.exit_code, None);
    assert_eq!(after_start.conclusion, RunConclusion::Cancelled);
    assert!(after_start.may_have_executed);
    assert_eq!(after_start.exit_code, None);
}

#[test]
fn adjacent_targets_share_the_post_target_source_observation() {
    let target: TargetId = "repo:test".parse().unwrap();
    let planned = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let mut scans = 0;

    epoch
        .prepare_target_with(&planned, || {
            scans += 1;
            Ok("sha256:stable".into())
        })
        .unwrap();
    let capture =
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode);
    let (capture, _) = epoch.finish_target_with(&planned, capture, || {
        scans += 1;
        Ok("sha256:stable".into())
    });
    epoch
        .prepare_target_with(&planned, || {
            scans += 1;
            Ok("sha256:stable".into())
        })
        .unwrap();

    assert_eq!(capture.conclusion, RunConclusion::Success);
    assert_eq!(
        scans, 2,
        "the adjacent precondition must reuse the post-target scan"
    );
}

#[test]
fn failed_post_target_source_observation_is_retried_before_the_next_target() {
    let target: TargetId = "repo:test".parse().unwrap();
    let planned = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let capture =
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode);
    let (_, observed) = epoch.finish_target_with(&planned, capture, || {
        Err("transient fingerprint failure".into())
    });
    assert!(observed.is_err());

    let mut scans = 0;
    epoch
        .prepare_target_with(&planned, || {
            scans += 1;
            Ok("sha256:stable".into())
        })
        .unwrap();

    assert_eq!(scans, 1);
}

#[test]
fn a_skipped_target_discards_the_previous_source_observation() {
    let target: TargetId = "repo:test".parse().unwrap();
    let planned = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let capture =
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode);
    let _ = epoch.finish_target_with(&planned, capture, || Ok("sha256:stable".into()));
    epoch.discard_reusable_observation();
    let mut scans = 0;

    epoch
        .prepare_target_with(&planned, || {
            scans += 1;
            Ok("sha256:stable".into())
        })
        .unwrap();

    assert_eq!(scans, 1);
}

#[test]
fn entering_a_parallel_layer_discards_the_previous_source_observation() {
    let target: TargetId = "repo:test".parse().unwrap();
    let planned = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let capture =
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode);
    let _ = epoch.finish_target_with(&planned, capture, || Ok("sha256:stable".into()));
    epoch.begin_read_only_layer();
    let mut scans = 0;

    epoch
        .prepare_target_with(&planned, || {
            scans += 1;
            Ok("sha256:stable".into())
        })
        .unwrap();

    assert_eq!(
        scans, 1,
        "a layer boundary cannot reuse an observation from before authority validation"
    );
}

#[test]
fn a_worktree_mutating_target_refreshes_its_source_precondition() {
    let target: TargetId = "repo:test".parse().unwrap();
    let read_only = PlannedTarget::new(
        target.clone(),
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let mut mutating = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Generate,
        ActionRunner::command("generate_command"),
        "sha256:input",
    );
    mutating.effects.push(jig_contract::ActionEffect::Worktree);
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let mut scans = 0;

    epoch
        .prepare_target_with(&read_only, || {
            scans += 1;
            Ok("sha256:stable".into())
        })
        .unwrap();
    let capture =
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode);
    let _ = epoch.finish_target_with(&read_only, capture, || {
        scans += 1;
        Ok("sha256:stable".into())
    });

    let error = epoch
        .prepare_target_with(&mutating, || {
            scans += 1;
            Ok("sha256:external-edit".into())
        })
        .unwrap_err();

    assert_eq!(scans, 3);
    assert!(error.contains("worktree changed"));
}

#[test]
fn a_mutating_target_that_never_executes_cannot_rebase_the_source_epoch() {
    let target: TargetId = "repo:generate".parse().unwrap();
    let mut mutating = PlannedTarget::new(
        target.clone(),
        jig_contract::ActionIntent::Generate,
        ActionRunner::command("generate_command"),
        "sha256:input",
    );
    mutating.effects.push(jig_contract::ActionEffect::Worktree);
    let read_only = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Check,
        ActionRunner::command("rust_test_command"),
        "sha256:input",
    );
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());

    let (capture, fingerprint) = epoch.finish_target_with(
        &mutating,
        TargetCapture::blocked("command was cancelled before spawn"),
        || panic!("a target that never executed must not take a postcondition"),
    );

    assert_eq!(capture.conclusion, RunConclusion::Blocked);
    assert_eq!(fingerprint.as_deref(), Ok("sha256:stable"));
    let error = epoch
        .prepare_target_with(&read_only, || Ok("sha256:external-edit".into()))
        .unwrap_err();
    assert!(error.contains("worktree changed"), "{error}");
}

#[test]
fn an_unverifiable_mutating_postcondition_blocks_success() {
    let target: TargetId = "repo:generate".parse().unwrap();
    let mut planned = PlannedTarget::new(
        target,
        jig_contract::ActionIntent::Generate,
        ActionRunner::command("generate_command"),
        "sha256:input",
    );
    planned.effects.push(jig_contract::ActionEffect::Worktree);
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let capture =
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode);

    let (capture, fingerprint) =
        epoch.finish_target_with(&planned, capture, || Err("fingerprint unavailable".into()));

    assert!(fingerprint.is_err());
    assert_eq!(capture.conclusion, RunConclusion::Blocked);
    assert_eq!(capture.receipt_exit_status, 1);
    assert!(
        capture
            .findings
            .iter()
            .any(|finding| finding.source.as_deref() == Some("effect_policy"))
    );
}

#[test]
fn run_failure_is_not_hidden_by_a_later_cancellation() {
    assert_eq!(
        aggregate_conclusion([RunConclusion::Failure, RunConclusion::Cancelled].into_iter()),
        RunConclusion::Failure
    );
}

#[test]
fn a_run_that_only_skips_targets_does_not_report_success() {
    assert_eq!(
        aggregate_conclusion([RunConclusion::Skipped].into_iter()),
        RunConclusion::Skipped
    );
}

fn plan_with_prepared_work_plans(work_plan_ids: &[Option<&str>]) -> RunPlan {
    let targets = work_plan_ids
        .iter()
        .enumerate()
        .map(|(index, work_plan_id)| {
            let mut target = PlannedTarget::new(
                format!("repo:file-budget-{index}").parse().unwrap(),
                jig_contract::ActionIntent::Check,
                ActionRunner::native(jig_contract::tool::FILE_BUDGET),
                "sha256:input",
            );
            target.prepared_native_input = Some(jig_contract::PreparedNativeInputV1 {
                schema_version: jig_contract::PreparedNativeInputV1::SCHEMA_VERSION,
                view: jig_contract::CurrentViewV1::Inventory,
                request: jig_contract::ComparisonRequestV1::StrictInventory {
                    reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
                },
                configuration: jig_contract::NativeFileBudgetConfigV1::default(),
                policy_source: jig_contract::PolicySourceV1 {
                    path: ".jig/file-budget.toml".into(),
                },
                work_plan_id: work_plan_id.map(str::to_owned),
                policy: jig_contract::PolicyPreparationV1::Ready {
                    policy_raw_digest: "sha256:raw".into(),
                    policy_semantic_digest: "sha256:semantic".into(),
                },
                comparison: jig_contract::ComparisonPreparationV1::Ready {
                    comparison: jig_contract::ResolvedComparisonV1::StrictInventory {
                        reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
                        fallback_from: None,
                    },
                },
            });
            target
        })
        .collect();
    RunPlan::new(
        "plan_example",
        "sha256:config",
        jig_contract::SourceIdentity::new(None, "sha256:worktree"),
        targets,
        Vec::new(),
    )
}

#[test]
fn execution_only_confirms_prepared_work_plan_identity() {
    let plan = plan_with_prepared_work_plans(&[Some("plan_alpha")]);
    validate_prepared_work_plan_identity(&plan, Some("plan_alpha")).unwrap();

    let mismatch = validate_prepared_work_plan_identity(&plan, Some("plan_beta"))
        .unwrap_err()
        .to_string();
    assert!(mismatch.contains("prepared for work_plan_id"), "{mismatch}");

    let inconsistent = plan_with_prepared_work_plans(&[Some("plan_alpha"), None]);
    let error = validate_prepared_work_plan_identity(&inconsistent, Some("plan_alpha"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("inconsistent"), "{error}");
}

#[test]
fn an_accepted_run_becomes_blocked_when_its_worker_stops() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(["rust_test_command"])
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let target: TargetId = "repo:test".parse().unwrap();
    let plan = RunPlan::new(
        "run-plan_1",
        "sha256:config",
        jig_contract::SourceIdentity::new(None, "sha256:worktree"),
        vec![PlannedTarget::new(
            target.clone(),
            jig_contract::ActionIntent::Check,
            ActionRunner::command("rust_test_command"),
            "sha256:input",
        )],
        vec![vec![target]],
    );
    let (run, _lease) = start_run(&ctx, plan, None).unwrap();

    let error = terminalize_started_run(&ctx, &run.result.run_id, || -> Result<()> {
        Err(anyhow::anyhow!("state failure"))
    })
    .unwrap_err();
    let terminal = run_by_id(&ctx, &run.result.run_id).unwrap();

    assert!(error.to_string().contains("state failure"));
    assert_eq!(terminal.result.status, RunStatus::Completed);
    assert_eq!(terminal.result.conclusion, Some(RunConclusion::Blocked));
    assert_eq!(
        terminal.result.targets[0].conclusion,
        Some(RunConclusion::Blocked)
    );
    assert!(
        terminal.result.targets[0].findings[0]
            .message
            .contains("state failure")
    );
}
