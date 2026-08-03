use super::*;
use std::process::Command;

use crate::test_env::TestRepoBuilder;

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
