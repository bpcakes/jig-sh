use super::*;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::test_env::TestRepoBuilder;

fn wait_for_repository_run(ctx: &RepoContext, run_id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let inspected =
            call_tool(ctx, tool::INSPECT, json!({"kind": "run", "run_id": run_id})).unwrap();
        if inspected["result"]["run"]["result"]["status"] == "completed" {
            return inspected;
        }
        assert!(Instant::now() < deadline, "run {run_id} did not complete");
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_repository_output_schema(ctx: &RepoContext, name: &str, output: &Value) {
    let descriptor = crate::tool_defs::tool_descriptors(ctx.contract_version(), ctx.tool_specs())
        .into_iter()
        .find(|descriptor| descriptor["name"] == name)
        .unwrap();
    let validator = jsonschema::validator_for(&descriptor["outputSchema"]).unwrap();
    assert!(
        validator.is_valid(output),
        "{name} output did not match its schema: {output:#}"
    );
}

fn add_v6_generate_action(root: &std::path::Path) {
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let config = config
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"printf 'api tests passed\\n'\"\ngenerate_command = \"printf generated > generated.txt\"",
        )
        .replace(
            "[[repository.profiles]]",
            r#"[[repository.actions]]
target = { component = "api", action = "generate" }
intent = "generate"
effects = ["worktree", "process"]
runner = { kind = "command", command = "generate_command" }
inputs = ["api/**"]

[[repository.profiles]]"#,
        );
    fs::write(config_path, config).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("generate_command"));
    manifest["actions"].as_array_mut().unwrap().push(json!({
        "target": {"component": "api", "action": "generate"},
        "intent": "generate",
        "effects": ["worktree", "process"],
        "runner": {"kind": "command", "command": "generate_command"},
        "inputs": ["api/**"]
    }));
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn add_v6_native_schema_action(root: &std::path::Path, command: &str, timeout_seconds: u64) {
    let escaped_command = command.replace('\\', "\\\\").replace('"', "\\\"");
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "default_branch = \"main\"",
            &format!("default_branch = \"main\"\nschema_dump_command = \"{escaped_command}\""),
        )
        .replace(
            "[[repository.profiles]]",
            &format!(
                r#"[[repository.actions]]
target = {{ component = "api", action = "schema" }}
intent = "check"
effects = ["worktree", "process"]
runner = {{ kind = "native", operation = "jig.schema_check" }}
inputs = ["api/**"]
timeout_seconds = {timeout_seconds}

[[repository.profiles]]"#
            ),
        );
    fs::write(config_path, config).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["actions"].as_array_mut().unwrap().push(json!({
        "target": {"component": "api", "action": "schema"},
        "intent": "check",
        "effects": ["worktree", "process"],
        "runner": {"kind": "native", "operation": "jig.schema_check"},
        "inputs": ["api/**"],
        "timeout_seconds": timeout_seconds
    }));
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn add_v6_native_migration_action(root: &std::path::Path) {
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "default_branch = \"main\"",
            "default_branch = \"main\"\nmigration_dir = \"migrations\"",
        )
        .replace(
            "adapters = [\"go\"]",
            "adapters = [\"go\", \"go-postgres\"]",
        )
        .replace(
            "[[repository.profiles]]",
            r#"[[repository.actions]]
target = { component = "api", action = "migration-add" }
intent = "generate"
effects = ["worktree", "process"]
runner = { kind = "native", operation = "jig.migration_add" }
inputs = ["migrations/**"]

[[repository.profiles]]"#,
        );
    fs::write(config_path, config).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["components"][0]["adapters"] = json!(["go", "go-postgres"]);
    manifest["actions"].as_array_mut().unwrap().push(json!({
        "target": {"component": "api", "action": "migration-add"},
        "intent": "generate",
        "effects": ["worktree", "process"],
        "runner": {"kind": "native", "operation": "jig.migration_add"},
        "inputs": ["migrations/**"]
    }));
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn mcp_v6_advertises_bounded_repository_tools_and_v5_keeps_manifest_tools() {
    let v6 = tempdir().unwrap();
    write_v6_evidence_fixture_repo(v6.path(), "");
    let v6_manifest_path = v6.path().join(".agent/jig-contract.json");
    let mut v6_manifest: Value =
        serde_json::from_str(&fs::read_to_string(&v6_manifest_path).unwrap()).unwrap();
    v6_manifest["tools"] = json!([{
        "name": tool::TEST,
        "kind": "command",
        "description": "Compatibility API test alias.",
        "command": "api_test_command"
    }]);
    fs::write(
        &v6_manifest_path,
        serde_json::to_string_pretty(&v6_manifest).unwrap(),
    )
    .unwrap();
    let v6_ctx = RepoContext::load_from(v6.path()).unwrap();
    let v6_descriptors =
        crate::tool_defs::tool_descriptors(v6_ctx.contract_version(), v6_ctx.tool_specs());
    let v6_names = v6_descriptors
        .iter()
        .filter_map(|descriptor| descriptor["name"].as_str())
        .collect::<Vec<_>>();

    for name in [
        tool::INSPECT,
        tool::PLAN_RUN,
        tool::EXECUTE_RUN,
        tool::CANCEL_RUN,
    ] {
        let descriptor = v6_descriptors
            .iter()
            .find(|descriptor| descriptor["name"] == name)
            .unwrap();
        assert!(descriptor.get("inputSchema").is_some());
        assert!(descriptor.get("outputSchema").is_some());
    }
    assert!(!v6_names.contains(&tool::TEST));
    assert!(
        call_tool(&v6_ctx, tool::TEST, json!({}))
            .unwrap_err()
            .to_string()
            .contains("Unsupported tool")
    );

    let v5 = tempdir().unwrap();
    write_fixture_repo(v5.path());
    let v5_ctx = RepoContext::load_from(v5.path()).unwrap();
    let v5_names =
        crate::tool_defs::tool_descriptors(v5_ctx.contract_version(), v5_ctx.tool_specs())
            .into_iter()
            .filter_map(|descriptor| descriptor["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();

    assert!(v5_names.iter().any(|name| name == "jig.custom_check"));
    assert!(!v5_names.iter().any(|name| name == tool::PLAN_RUN));
}

#[test]
fn mcp_repository_plan_execute_and_inspect_share_durable_run_state() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let workspace = call_tool(&ctx, tool::INSPECT, json!({"kind": "workspace"})).unwrap();
    assert_repository_output_schema(&ctx, tool::INSPECT, &workspace);
    assert_eq!(
        workspace["result"]["components"].as_array().unwrap().len(),
        2
    );
    let mut invalid_workspace = workspace;
    invalid_workspace["result"]["unexpected"] = json!(true);
    let inspect_descriptor =
        crate::tool_defs::tool_descriptors(ctx.contract_version(), ctx.tool_specs())
            .into_iter()
            .find(|descriptor| descriptor["name"] == tool::INSPECT)
            .unwrap();
    assert!(
        !jsonschema::validator_for(&inspect_descriptor["outputSchema"])
            .unwrap()
            .is_valid(&invalid_workspace)
    );

    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"profile": "verify"})).unwrap();
    assert_repository_output_schema(&ctx, tool::PLAN_RUN, &planned);
    assert_eq!(planned["plan"]["targets"].as_array().unwrap().len(), 2);

    let executed = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap();
    assert_repository_output_schema(&ctx, tool::EXECUTE_RUN, &executed);
    assert_eq!(executed["accepted"], true);
    assert_eq!(executed["status"], "queued");
    let run_id = executed["run_id"].as_str().unwrap();

    let inspected = wait_for_repository_run(&ctx, run_id);
    assert_repository_output_schema(&ctx, tool::INSPECT, &inspected);
    assert_eq!(
        inspected["result"]["run"]["result"]["conclusion"],
        "success"
    );
    assert_eq!(
        inspected["result"]["run"]["result"]["targets"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!crate::runtime::mcp_repository::is_live_run_registered(
        &ctx, run_id
    ));
    let lease_path = temp
        .path()
        .join(".agent/.cache/run-leases")
        .join(format!("{run_id}.lock"));
    let lease_cleanup_deadline = Instant::now() + Duration::from_secs(1);
    while lease_path.exists() && Instant::now() < lease_cleanup_deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !lease_path.exists(),
        "terminal worker lease was not removed"
    );

    let terminal_cancel = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_eq!(terminal_cancel["cancellation_requested"], false);
    assert_eq!(terminal_cancel["worker_signalled"], false);
    assert_eq!(terminal_cancel["run"]["conclusion"], "success");
}

