use super::*;

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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(&ctx, &catalog, jig_repository::PlanRunRequest::default())
        .unwrap();

    let execution = crate::runtime::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
            && target.output_tail.as_ref().is_some_and(|tail| {
                tail.stderr
                    .contains("cancellation was requested before the target started")
            })
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
                comparison: None,
                explain: false,
                fail_fast,
            },
        ))
    };

    let collected = crate::runtime::dispatch(&ctx, request(false)).unwrap();
    assert_eq!(collected["ok"], false);
    assert_eq!(collected["results"].as_array().unwrap().len(), 2);
    assert!(temp.path().join("later-ran.txt").exists());

    fs::remove_file(temp.path().join("later-ran.txt")).unwrap();
    let stopped = crate::runtime::dispatch(&ctx, request(true)).unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["results"].as_array().unwrap().len(), 1);
    assert!(!temp.path().join("later-ran.txt").exists());
    let failed = &stopped["run"]["targets"][0];
    assert_eq!(failed["exit_code"], 7, "{failed:#}");
    assert_eq!(failed["output_tail"]["stderr"], "failed\n", "{failed:#}");
    let skipped = &stopped["run"]["targets"][1];
    assert_eq!(skipped["conclusion"], "skipped", "{skipped:#}");
    assert!(
        skipped["output_tail"]["stderr"]
            .as_str()
            .unwrap()
            .contains("fail-fast was requested"),
        "{skipped:#}"
    );
}

#[test]
fn plain_v6_named_test_routes_through_repository_planning_for_every_component() {
    #[derive(Default)]
    struct RecordingObserver {
        output: Vec<u8>,
        started: bool,
        finished: bool,
    }

    impl jig_execution::ExecutionObserver for RecordingObserver {
        fn event(&mut self, event: jig_execution::ExecutionEvent<'_>) {
            match event {
                jig_execution::ExecutionEvent::PhaseStarted { .. } => self.started = true,
                jig_execution::ExecutionEvent::Output { bytes, .. } => {
                    self.output.extend_from_slice(bytes);
                }
                jig_execution::ExecutionEvent::PhaseFinished { .. } => self.finished = true,
                jig_execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl jig_execution::ExecutionCancellation for RecordingObserver {}

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
        RuntimeCommand::Check(crate::command::CheckCommand::Named(
            crate::command::NamedCheck::TEST,
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
        output["source_observations"]["count"], 2,
        "one parallel layer requires one shared before/after source observation"
    );
}

#[test]
fn native_contract_check_writes_no_receipt() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();

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
            crate::cli::CheckCommand::Named(crate::cli::NamedCheckCommand::Contract(
                crate::cli::CheckTargetOpts {
                    selectors: Vec::new(),
                },
            )),
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert!(output.get("receipt_id").is_none());
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}
