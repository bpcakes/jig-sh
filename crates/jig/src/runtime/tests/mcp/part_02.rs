#[test]
fn mcp_work_check_aggregates_a_late_non_contention_error_and_preserves_prior_results() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"mv .jig.toml .jig.toml.hidden; printf 'first passed\\n'\"",
        )
        .replacen(
            "inputs = [\"api/**\"]",
            "inputs = [\"api/**\"]\nlegacy_aliases = [\"jig.first_check\"]",
            1,
        )
        .replacen(
            "inputs = [\"web/**\"]",
            "inputs = [\"web/**\"]\nlegacy_aliases = [\"jig.second_check\"]",
            1,
        );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["tools"] = json!([
        {
            "name": "jig.first_check",
            "kind": "command",
            "description": "Run the first check.",
            "command": "api_test_command"
        },
        {
            "name": "jig.second_check",
            "kind": "command",
            "description": "Run the second check.",
            "command": "web_test_command"
        }
    ]);
    manifest["actions"][0]["legacy_aliases"] = json!(["jig.first_check"]);
    manifest["actions"][1]["legacy_aliases"] = json!(["jig.second_check"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1",
            "tools": ["jig.first_check", "jig.second_check"]
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Failed to refresh repository authority") && error.contains(".jig.toml"),
        "{error}"
    );
    let receipts = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let child = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.first_check")
        .expect("the successful earlier check must retain its receipt");
    let batch = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("the late execution error must retain the batch receipt");
    assert_eq!(child["stdout_preview"], "first passed\n");
    assert_eq!(batch["exit_status"], 1);
    assert_eq!(batch["args"]["receipt_ids"], json!([child["id"]]));
    assert!(
        batch["stderr_preview"]
            .as_str()
            .unwrap()
            .contains("Failed to refresh repository authority"),
        "{batch:#}"
    );
}

#[test]
fn mcp_work_start_and_status_refresh_repository_metadata_after_server_start() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("repo_name = \"demo\"", "repo_name = \"ExampleProject\"");
    assert!(
        config.contains("repo_name = \"ExampleProject\""),
        "{config}"
    );
    fs::write(config_path, config).unwrap();

    let status = call_tool(&ctx, tool::WORK_STATUS, json!({})).unwrap();
    let started = call_tool(
        &ctx,
        tool::WORK_START,
        json!({"title": "Example work", "body": "Validation plan."}),
    )
    .unwrap();

    assert_eq!(status["repo"]["name"], "ExampleProject", "{status:#}");
    assert_eq!(
        started["session"]["summary"]["repo_name"], "ExampleProject",
        "{started:#}"
    );
}

#[test]
fn mcp_agent_doctor_refreshes_marketplace_requirements_after_server_start() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    config.push_str("\n[agent_tooling.codex]\nmarketplaces = []\n");
    fs::write(config_path, config).unwrap();

    let doctor = call_tool(&ctx, tool::AGENT_DOCTOR, json!({})).unwrap();

    assert_eq!(doctor["ok"], true, "{doctor:#}");
    assert_eq!(doctor["codex"]["required"], false, "{doctor:#}");
    assert_eq!(doctor["codex"]["probe_skipped"], true, "{doctor:#}");
    assert!(doctor["marketplaces"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_and_ui_work_gates_refresh_manifest_authority_after_server_start() {
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
    call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id": "plan_1"})).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["jig_version"] = json!("semantic-contract-drift");
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let gates = call_tool(&ctx, tool::WORK_GATES, json!({"plan_id": "plan_1"})).unwrap();
    let ui_gates = super::super::work_gates_snapshot(&ctx, Some("plan_1".into())).unwrap();
    let finish_error = call_tool(
        &ctx,
        tool::WORK_FINISH,
        json!({
            "plan_id": "plan_1",
            "resolution": "done",
            "outcome": "success"
        }),
    )
    .unwrap_err()
    .to_string();

    assert_eq!(gates["overall"], "blocked", "{gates:#}");
    assert_eq!(gates["gates"][0]["status"], "stale", "{gates:#}");
    assert_eq!(ui_gates["gates"][0]["status"], "stale", "{ui_gates:#}");
    assert!(
        finish_error.contains("Stale: [api-tests]"),
        "{finish_error}"
    );
}

#[test]
fn mcp_repository_execution_rejects_manifest_only_contract_drift() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["jig_version"] = json!("semantic-contract-drift");
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("repository configuration changed"),
        "{error}"
    );
}

#[test]
fn mcp_repository_arguments_round_trip_from_wire_keys_into_native_actions() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_native_migration_action(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let planned = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({
            "selectors": ["api:migration-add"],
            "arguments": {
                "api:migration-add": {"name": "create_examples"}
            }
        }),
    )
    .unwrap();

    assert_repository_output_schema(&ctx, tool::PLAN_RUN, &planned);
    assert_eq!(
        planned["plan"]["targets"][0]["arguments"]["name"],
        "create_examples"
    );
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({
            "plan": planned["plan"].clone(),
            "approved_effects": ["worktree"]
        }),
    )
    .unwrap();
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "success",
        "{terminal:#}"
    );
    let migrations = fs::read_dir(temp.path().join("migrations"))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(migrations.len(), 1);
    assert!(
        migrations[0]
            .file_name()
            .to_string_lossy()
            .contains("create_examples")
    );
}

