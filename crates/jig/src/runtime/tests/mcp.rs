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

include!("mcp/part_02.rs");

mod repository_execution;
