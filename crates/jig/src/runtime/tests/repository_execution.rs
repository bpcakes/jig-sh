// agentic-loc-exception: repository execution evidence and cancellation cases share one durable-run fixture boundary.

use super::*;

#[test]
fn empty_freshly_planned_check_rejects_source_drift_before_creating_a_run() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[repository]\ndefault_check_profile = \"verify\"",
        "[repository]\ndefault_check_profile = \"verify\"\naffected_ignore = [\"README.md\"]",
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["affected_ignore"] = json!(["README.md"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    fs::write(temp.path().join("README.md"), "documentation only\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            affected_base: Some("HEAD".into()),
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    assert!(plan.targets.is_empty());
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();

    let mut observer = crate::execution::NoopExecutionObserver;
    let error = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap_err();

    assert!(error.to_string().contains("source changed after planning"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());
}

#[test]
fn freshly_planned_check_rejects_authority_that_changed_before_planning() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let changed = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'changed command\\n'\"",
    );
    fs::write(&config_path, changed).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let mut observer = crate::execution::NoopExecutionObserver;
    let error = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap_err();

    assert!(error.to_string().contains("execution authority changed"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());
}

#[test]
fn accepted_empty_check_cannot_complete_under_changed_manifest_authority() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[repository]\ndefault_check_profile = \"verify\"",
        "[repository]\ndefault_check_profile = \"verify\"\naffected_ignore = [\"README.md\"]",
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["affected_ignore"] = json!(["README.md"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    fs::write(temp.path().join("README.md"), "documentation only\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            affected_base: Some("HEAD".into()),
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();
    assert!(plan.targets.is_empty());
    let (run, _lease) = super::run_execution::start_check_run(&ctx, &catalog, plan, None).unwrap();
    manifest["jig_version"] = json!("changed-after-acceptance");
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let run_id = run.result.run_id.clone();
    let error = super::run_execution::execute_started_check_run(
        &ctx,
        &catalog,
        run,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &|| Ok(false),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("execution authority changed"), "{error}");
    assert_eq!(
        crate::state::run_by_id(&ctx, &run_id)
            .unwrap()
            .result
            .conclusion,
        Some(jig_contract::RunConclusion::Blocked)
    );
}

#[test]
fn target_that_changes_manifest_authority_cannot_report_success() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let changed = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'not-json\\n' > .agent/jig-contract.json\"",
    );
    fs::write(&config_path, changed).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &|| false,
    )
    .unwrap();

    let target = &execution.run.result.targets[0];
    assert_eq!(
        target.conclusion,
        Some(jig_contract::RunConclusion::Blocked)
    );
    assert!(
        target
            .findings
            .iter()
            .any(|finding| finding.source.as_deref() == Some("execution_authority"))
    );
}

#[test]
fn repository_command_target_fails_on_the_configured_output_limit() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'output larger than the configured bound'\"",
    ) + "\n[execution]\ncommand_output_limit_bytes = 16\n";
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| false,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Failure)
    );
    let target = &execution.run.result.targets[0];
    assert_eq!(
        target.conclusion,
        Some(jig_contract::RunConclusion::Failure)
    );
    assert_eq!(
        target.findings[0].source.as_deref(),
        Some("execution_policy")
    );
    assert!(target.findings[0].message.contains("16 byte stdout"));
    assert_eq!(
        execution.results[0]["response"]["result"]["stdout"],
        "output larger th"
    );
}

#[test]
fn repository_command_target_uses_the_configured_default_timeout() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"sleep 30\"",
    ) + "\n[execution]\ncommand_timeout_seconds = 1\n";
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..crate::repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &|| false,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::TimedOut)
    );
    assert_eq!(
        execution.run.result.targets[0].conclusion,
        Some(jig_contract::RunConclusion::TimedOut)
    );
}

#[test]
fn repository_affected_check_rejects_legacy_contracts_before_git_resolution() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = super::super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: Some("missing-ref".into()),
                explain: true,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, true),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("contract version 6 or later"));
}

