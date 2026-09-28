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
            comparison: None,
        },
    )
    .unwrap();
    let (run, _lease) =
        crate::runtime::run_execution::start_check_run(&ctx, &catalog, plan).unwrap();
    fs::write(temp.path().join("api/drift.txt"), "stable drift\n").unwrap();

    let execution = crate::runtime::run_execution::execute_started_check_run(
        &ctx,
        &catalog,
        run,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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
            comparison: None,
        },
        Default::default(),
    )
    .unwrap();
    let (run, _lease) =
        crate::runtime::run_execution::start_check_run(&ctx, &catalog, plan).unwrap();
    fs::write(temp.path().join("api/drift.txt"), "stable drift\n").unwrap();

    let execution = crate::runtime::run_execution::execute_started_check_run(
        &ctx,
        &catalog,
        run,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
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

#[test]
fn mcp_plan_receives_cancellation_after_outer_dispatch() {
    struct CancelInPlan(std::sync::atomic::AtomicUsize);
    impl crate::execution::ExecutionObserver for CancelInPlan {}
    impl crate::execution::ExecutionCancellation for CancelInPlan {
        fn cancelled(&self) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0
        }
    }
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelInPlan(std::sync::atomic::AtomicUsize::new(0));
    let error = call_tool_with_observer(&ctx, "jig.plan_run", json!({"selectors":["api:test"]}), &mut observer).unwrap_err();
    assert!(error.to_string().contains("cancelled"), "{error:#}");
    assert!(observer.0.load(std::sync::atomic::Ordering::SeqCst) >= 2);
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
    let current_fingerprint = crate::git_receipts::repository_source_snapshot(ctx.root())
        .unwrap()
        .worktree_fingerprint;
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
