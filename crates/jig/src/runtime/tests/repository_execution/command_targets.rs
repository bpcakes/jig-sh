use super::*;

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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = crate::runtime::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            ..jig_repository::PlanRunRequest::default()
        },
    )
    .unwrap();

    let execution = crate::runtime::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
fn command_tool_streams_both_outputs_through_execution_observer() {
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
                    self.output.extend_from_slice(bytes)
                }
                jig_execution::ExecutionEvent::PhaseFinished { .. } => self.finished = true,
                jig_execution::ExecutionEvent::Heartbeat { .. } => {}
            }
        }
    }

    impl jig_execution::ExecutionCancellation for RecordingObserver {}

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
        RuntimeCommand::Check(crate::command::CheckCommand::Named(
            crate::command::NamedCheck::TEST,
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
        RuntimeCommand::Check(crate::command::CheckCommand::Named(
            crate::command::NamedCheck::TEST,
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
            crate::cli::CheckCommand::Named(crate::cli::NamedCheckCommand::Test(
                crate::cli::CheckTargetOpts {
                    selectors: Vec::new(),
                },
            )),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn failed_tool_reports_its_output_without_writable_state() {
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
            crate::cli::CheckCommand::Named(crate::cli::NamedCheckCommand::Test(
                crate::cli::CheckTargetOpts {
                    selectors: Vec::new(),
                },
            )),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(!error.contains("receipt"), "{error}");
}

#[test]
fn direct_tool_execution_keeps_failed_tool_context_without_writable_state() {
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
    let error = tool_execution::execute_manifest_tool_with_observer(
        &ctx,
        jig_commands::tool_defs::tool::TEST,
        json!({}),
        &mut jig_execution::NoopExecutionObserver,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(!error.contains("receipt"), "{error}");
}