#[test]
fn independent_read_only_layer_targets_execute_concurrently() {
    #[derive(Default)]
    struct RecordingObserver {
        started: Vec<String>,
        finished: Vec<String>,
    }

    impl crate::execution::ExecutionObserver for RecordingObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { label, .. } => {
                    self.started.push(label.to_owned());
                }
                crate::execution::ExecutionEvent::PhaseFinished { label, .. } => {
                    self.finished.push(label.to_owned());
                }
                crate::execution::ExecutionEvent::Output { .. }
                | crate::execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for RecordingObserver {}

    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"touch .agent/.cache/api-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/web-started ] && exit 0; sleep 0.01; done; exit 9\"",
        )
        .replace(
            "web_test_command = \"printf 'web tests passed\\n'\"",
            "web_test_command = \"touch .agent/.cache/web-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/api-started ] && exit 0; sleep 0.01; done; exit 9\"",
        );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    assert_eq!(plan.execution_layers.len(), 1);
    assert_eq!(plan.execution_layers[0].len(), 2);
    let mut observer = RecordingObserver::default();

    let execution = super::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: false,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Success)
    );
    assert_eq!(
        execution
            .run
            .result
            .targets
            .iter()
            .map(|target| target.target.to_string())
            .collect::<Vec<_>>(),
        ["api:test", "web:test"]
    );
    assert_eq!(observer.started.len(), 2);
    assert_eq!(observer.finished.len(), 2);
}

#[test]
fn repository_execution_records_cancelled_results_for_every_target() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
first_command = "printf 'first\n'"
second_command = "printf 'second\n'"

[work]
checks = ["jig.first", "jig.second"]
"#,
        )
        .required_commands(["first_command", "second_command"])
        .tool(json!({
            "name": "jig.first",
            "kind": "command",
            "description": "Run first.",
            "command": "first_command"
        }))
        .tool(json!({
            "name": "jig.second",
            "kind": "command",
            "description": "Run second.",
            "command": "second_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| true,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Cancelled)
    );
    assert_eq!(execution.run.result.targets.len(), 2);
    assert!(execution.run.result.targets.iter().all(|target| {
        target.status == jig_contract::RunStatus::Completed
            && target.conclusion == Some(jig_contract::RunConclusion::Cancelled)
            && target.receipt_id.is_some()
    }));
}

#[test]
fn repository_check_collects_failures_unless_fail_fast_is_explicit() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
failing_command = "printf 'failed\n' >&2; exit 7"
later_command = "printf 'later\n' > later-ran.txt"

[work]
checks = ["jig.a_fail", "jig.z_later"]
"#,
        )
        .required_commands(["failing_command", "later_command"])
        .tool(json!({
            "name": "jig.a_fail",
            "kind": "command",
            "description": "Fail first.",
            "command": "failing_command"
        }))
        .tool(json!({
            "name": "jig.z_later",
            "kind": "command",
            "description": "Run later.",
            "command": "later_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let request = |fail_fast| {
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast,
                tool: crate::command::ToolRequest::new(None, true),
            },
        ))
    };

    let collected = super::super::dispatch(&ctx, request(false)).unwrap();
    assert_eq!(collected["ok"], false);
    assert_eq!(collected["results"].as_array().unwrap().len(), 2);
    assert!(temp.path().join("later-ran.txt").exists());

    fs::remove_file(temp.path().join("later-ran.txt")).unwrap();
    let stopped = super::super::dispatch(&ctx, request(true)).unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["results"].as_array().unwrap().len(), 1);
    assert!(!temp.path().join("later-ran.txt").exists());
    let stopped_run_id = stopped["run"]["run_id"].as_str().unwrap();
    let skipped_target = &stopped["run"]["targets"][1]["target"];
    let skipped_receipt = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|receipt| receipt["run_id"] == stopped_run_id && receipt["target"] == *skipped_target)
        .unwrap();
    assert!(skipped_receipt["worktree_fingerprint"].is_null());
    assert!(
        skipped_receipt["worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("did not start")
    );
}

