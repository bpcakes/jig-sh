// agentic-loc-exception: MCP lifecycle tests share one server fixture and durable repository-run polling helpers.

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

fn add_v6_mutating_effect_action(root: &std::path::Path, action: &str, effect: &str) {
    let command_key = format!("{}_command", action.replace('-', "_"));
    let output_path = format!("{action}-mutation.txt");
    let effects = if effect == "process" {
        vec![effect]
    } else {
        vec![effect, "process"]
    };
    let toml_effects = effects
        .iter()
        .map(|effect| format!("\"{effect}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let config = config
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            &format!(
                "api_test_command = \"printf 'api tests passed\\n'\"\n{command_key} = \"printf mutated > {output_path}\""
            ),
        )
        .replace(
            "[[repository.profiles]]",
            &format!(
                r#"[[repository.actions]]
target = {{ component = "api", action = "{action}" }}
intent = "operate"
effects = [{toml_effects}]
runner = {{ kind = "command", command = "{command_key}" }}
inputs = ["api/**"]

[[repository.profiles]]"#
            ),
        );
    fs::write(config_path, config).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!(command_key));
    manifest["actions"].as_array_mut().unwrap().push(json!({
        "target": {"component": "api", "action": action},
        "intent": "operate",
        "effects": effects,
        "runner": {"kind": "command", "command": command_key},
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
            "default_branch = \"main\"\nschema_dump_enabled = true",
        )
        .replace(
            "[commands]",
            &format!("[commands]\napi_schema_dump_command = \"{escaped_command}\""),
        )
        .replace("adapters = [\"go\"]", "adapters = [\"go\", \"sqlx\"]")
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

[[repository.actions]]
target = {{ component = "api", action = "schema-dump" }}
intent = "generate"
effects = ["worktree", "process"]
runner = {{ kind = "command", command = "api_schema_dump_command" }}
inputs = ["api/**"]

[[repository.profiles]]"#
            ),
        );
    fs::write(config_path, config).unwrap();

    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("api_schema_dump_command"));
    manifest["components"][0]["adapters"] = json!(["go", "sqlx"]);
    manifest["actions"].as_array_mut().unwrap().extend([
        json!({
            "target": {"component": "api", "action": "schema"},
            "intent": "check",
            "effects": ["worktree", "process"],
            "runner": {"kind": "native", "operation": "jig.schema_check"},
            "inputs": ["api/**"],
            "timeout_seconds": timeout_seconds
        }),
        json!({
            "target": {"component": "api", "action": "schema-dump"},
            "intent": "generate",
            "effects": ["worktree", "process"],
            "runner": {"kind": "command", "command": "api_schema_dump_command"},
            "inputs": ["api/**"]
        }),
    ]);
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
    assert!(
        lease_path.exists(),
        "terminal worker lease must keep a stable inode"
    );

    let terminal_cancel = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_eq!(terminal_cancel["cancellation_requested"], false);
    assert_eq!(terminal_cancel["worker_signalled"], false);
    assert_eq!(terminal_cancel["run"]["conclusion"], "success");
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
fn mcp_work_check_refreshes_repository_authority_after_server_start() {
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
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"printf 'refreshed authority\\n'\"",
    );
    fs::write(config_path, config).unwrap();

    let check = call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id": "plan_1"})).unwrap();

    assert_eq!(check["ok"], true, "{check:#}");
    assert_eq!(
        check["results"][0]["response"]["result"]["stdout"],
        "refreshed authority\n"
    );
}