#[test]
fn mcp_repository_affected_plan_uses_the_shared_explainable_resolver() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    fs::write(
        temp.path().join("web/example.ts"),
        "export const example = 'changed';\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let planned = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({"selectors": ["test"], "affected_base": "HEAD"}),
    )
    .unwrap();

    assert_repository_output_schema(&ctx, tool::PLAN_RUN, &planned);
    assert_eq!(planned["plan"]["affected_base"], "HEAD");
    assert_eq!(planned["plan"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(
        planned["plan"]["targets"][0]["target"],
        json!({"component": "web", "action": "test"})
    );
    assert!(
        planned["plan"]["targets"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                == &json!({
                    "kind": "direct_input",
                    "path": "web/example.ts"
                }))
    );
}

#[test]
fn mcp_repository_affected_plan_does_not_treat_stable_dotenv_presence_as_a_change() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    fs::write(
        temp.path().join(".gitignore"),
        ".env\n.env.*\n**/.env\n**/.env.*\n",
    )
    .unwrap();
    init_git_repo(temp.path());
    fs::write(temp.path().join(".env"), "EXAMPLE_VALUE=local\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let planned = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({"selectors": ["test"], "affected_base": "HEAD"}),
    )
    .unwrap();

    assert!(planned["plan"]["targets"].as_array().unwrap().is_empty());
    assert!(
        planned["plan"]["execution_layers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn mcp_repository_failures_are_structured_terminal_conclusions() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace("printf 'api tests passed\\n'", "exit 7"),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();

    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap();
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());

    assert_eq!(accepted["ok"], true);
    assert_eq!(terminal["result"]["run"]["result"]["conclusion"], "failure");
}

#[test]
fn repository_execution_fails_a_read_only_action_that_mutates_the_worktree() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "printf 'api tests passed\\n'",
        "printf mutated > unexpected-mutation.txt",
    );
    fs::write(&config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap();

    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());

    assert_eq!(terminal["result"]["run"]["result"]["conclusion"], "failure");
    assert!(
        terminal["result"]["run"]["result"]["targets"][0]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["source"] == "effect_policy")
    );
}

#[test]
fn read_only_targets_use_a_fresh_epoch_after_worktree_targets() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_generate_action(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({"selectors": ["api:generate", "api:test"]}),
    )
    .unwrap();
    let planned_fingerprint = planned["plan"]["source"]["worktree_fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    let planned_test_input_digest = planned["plan"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["action"] == "test")
        .unwrap()["input_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({
            "plan": planned["plan"].clone(),
            "approved_effects": ["worktree"]
        }),
    )
    .unwrap();

    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "success",
        "{terminal:#}"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("generated.txt")).unwrap(),
        "generated"
    );
    let current_fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .unwrap();
    assert_ne!(planned_fingerprint, current_fingerprint);
    let test_result = terminal["result"]["run"]["result"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["target"]["action"] == "test")
        .unwrap();
    assert_ne!(test_result["input_digest"], planned_test_input_digest);
    let receipt = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|receipt| receipt["target"]["action"] == "test")
        .unwrap();
    assert_eq!(receipt["worktree_fingerprint"], current_fingerprint);
    assert_eq!(receipt["input_digest"], test_result["input_digest"]);
}

#[test]
fn read_only_target_rejects_stable_drift_after_plan_validation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "printf 'api tests passed\\n'",
        "printf ran > api/target-ran.txt",
    );
    fs::write(&config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:test".into()],
            profile: None,
            affected_base: None,
        },
    )
    .unwrap();
    let (run, _lease) =
        crate::runtime::run_execution::start_check_run(&ctx, &catalog, plan, None).unwrap();
    fs::write(temp.path().join("api/drift.txt"), "stable drift\n").unwrap();

    let execution = crate::runtime::run_execution::execute_started_check_run(
        &ctx,
        &catalog,
        run,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| Ok(false),
    )
    .unwrap();

    assert_eq!(
        execution.run.result.targets[0].conclusion,
        Some(jig_contract::RunConclusion::Blocked)
    );
    assert!(execution.run.result.targets[0].started_at_ms.is_none());
    assert!(
        execution.run.result.targets[0].findings[0]
            .message
            .contains("worktree changed after plan validation")
    );
    assert!(!temp.path().join("api/target-ran.txt").exists());
}

#[test]
fn worktree_target_rejects_stable_drift_before_it_starts() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_generate_action(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = crate::repository::plan_action_run(
        &ctx,
        &catalog,
        crate::repository::PlanRunRequest {
            selectors: vec!["api:generate".into()],
            profile: None,
            affected_base: None,
        },
        Default::default(),
    )
    .unwrap();
    let (run, _lease) =
        crate::runtime::run_execution::start_check_run(&ctx, &catalog, plan, None).unwrap();
    fs::write(temp.path().join("api/drift.txt"), "stable drift\n").unwrap();

    let execution = crate::runtime::run_execution::execute_started_check_run(
        &ctx,
        &catalog,
        run,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| Ok(false),
    )
    .unwrap();

    assert_eq!(
        execution.run.result.targets[0].conclusion,
        Some(jig_contract::RunConclusion::Blocked)
    );
    assert!(execution.run.result.targets[0].started_at_ms.is_none());
    assert!(!temp.path().join("generated.txt").exists());
}
