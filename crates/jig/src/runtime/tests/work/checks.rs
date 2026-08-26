use super::*;

#[test]
fn work_check_runs_configured_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["checks"].as_array().unwrap().len(), 1);
    assert_eq!(output["checks"][0]["tool"], "jig.custom_check");
    assert!(output["checks"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_check_emits_one_balanced_phase_per_tool_with_aggregate_positions() {
    #[derive(Default)]
    struct PhaseObserver(Vec<(String, String, usize, usize)>);

    impl crate::execution::ExecutionObserver for PhaseObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { label, position } => {
                    self.0.push((
                        "started".into(),
                        label.into(),
                        position.current(),
                        position.total(),
                    ))
                }
                crate::execution::ExecutionEvent::PhaseFinished { label, .. } => {
                    self.0.push(("finished".into(), label.into(), 0, 0));
                }
                _ => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for PhaseObserver {}

    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = PhaseObserver::default();

    crate::runtime::dispatch_with_observer(
        &ctx,
        RuntimeCommand::Work(crate::command::WorkCommand::Check(
            crate::command::WorkCheckRequest {
                plan_id: "plan_1".into(),
                tools: Vec::new(),
            },
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        observer.0,
        [
            ("started".into(), "jig.first_check".into(), 1, 2),
            ("finished".into(), "jig.first_check".into(), 0, 0),
            ("started".into(), "jig.mutating_check".into(), 2, 2),
            ("finished".into(), "jig.mutating_check".into(), 0, 0),
        ]
    );
}

#[test]
fn work_check_rejects_unknown_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_missing".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn work_check_rejects_closed_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_1".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan is already closed: plan_1"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn work_check_collects_change_metadata_only_on_batch_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("outside-agent.txt"), "changed\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let receipts_text = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .expect("work check should write receipts");
    let receipts = receipts_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let tool_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.custom_check")
        .expect("tool receipt should be recorded");
    let batch_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("work check batch receipt should be recorded");

    assert!(tool_receipt["worktree_fingerprint"].is_null());
    assert_eq!(tool_receipt["changed_paths"], json!([]));
    assert!(tool_receipt["changed_path_count"].is_null());
    assert_eq!(tool_receipt["diff_stat"]["files"], 0);
    assert!(batch_receipt["worktree_fingerprint"].as_str().is_some());
    assert_eq!(batch_receipt["changed_paths"], json!(["outside-agent.txt"]));
    assert_eq!(batch_receipt["changed_path_count"], 1);
    assert_eq!(batch_receipt["changed_paths_truncated"], false);
    assert!(batch_receipt["changed_paths_digest"].as_str().is_some());
    assert_eq!(
        batch_receipt["args"]["receipt_ids"][0],
        tool_receipt["id"].as_str().unwrap()
    );
}

#[test]
fn failed_work_check_records_metadata_on_batch_and_stops_later_tools() {
    let temp = tempdir().unwrap();
    write_fail_fast_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    fs::write(temp.path().join("outside-agent.txt"), "changed\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: vec!["jig.failing_check".into(), "jig.later_check".into()],
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.failing_check failed with status 7"));
    assert!(error.contains("command key: failing_check_command"));
    assert!(!temp.path().join("later-check-ran.txt").exists());

    let receipts_text = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .expect("failed work check should write child and batch receipts");
    let receipts = receipts_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let tool_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.failing_check")
        .expect("failed tool receipt should be recorded");
    let batch_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("failed work check batch receipt should be recorded");

    assert_eq!(tool_receipt["exit_status"], 7);
    assert!(tool_receipt["worktree_fingerprint"].is_null());
    assert_eq!(tool_receipt["changed_paths"], json!([]));
    assert!(tool_receipt["changed_path_count"].is_null());
    assert!(tool_receipt["changed_paths_digest"].is_null());
    assert_eq!(tool_receipt["diff_stat"]["files"], 0);

    assert_eq!(batch_receipt["exit_status"], 7);
    assert_eq!(
        batch_receipt["args"]["tools"],
        json!(["jig.failing_check", "jig.later_check"])
    );
    assert_eq!(
        batch_receipt["args"]["receipt_ids"],
        json!([tool_receipt["id"].as_str().unwrap()])
    );
    assert_eq!(batch_receipt["changed_paths"], json!(["outside-agent.txt"]));
    assert_eq!(batch_receipt["changed_path_count"], 1);
    assert_eq!(batch_receipt["changed_paths_truncated"], false);
    assert!(batch_receipt["changed_paths_digest"].as_str().is_some());
    assert!(batch_receipt["worktree_fingerprint"].as_str().is_some());
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["tool_name"] != "jig.later_check")
    );
}

#[test]
fn cancelled_collect_all_work_check_stops_unstarted_tools() {
    struct CancelWhenStarted(std::path::PathBuf);

    impl crate::execution::ExecutionObserver for CancelWhenStarted {}

    impl crate::execution::ExecutionCancellation for CancelWhenStarted {
        fn cancelled(&self) -> bool {
            self.0.exists()
        }
    }

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
first_check_command = "printf started > first-check-started; sleep 30"
later_check_command = "printf ran > later-check-ran"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.first_check"

[[work.gates]]
id = "later"
kind = "check"
tool = "jig.later_check"
"#,
        )
        .required_commands(["first_check_command", "later_check_command"])
        .tool(json!({
            "name": "jig.first_check",
            "kind": "command",
            "description": "Run the first fixture check.",
            "command": "first_check_command"
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Run the later fixture check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelWhenStarted(temp.path().join("first-check-started"));

    let error = crate::runtime::work::check_tools_collect_failures_with_observer(
        &ctx,
        "plan_1",
        vec!["jig.first_check".into(), "jig.later_check".into()],
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cancelled"), "{error}");
    assert!(!temp.path().join("later-check-ran").exists());
    let receipts = read_receipts(temp.path());
    let child = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.first_check")
        .expect("started cancelled check should record a child receipt");
    assert_eq!(child["evidence"]["status"], "cancelled");
    let batch = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("cancelled work check should record its batch receipt");
    assert_eq!(batch["args"]["receipt_ids"], json!([child["id"]]));
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["tool_name"] != "jig.later_check")
    );
}

#[test]
fn cancelled_collect_all_work_check_stops_after_a_native_tool() {
    #[derive(Default)]
    struct CancelAfterNativePhase {
        native_finished: bool,
    }

    impl crate::execution::ExecutionObserver for CancelAfterNativePhase {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            if matches!(
                event,
                crate::execution::ExecutionEvent::PhaseFinished {
                    label: crate::tool_defs::tool::CONTRACT_CHECK,
                    ..
                }
            ) {
                self.native_finished = true;
            }
        }
    }

    impl crate::execution::ExecutionCancellation for CancelAfterNativePhase {
        fn cancelled(&self) -> bool {
            self.native_finished
        }
    }

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
later_check_command = "printf ran > later-check-ran"
"#,
        )
        .required_commands(["later_check_command"])
        .tool(json!({
            "name": crate::tool_defs::tool::CONTRACT_CHECK,
            "kind": "native",
            "description": "Check the fixture contract."
        }))
        .tool(json!({
            "name": "jig.later_check",
            "kind": "command",
            "description": "Run the later fixture check.",
            "command": "later_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelAfterNativePhase::default();

    let error = crate::runtime::work::check_tools_collect_failures_with_observer(
        &ctx,
        "plan_1",
        vec![
            crate::tool_defs::tool::CONTRACT_CHECK.into(),
            "jig.later_check".into(),
        ],
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cancelled"), "{error}");
    assert!(!temp.path().join("later-check-ran").exists());
    let receipts = read_receipts(temp.path());
    assert!(
        receipts
            .iter()
            .any(|receipt| receipt["tool_name"] == crate::tool_defs::tool::CONTRACT_CHECK)
    );
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["tool_name"] != "jig.later_check")
    );
}

#[test]
fn timed_out_work_check_records_child_and_batch_failure_receipts() {
    let temp = tempdir().unwrap();
    write_timeout_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let started = std::time::Instant::now();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let receipts = read_receipts(temp.path());
    let child = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.timeout_check")
        .expect("timed-out configured check should record a child receipt");
    assert_eq!(child["exit_status"], 1);
    assert!(
        child["stderr_preview"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    assert_eq!(child["evidence"]["kind"], "supervised_command");
    assert_eq!(child["evidence"]["status"], "error");

    let batch = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("failed work check should record a batch receipt");
    assert_eq!(batch["exit_status"], 1);
    assert!(
        batch["stderr_preview"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    assert_eq!(batch["args"]["receipt_ids"][0], child["id"]);
}

#[test]
fn overflowed_legacy_work_check_retains_bounded_output_in_its_receipt() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[commands]
overflow_check_command = "printf 'stderr-prefix' >&2; printf 'stdout-prefix-and-more'"

[execution]
command_output_limit_bytes = 16

[[work.gates]]
id = "overflow"
kind = "check"
tool = "jig.overflow_check"
"#,
        )
        .required_commands(["overflow_check_command"])
        .tool(json!({
            "name": "jig.overflow_check",
            "kind": "command",
            "description": "Run configured overflow check.",
            "command": "overflow_check_command"
        }))
        .write();
    write_open_plan(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("capture limit"), "{error}");
    let receipts = read_receipts(temp.path());
    let child = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.overflow_check")
        .expect("overflowed configured check should record a child receipt");
    assert!(
        child["stdout_preview"]
            .as_str()
            .is_some_and(|stdout| stdout.starts_with("stdout-prefix"))
    );
    assert!(
        child["stderr_preview"].as_str().is_some_and(
            |stderr| stderr.contains("stderr-prefix") && stderr.contains("capture limit")
        )
    );
    assert_eq!(child["evidence"]["status"], "error");
}

#[test]
fn work_check_marks_batch_fingerprint_unknown_when_checks_mutate_worktree() {
    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["unknown_required"].as_array().unwrap().len(), 2);
    assert_eq!(gates["gates"][0]["status"], "unknown");
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("worktree changed during work check")
    );
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("before fingerprint")
    );
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("after fingerprint")
    );
}
