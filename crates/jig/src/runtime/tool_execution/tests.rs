use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::tempdir;

use super::*;
use crate::test_env::TestRepoBuilder;

fn write_v6_alias_repo(
    root: &std::path::Path,
    config_prefix: &str,
    authored_action: &str,
    action: serde_json::Value,
    tools: Vec<serde_json::Value>,
) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
{config_prefix}

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "repo"
root = "."

{authored_action}

[[repository.profiles]]
id = "verify"
targets = [{{ component = "repo", action = "check" }}]
"#
        ),
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "contract_version": 6,
            "tool_namespace": "jig",
            "required_commands": action["runner"]["command"]
                .as_str()
                .into_iter()
                .collect::<Vec<_>>(),
            "tools": tools,
            "components": [{"id": "repo", "root": "."}],
            "actions": [action],
            "profiles": [{
                "id": "verify",
                "targets": [{"component": "repo", "action": "check"}]
            }],
            "default_check_profile": "verify"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn native_output_is_bounded_at_the_common_execution_seam() {
    let limit = jig_owned_process::ProcessOutputLimits::default().stdout;
    let output = bound_native_output(NativeToolOutput {
        exit_status: 0,
        stdout: "x".repeat(limit + 1),
        stderr: "y".repeat(limit + 1),
    });

    assert!(output.stdout.starts_with(&"x".repeat(limit)));
    assert!(output.stdout.ends_with("[output truncated by Jig]\n"));
    assert!(output.stderr.ends_with("[output truncated by Jig]\n"));
}

#[test]
fn native_runner_rejects_an_elapsed_timeout_before_start() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = run_native_tool_with_control(
        &ctx,
        tool::CONTRACT_CHECK,
        None,
        &serde_json::json!({}),
        Duration::ZERO,
        &|| false,
    )
    .unwrap_err();

    assert!(matches!(
        error.downcast_ref::<jig_owned_process::OwnedProcessTreeError>(),
        Some(jig_owned_process::OwnedProcessTreeError::TimedOut)
    ));
}

#[test]
fn native_runner_preserves_cancellation_before_operation_start() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = run_native_tool_with_control(
        &ctx,
        tool::CONTRACT_CHECK,
        None,
        &serde_json::json!({}),
        Duration::from_secs(30),
        &|| true,
    )
    .unwrap_err();

    assert!(matches!(
        error.downcast_ref::<jig_owned_process::OwnedProcessTreeError>(),
        Some(jig_owned_process::OwnedProcessTreeError::CancelledBeforeStart)
    ));
}

