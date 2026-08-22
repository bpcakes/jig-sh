use super::*;

fn run_repository_target(ctx: &RepoContext, selector: &str) -> Value {
    dispatch(
        ctx,
        CommandKind::Check(crate::cli::CheckOpts {
            tool: crate::cli::ToolOpts {
                plan_id: Some("plan_1".into()),
                no_receipt: false,
            },
            profile: None,
            affected: None,
            explain: false,
            fail_fast: false,
            command: Some(crate::cli::CheckCommand::Selectors(vec![selector.into()])),
        }),
    )
    .unwrap()
}

#[test]
fn repository_command_actions_capture_bounded_process_output() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = run_repository_target(&ctx, "api:test");

    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "api tests passed\n"
    );
}

fn work_gates(ctx: &RepoContext) -> Value {
    dispatch(
        ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: Some("plan_1".into()),
        })),
    )
    .unwrap()
}

#[test]
fn target_evidence_gate_ignores_success_from_an_unrelated_target() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "api-tests"
kind = "evidence"
target = "api:test"
"#,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    run_repository_target(&ctx, "web:test");
    let unrelated = work_gates(&ctx);

    assert_eq!(unrelated["overall"], "blocked");
    assert_eq!(unrelated["gates"][0]["status"], "missing");
    assert_eq!(unrelated["gates"][0]["target"], "api:test");
    assert_eq!(
        unrelated["gates"][0]["targets"][0]["target"]["component"],
        "api"
    );
    assert!(unrelated["gates"][0]["targets"][0]["receipt_id"].is_null());

    run_repository_target(&ctx, "api:test");
    let matching = work_gates(&ctx);

    assert_eq!(matching["overall"], "passed", "{matching:#}");
    assert_eq!(matching["gates"][0]["status"], "passed");
    assert_eq!(matching["gates"][0]["targets"][0]["freshness"], "fresh");
}

#[test]
fn profile_evidence_gate_requires_all_targets_from_one_run() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "verify"
kind = "evidence"
profile = "verify"
"#,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    run_repository_target(&ctx, "api:test");
    run_repository_target(&ctx, "web:test");
    let split_runs = work_gates(&ctx);

    assert_eq!(split_runs["overall"], "blocked");
    assert_eq!(split_runs["gates"][0]["status"], "missing");
    assert_eq!(
        split_runs["gates"][0]["targets"].as_array().unwrap().len(),
        2
    );

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert_eq!(checked["run"]["targets"].as_array().unwrap().len(), 2);

    let complete = work_gates(&ctx);
    assert_eq!(complete["overall"], "passed", "{complete:#}");
    assert_eq!(complete["gates"][0]["status"], "passed");
    let run_id = complete["gates"][0]["run_id"].as_str().unwrap();
    assert!(
        complete["gates"][0]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|target| target["run_id"].as_str() == Some(run_id))
    );
}

#[test]
fn evidence_gate_rejects_receipts_after_target_inputs_change() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "api-tests"
kind = "evidence"
target = "api:test"
"#,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    run_repository_target(&ctx, "api:test");

    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();
    let stale = work_gates(&ctx);

    assert_eq!(stale["overall"], "blocked");
    assert_eq!(stale["gates"][0]["status"], "stale");
    assert_eq!(stale["gates"][0]["targets"][0]["freshness"], "stale");
    assert!(
        stale["gates"][0]["targets"][0]["freshness_reason"]
            .as_str()
            .unwrap()
            .contains("input digest")
    );
}

#[test]
fn evidence_gate_rejects_receipts_from_a_different_repository_config() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "api-tests"
kind = "evidence"
target = "api:test"
"#,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    run_repository_target(&ctx, "api:test");

    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace("api tests passed", "api verification passed"),
    )
    .unwrap();
    let changed_ctx = RepoContext::load_from(temp.path()).unwrap();
    let stale = work_gates(&changed_ctx);

    assert_eq!(stale["gates"][0]["status"], "stale");
    assert_eq!(
        stale["gates"][0]["targets"][0]["freshness_reason"],
        "receipt was recorded for a different repository configuration"
    );
}

#[test]
fn contract_check_rejects_unknown_evidence_gate_profiles() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "unknown-profile"
kind = "evidence"
profile = "does-not-exist"
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::policy::contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(
        output
            .stderr
            .contains("Work gate 'unknown-profile': work evidence gate references unknown profile 'does-not-exist'")
    );
}

#[test]
fn contract_five_tool_gate_keeps_legacy_work_check_semantics() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
custom_check_command = "printf 'legacy check passed\n'"

[[work.gates]]
id = "legacy"
kind = "check"
tool = "jig.custom_check"
"#,
        )
        .required_commands(["custom_check_command"])
        .tool(json!({
            "name": "jig.custom_check",
            "kind": "command",
            "description": "Run the legacy configured check.",
            "command": "custom_check_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Test plan", "# Test plan\n").unwrap();
    init_git_repo(temp.path());

    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    let gates = work_gates(&ctx);

    assert_eq!(checked["checks"].as_array().unwrap().len(), 1);
    assert!(checked.get("run").is_none());
    assert_eq!(gates["overall"], "passed", "{gates:#}");
    assert_eq!(gates["gates"][0]["kind"], "check");
    assert_eq!(gates["gates"][0]["tool"], "jig.custom_check");
}

#[test]
fn failing_legacy_gate_does_not_prevent_evidence_targets_from_running() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "legacy"
kind = "check"
tool = "jig.failing_check"

[[work.gates]]
id = "api-tests"
kind = "evidence"
target = "api:test"
"#,
    );
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap()
        .replace(
            "web_test_command = \"printf 'web tests passed\\n'\"",
            "web_test_command = \"printf 'web tests passed\\n'\"\nfailing_check_command = \"printf 'legacy failed\\n' >&2; exit 7\"",
        )
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"printf evidence > evidence-target-ran.txt\"",
        );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("failing_check_command"));
    manifest["tools"].as_array_mut().unwrap().push(json!({
        "name": "jig.failing_check",
        "kind": "command",
        "description": "Fail the legacy check.",
        "command": "failing_check_command"
    }));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
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

    assert!(
        error.contains("jig.failing_check failed with status 7"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("evidence-target-ran.txt")).unwrap(),
        "evidence"
    );
    let receipts = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    assert!(receipts.contains(r#""target":{"component":"api","action":"test"}"#));
}