#[test]
fn mcp_work_check_rejects_repository_lease_contention_without_blocking_the_transport() {
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
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let root = temp.path().to_owned();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel(0);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let ctx = RepoContext::load_from(&root).unwrap();
        ready_tx.send(()).unwrap();
        start_rx.recv().unwrap();
        let result = call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id": "plan_1"}))
            .map_err(|error| error.to_string());
        let _ = result_tx.send(result);
    });

    ready_rx.recv().unwrap();
    start_tx.send(()).unwrap();
    let timely = result_rx.recv_timeout(Duration::from_secs(5));
    drop(held);
    let result = match timely {
        Ok(result) => result,
        Err(error) => {
            let _ = result_rx.recv_timeout(Duration::from_secs(5));
            worker.join().unwrap();
            panic!("MCP work check blocked the request loop on repository contention: {error}");
        }
    };
    worker.join().unwrap();

    let error = result.unwrap_err();
    assert!(
        error.contains("repository execution is busy with an incompatible run"),
        "{error}"
    );
}

#[test]
fn mcp_explicit_work_check_rejects_repository_lease_contention_without_blocking_the_transport() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replacen(
        "inputs = [\"api/**\"]",
        "inputs = [\"api/**\"]\nlegacy_aliases = [\"jig.api_test\"]",
        1,
    );
    fs::write(config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["tools"].as_array_mut().unwrap().push(json!({
        "name": "jig.api_test",
        "kind": "command",
        "description": "Run API tests.",
        "command": "api_test_command"
    }));
    manifest["actions"][0]["legacy_aliases"] = json!(["jig.api_test"]);
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let root = temp.path().to_owned();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel(0);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let ctx = RepoContext::load_from(&root).unwrap();
        ready_tx.send(()).unwrap();
        start_rx.recv().unwrap();
        let result = call_tool(
            &ctx,
            tool::WORK_CHECK,
            json!({"plan_id": "plan_1", "tools": ["jig.api_test"]}),
        )
        .map_err(|error| error.to_string());
        let _ = result_tx.send(result);
    });

    ready_rx.recv().unwrap();
    start_tx.send(()).unwrap();
    let timely = result_rx.recv_timeout(Duration::from_secs(5));
    drop(held);
    let result = match timely {
        Ok(result) => result,
        Err(error) => {
            let _ = result_rx.recv_timeout(Duration::from_secs(5));
            worker.join().unwrap();
            panic!("explicit MCP work check blocked the request loop on contention: {error}");
        }
    };
    worker.join().unwrap();

    let error = result.unwrap_err();
    assert!(
        error.contains("repository execution is busy with an incompatible run"),
        "{error}"
    );
}

#[test]
fn mcp_work_refine_rejects_final_check_lease_contention_without_blocking_the_transport() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(
        temp.path(),
        r#"
[[work.gates]]
id = "review"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
severity = "high"
required = true

[[work.gates]]
id = "api-tests"
kind = "evidence"
target = "api:test"
"#,
    );
    init_git_repo(temp.path());
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "-o" ]; then
    out="$arg"
  fi
  prev="$arg"
done
printf '{"summary":"clean","findings":[]}\n' > "$out"
"#,
    );
    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let held = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::Worktree],
    )
    .unwrap();
    let root = temp.path().to_owned();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
    let (start_tx, start_rx) = std::sync::mpsc::sync_channel(0);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let ctx = RepoContext::load_from(&root).unwrap();
        ready_tx.send(()).unwrap();
        start_rx.recv().unwrap();
        let result = call_tool(
            &ctx,
            tool::WORK_REFINE,
            json!({"plan_id": "plan_1", "max_iterations": 1}),
        )
        .map_err(|error| error.to_string());
        let _ = result_tx.send(result);
    });

    ready_rx.recv().unwrap();
    start_tx.send(()).unwrap();
    let timely = result_rx.recv_timeout(Duration::from_secs(5));
    drop(held);
    let result = match timely {
        Ok(result) => result,
        Err(error) => {
            let _ = result_rx.recv_timeout(Duration::from_secs(5));
            worker.join().unwrap();
            panic!("MCP work refine blocked on final check contention: {error}");
        }
    };
    worker.join().unwrap();

    let error = result.unwrap_err();
    assert!(
        error.contains("repository execution is busy with an incompatible run"),
        "{error}"
    );
}

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

mod repository_execution;