#[test]
fn mcp_inspect_reconciles_a_run_whose_worker_lease_disappeared() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let plan: jig_contract::RunPlan = serde_json::from_value(planned["plan"].clone()).unwrap();
    let (abandoned, abandoned_lease) = crate::state::start_run(&ctx, plan, None).unwrap();
    drop(abandoned_lease);

    let inspected = call_tool(
        &ctx,
        tool::INSPECT,
        json!({"kind": "run", "run_id": abandoned.result.run_id}),
    )
    .unwrap();

    assert_eq!(inspected["result"]["run"]["result"]["status"], "completed");
    assert_eq!(
        inspected["result"]["run"]["result"]["conclusion"],
        "blocked"
    );
    assert!(
        inspected["result"]["run"]["result"]["targets"][0]["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("worker lease is no longer held")
    );
}

#[test]
fn mcp_repository_effectful_action_requires_exact_plan_approval() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_generate_action(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"]})).unwrap();
    assert_eq!(planned["plan"]["effects"], json!(["worktree", "process"]));

    let error = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("approved_effects {Worktree}"), "{error}");

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
}

#[test]
fn mcp_repository_planning_refreshes_catalog_after_server_start() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    add_v6_generate_action(temp.path());

    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"]})).unwrap();

    assert_eq!(
        planned["plan"]["targets"][0]["target"]["action"],
        "generate"
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
    let mut manifest = fs::read_to_string(&manifest_path).unwrap();
    manifest.push('\n');
    fs::write(manifest_path, manifest).unwrap();

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
fn mcp_repository_cancel_is_cooperative_idempotent_and_cleans_registry() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(
            "printf 'api tests passed\\n'",
            "printf 'started\\n'; sleep 30; printf 'done\\n' > api-finished.txt",
        ),
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
    let run_id = accepted["run_id"].as_str().unwrap();

    let cancelled = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_repository_output_schema(&ctx, tool::CANCEL_RUN, &cancelled);
    assert_eq!(cancelled["cancellation_requested"], true);
    assert_eq!(cancelled["worker_signalled"], true);

    let terminal = wait_for_repository_run(&ctx, run_id);
    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    let registry_deadline = Instant::now() + Duration::from_secs(1);
    while crate::runtime::mcp_repository::is_live_run_registered(&ctx, run_id) {
        assert!(
            Instant::now() < registry_deadline,
            "completed run remained in the live registry"
        );
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("api-finished.txt").exists());

    let repeated = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_eq!(repeated["ok"], true);
    assert_eq!(repeated["worker_signalled"], false);
    assert_eq!(repeated["run"]["status"], "completed");
}