#[test]
fn cancellation_after_an_in_process_mutation_does_not_reclassify_completion() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
backend_language = "go"
go_database = "postgres"
migration_dir = "migrations"
"#,
        )
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let probes = AtomicUsize::new(0);

    let output = run_native_tool_with_control(
        &ctx,
        tool::MIGRATION_ADD,
        None,
        &serde_json::json!({args::NAME: "create_examples"}),
        Duration::from_secs(30),
        &|| probes.fetch_add(1, Ordering::SeqCst) > 0,
    )
    .unwrap();

    assert_eq!(output.exit_status, 0);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_dir(temp.path().join("migrations"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn v6_command_alias_uses_the_action_runner_contract() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("work")).unwrap();
    write_v6_alias_repo(
        temp.path(),
        r#"[commands]
alias_command = "printf '%s|%s\\n' \"$PWD\" \"$ALIAS_ENV\"""#,
        r#"[[repository.actions]]
target = { component = "repo", action = "check" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "alias_command", working_directory = "work", environment = { ALIAS_ENV = "from-action" } }
inputs = ["work/**"]
legacy_aliases = ["jig.compat_check"]"#,
        serde_json::json!({
            "target": {"component": "repo", "action": "check"},
            "intent": "check",
            "effects": ["read_only", "process"],
            "runner": {
                "kind": "command",
                "command": "alias_command",
                "working_directory": "work",
                "environment": {"ALIAS_ENV": "from-action"}
            },
            "inputs": ["work/**"],
            "legacy_aliases": ["jig.compat_check"]
        }),
        vec![serde_json::json!({
            "name": "jig.compat_check",
            "kind": "command",
            "description": "Compatibility check.",
            "command": "alias_command"
        })],
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = execute_manifest_tool_result_without_worktree_fingerprint(
        &ctx,
        "jig.compat_check",
        serde_json::json!({}),
        None,
    )
    .unwrap();

    assert_eq!(result["result"]["exit_status"], 0);
    assert_eq!(
        result["result"]["stdout"],
        format!(
            "{}|from-action\n",
            temp.path().join("work").canonicalize().unwrap().display()
        )
    );
}

#[test]
fn v6_native_alias_dispatches_the_action_operation() {
    let temp = tempdir().unwrap();
    write_v6_alias_repo(
        temp.path(),
        "harness_footprint = \"minimal\"",
        r#"[[repository.actions]]
target = { component = "repo", action = "check" }
intent = "check"
effects = ["read_only"]
runner = { kind = "native", operation = "jig.contract_check" }
inputs = [".jig.toml"]
legacy_aliases = ["jig.compat_contract"]"#,
        serde_json::json!({
            "target": {"component": "repo", "action": "check"},
            "intent": "check",
            "effects": ["read_only"],
            "runner": {"kind": "native", "operation": "jig.contract_check"},
            "inputs": [".jig.toml"],
            "legacy_aliases": ["jig.compat_contract"]
        }),
        vec![
            serde_json::json!({
                "name": "jig.compat_contract",
                "kind": "native",
                "description": "Compatibility contract check."
            }),
            serde_json::json!({
                "name": "jig.contract_check",
                "kind": "native",
                "description": "Contract check."
            }),
        ],
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let result = execute_manifest_tool_result_without_worktree_fingerprint(
        &ctx,
        "jig.compat_contract",
        serde_json::json!({}),
        None,
    )
    .unwrap();

    assert_eq!(result["result"]["exit_status"], 1, "{result:#}");
    assert!(
        result["result"]["stderr"]
            .as_str()
            .unwrap()
            .contains("Missing required jig tool definition: jig.bootstrap")
    );
    assert!(
        !result["result"]["stderr"]
            .as_str()
            .unwrap()
            .contains("Unsupported native tool: jig.compat_contract")
    );
}

#[test]
fn v6_manifest_tools_without_an_action_alias_fail_closed() {
    let temp = tempdir().unwrap();
    write_v6_alias_repo(
        temp.path(),
        r#"harness_footprint = "minimal"

[commands]
alias_command = "printf 'must not run\n'""#,
        r#"[[repository.actions]]
target = { component = "repo", action = "check" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "alias_command" }
legacy_aliases = ["jig.compat_check"]"#,
        serde_json::json!({
            "target": {"component": "repo", "action": "check"},
            "intent": "check",
            "effects": ["read_only", "process"],
            "runner": {"kind": "command", "command": "alias_command"},
            "legacy_aliases": ["jig.compat_check"]
        }),
        vec![serde_json::json!({
            "name": "jig.bootstrap",
            "kind": "command",
            "description": "Unmapped compatibility tool.",
            "command": "alias_command"
        })],
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = execute_manifest_tool_result_without_worktree_fingerprint(
        &ctx,
        "jig.bootstrap",
        serde_json::json!({}),
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("does not resolve to a repository action through legacy_aliases"),
        "{error}"
    );

    let contract_error = crate::policy::validate_contract(&ctx)
        .unwrap_err()
        .to_string();
    assert!(
        contract_error.contains(
            "Contract-v6 tool jig.bootstrap is not mapped to a repository action through legacy_aliases"
        ),
        "{contract_error}"
    );
}