#[test]
fn command_tool_streams_both_outputs_through_execution_observer() {
    #[derive(Default)]
    struct RecordingObserver {
        output: Vec<u8>,
        started: bool,
        finished: bool,
    }

    impl crate::execution::ExecutionObserver for RecordingObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { .. } => self.started = true,
                crate::execution::ExecutionEvent::Output { bytes, .. } => {
                    self.output.extend_from_slice(bytes)
                }
                crate::execution::ExecutionEvent::PhaseFinished { .. } => self.finished = true,
                crate::execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for RecordingObserver {}

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'live stdout'; printf 'live stderr' >&2"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = RecordingObserver::default();

    let output = dispatch_with_observer(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Test(
            crate::command::ToolRequest::new(None, false),
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(output["result"]["stdout"], "live stdout");
    assert_eq!(output["result"]["stderr"], "live stderr");
    let observed = String::from_utf8(observer.output).unwrap();
    assert!(observed.contains("live stdout"));
    assert!(observed.contains("live stderr"));
    assert!(observer.started);
    assert!(observer.finished);
}

#[test]
fn plain_v6_named_test_routes_through_repository_planning_for_every_component() {
    #[derive(Default)]
    struct RecordingObserver {
        output: Vec<u8>,
        started: bool,
        finished: bool,
    }

    impl crate::execution::ExecutionObserver for RecordingObserver {
        fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
            match event {
                crate::execution::ExecutionEvent::PhaseStarted { .. } => self.started = true,
                crate::execution::ExecutionEvent::Output { bytes, .. } => {
                    self.output.extend_from_slice(bytes);
                }
                crate::execution::ExecutionEvent::PhaseFinished { .. } => self.finished = true,
                crate::execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl crate::execution::ExecutionCancellation for RecordingObserver {}

    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'live v6 stdout'; printf 'live v6 stderr' >&2\"",
    );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = RecordingObserver::default();

    let output = dispatch_with_observer(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Test(
            crate::command::ToolRequest::new(None, false),
        )),
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "live v6 stdout"
    );
    assert_eq!(
        output["results"][0]["response"]["result"]["stderr"],
        "live v6 stderr"
    );
    let observed = String::from_utf8(observer.output).unwrap();
    assert!(observed.contains("live v6 stdout"));
    assert!(observed.contains("live v6 stderr"));
    assert!(observer.started);
    assert!(observer.finished);
    assert_eq!(output["run"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(
        output["run"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|target| target["target"].clone())
            .collect::<Vec<_>>(),
        [
            json!({"component": "api", "action": "test"}),
            json!({"component": "web", "action": "test"}),
        ]
    );
    assert_eq!(
        output["source_observations"]["count"], 4,
        "parallel targets each require an independent before/after source observation"
    );
}

#[cfg(unix)]
#[test]
fn configured_command_output_limit_can_exceed_the_internal_protocol_bound() {
    const OUTPUT_BYTES: usize = 4 * 1024 * 1024 + 1;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(format!(
            r#"
rust_test_command = "head -c {OUTPUT_BYTES} /dev/zero"

[execution]
command_output_limit_bytes = {OUTPUT_BYTES}
"#,
        ))
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Test(
            crate::command::ToolRequest::new(None, false),
        )),
    )
    .unwrap();

    assert_eq!(
        output["result"]["stdout"].as_str().unwrap().len(),
        OUTPUT_BYTES
    );
}

#[test]
fn command_tool_honors_repository_timeout() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "sleep 30"

[execution]
command_timeout_seconds = 1
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let started = Instant::now();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Test(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: true,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
#[test]
fn native_tool_no_receipt_skips_receipt_append() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
bootstrap_command = "printf 'bootstrap\n'"
rust_fmt_check_command = "printf 'fmt\n'"
rust_clippy_command = "printf 'clippy\n'"
rust_test_command = "printf 'test\n'"
rust_test_locked_command = "printf 'test locked\n'"
"#,
        )
        .required_commands([
            "bootstrap_command",
            "rust_fmt_check_command",
            "rust_clippy_command",
            "rust_test_command",
            "rust_test_locked_command",
        ])
        .tool(json!({ "name": "jig.bootstrap", "kind": "command", "description": "Run bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": "jig.fmt_check", "kind": "command", "description": "Run fmt.", "command": "rust_fmt_check_command" }))
        .tool(json!({ "name": "jig.clippy", "kind": "command", "description": "Run clippy.", "command": "rust_clippy_command" }))
        .tool(json!({ "name": "jig.test", "kind": "command", "description": "Run tests.", "command": "rust_test_command" }))
        .tool(json!({ "name": "jig.test_locked", "kind": "command", "description": "Run locked tests.", "command": "rust_test_locked_command" }))
        .tool(json!({ "name": "jig.contract_check", "kind": "native", "description": "Run native contract check." }))
        .write();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Contract(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: true,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn failed_tool_error_remains_primary_when_receipt_append_fails() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'tool failed stdout\n'; printf 'tool failed stderr\n' >&2; exit 7"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    fs::write(temp.path().join(".agent/state"), "not a directory").unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Test(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}

#[test]
fn collect_result_keeps_failed_tool_context_when_receipt_append_fails() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'tool failed stdout\n'; printf 'tool failed stderr\n' >&2; exit 7"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run configured test command.",
            "command": "rust_test_command"
        }))
        .write();
    fs::write(temp.path().join(".agent/state"), "not a directory").unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = tool_execution::execute_manifest_tool_result_without_worktree_fingerprint(
        &ctx,
        crate::tool_defs::tool::TEST,
        json!({}),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}