#[test]
fn mcp_repository_worker_observes_a_durable_external_cancel_request() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(
            "printf 'api tests passed\\n'",
            "printf 'started\\n'; sleep 30; printf 'done\\n' > api-finished.txt",
        ),
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
    let run_id = accepted["run_id"].as_str().unwrap();

    crate::state::request_run_cancel(&ctx, run_id).unwrap();
    let terminal = wait_for_repository_run(&ctx, run_id);

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("api-finished.txt").exists());
}

#[test]
fn mcp_native_schema_action_honors_timeout_and_cleans_its_process_tree() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_native_schema_action(
        temp.path(),
        "sleep 30; printf finished > schema-finished.txt",
        1,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:schema"]})).unwrap();
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
        terminal["result"]["run"]["result"]["conclusion"], "timed_out",
        "{terminal:#}"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("schema-finished.txt").exists());
}

#[test]
fn mcp_native_schema_action_honors_cancellation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_native_schema_action(
        temp.path(),
        "sleep 30; printf finished > schema-finished.txt",
        30,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:schema"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({
            "plan": planned["plan"].clone(),
            "approved_effects": ["worktree"]
        }),
    )
    .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap();

    let cancelled = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_eq!(cancelled["cancellation_requested"], true);
    let terminal = wait_for_repository_run(&ctx, run_id);

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "cancelled",
        "{terminal:#}"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("schema-finished.txt").exists());
}

#[test]
fn mcp_repository_rejects_unknown_arguments_stale_plans_and_v6_legacy_calls() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(
        &ctx,
        tool::INSPECT,
        json!({"kind": "workspace", "unexpected": true}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Invalid arguments for jig.inspect"));

    let mut planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    planned["plan"]["config_digest"] = json!("sha256:tampered");
    let error = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("repository configuration changed"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());

    let error = call_tool(&ctx, tool::TEST, json!({}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Unsupported tool"));
}

#[test]
fn mcp_call_dispatches_command_tool_declared_only_in_manifest() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command_key"], "custom_check_command");
    assert_eq!(output["result"]["stdout"], "manifest target ran\n");
}

#[test]
fn mcp_call_dispatches_command_tool_without_makefile() {
    let temp = tempdir().unwrap();
    write_command_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command_key"], "rust_test_command");
    assert_eq!(output["result"]["stdout"], "command tool ran\n");
    assert!(!temp.path().join("Makefile").exists());

    let receipts = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    let receipt = receipts.lines().last().unwrap();
    assert!(receipt.contains(r#""invoked_command_key":"rust_test_command""#));
}

#[test]
fn mcp_native_migration_add_creates_files() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "migrations"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.migration_add",
            "kind": "native",
            "description": "Add migration."
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.migration_add", json!({ "name": "create_users" })).unwrap();

    assert_eq!(output["ok"], true);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("create_users")
    );
    let entries = fs::read_dir(temp.path().join("migrations"))
        .unwrap()
        .count();
    assert_eq!(entries, 2);
}

#[test]
fn mcp_native_contract_check_validates_manifest() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --check"
rust_clippy_command = "cargo clippy"
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands([
            "bootstrap_command",
            "rust_fmt_check_command",
            "rust_clippy_command",
            "rust_test_command",
        ])
        .tool(json!({ "name": "jig.bootstrap", "kind": "command", "description": "Bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": "jig.fmt_check", "kind": "command", "description": "Format.", "command": "rust_fmt_check_command" }))
        .tool(json!({ "name": "jig.clippy", "kind": "command", "description": "Clippy.", "command": "rust_clippy_command" }))
        .tool(json!({ "name": "jig.test", "kind": "command", "description": "Test.", "command": "rust_test_command" }))
        .tool(json!({ "name": "jig.contract_check", "kind": "native", "description": "Contract check." }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.contract_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
}

#[test]
fn mcp_native_schema_check_detects_clean_schema_dump() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
schema_dump_enabled = true
rust_migration_dir = "migrations"
schema_dump_command = "mkdir -p docs/schema && printf 'stable\n' > docs/schema/tables.sql"
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command", "schema_dump_command"])
        .tool(json!({
            "name": "jig.schema_check",
            "kind": "native",
            "description": "Schema check."
        }))
        .write();
    for args in [
        ["init", "-q"].as_slice(),
        ["config", "user.name", "Fixture"].as_slice(),
        ["config", "user.email", "fixture@example.com"].as_slice(),
        ["add", "."].as_slice(),
        ["commit", "-m", "fixture", "-q"].as_slice(),
    ] {
        let status = Command::new("git")
            .current_dir(temp.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.schema_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["result"]["stdout"], "Schema dump is up to date.\n");
}

#[test]
fn mcp_exposes_read_only_agent_doctor_tool() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(codex_home.join("config.toml"), "").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = call_tool(&ctx, tool::AGENT_DOCTOR, json!({})).unwrap();

    assert_eq!(output["command"], "agent doctor");
    assert_eq!(output["codex"]["available"], true);
}

#[test]
fn mcp_does_not_expose_dev_or_proxy_commands() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    for name in [
        "jig.dev",
        "jig.proxy",
        "jig.proxy_start",
        "jig.proxy_cert_trust",
    ] {
        let error = call_tool(&ctx, name, json!({})).unwrap_err().to_string();
        assert!(error.contains("Unsupported tool"));
    }
}

#[test]
fn mcp_work_tools_deserialize_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(
        &ctx,
        tool::WORK_START,
        json!({
            "title": "Typed MCP request",
            "body": "Use serde for tool arguments"
        }),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert!(output["plan"]["plan_id"].as_str().is_some());
}

#[test]
fn mcp_work_append_rejects_blank_progress_without_mutating_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_path = ctx.plan_body_path("plan_1");
    let body_before = fs::read_to_string(&plan_path).unwrap();
    let state_before = crate::state::state_summary(&ctx).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_APPEND,
        json!({
            "plan_id": "plan_1",
            "body": " \n\t "
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Progress text must not be empty"));
    assert_eq!(fs::read_to_string(plan_path).unwrap(), body_before);
    assert_eq!(crate::state::state_summary(&ctx).unwrap(), state_before);
}

#[test]
fn mcp_work_tools_tolerate_null_optional_defaults() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let check = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1",
            "tools": null
        }),
    )
    .unwrap();
    let receipts = call_tool(
        &ctx,
        tool::WORK_RECEIPTS,
        json!({
            "failed_only": null,
            "limit": null
        }),
    )
    .unwrap();
    let evidence = call_tool(
        &ctx,
        tool::WORK_EVIDENCE,
        json!({
            "plan_id": null
        }),
    )
    .unwrap();

    assert_eq!(check["ok"], true);
    assert_eq!(receipts["ok"], true);
    assert_eq!(evidence["command"], "work evidence");
    assert!(!receipts["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_work_check_rejects_unknown_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_missing",
            "tools": null
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn mcp_work_tools_reject_invalid_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, tool::WORK_START, json!({ "body": "missing title" })).unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("Invalid work tool arguments"));
    assert!(error.contains("missing field `title`"));
}
