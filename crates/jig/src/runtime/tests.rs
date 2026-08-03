use std::fs;

use jig_vault::{SecretBytes, Vault};
use secrecy::{ExposeSecret, SecretString};
use tempfile::tempdir;

use common::*;

use crate::cli::CommandKind;
use crate::command::RuntimeCommand;
use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

use super::*;

mod agent;
mod common;
mod loops;
mod mcp;
mod work;

#[test]
fn dispatch_vault_run_injects_redacts_and_verifies_audit() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let vault_home = temp.path().join("vault");
    let passphrase = "correct horse battery staple";
    let _init_passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase);

    capture_vault_passphrase().unwrap();
    dispatch_vault(crate::command::VaultCommand::Init(
        crate::command::VaultInitRequest {
            vault: crate::command::VaultRuntimeOptions {
                home: Some(vault_home.clone()),
                ..Default::default()
            },
        },
    ))
    .unwrap();
    let vault = Vault::resolve(Some(vault_home.clone())).unwrap();
    let passphrase = SecretString::from(passphrase.to_string());
    vault
        .set_secret(
            &passphrase,
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    let _run_passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase.expose_secret());
    capture_vault_passphrase().unwrap();
    let output = dispatch_vault(crate::command::VaultCommand::Run(
        crate::command::VaultRunRequest {
            env: vec!["TOKEN=api_token".into()],
            files: Vec::new(),
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf 'token=%s\\n' \"$TOKEN\"; env".into(),
            ],
            vault: crate::command::VaultRuntimeOptions {
                home: Some(vault_home.clone()),
                ..Default::default()
            },
        },
    ))
    .unwrap();

    assert_eq!(output["ok"], true);
    let stdout = output["result"]["stdout"].as_str().unwrap();
    assert!(stdout.contains("token=[REDACTED]"));
    assert!(!stdout.contains("secret-value"));
    assert!(!stdout.contains("JIG_VAULT_PASSPHRASE"));
    assert!(!stdout.contains("correct horse battery staple"));
    assert_eq!(output["result"]["exit_status"], 0);

    let _verify_passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase.expose_secret());
    capture_vault_passphrase().unwrap();
    let verification = dispatch_vault(crate::command::VaultCommand::Audit(
        crate::command::VaultAuditCommand::Verify(crate::command::VaultAuditVerifyRequest {
            vault: crate::command::VaultRuntimeOptions {
                home: Some(vault_home),
                ..Default::default()
            },
        }),
    ))
    .unwrap();
    assert_eq!(verification["ok"], true);
    assert_eq!(verification["event_count"].as_u64().unwrap(), 4);
}

#[cfg(unix)]
#[test]
fn dispatch_vault_run_delivers_secret_file() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let vault_home = temp.path().join("vault");
    let passphrase = "correct horse battery staple";
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase);
    capture_vault_passphrase().unwrap();
    let vault = Vault::resolve(Some(vault_home.clone())).unwrap();
    let passphrase = SecretString::from(passphrase.to_string());
    vault.init(&passphrase).unwrap();
    vault
        .set_secret(
            &passphrase,
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    let output = dispatch_vault(crate::command::VaultCommand::Run(
        crate::command::VaultRunRequest {
            env: Vec::new(),
            files: vec!["TOKEN_FILE=api_token".into()],
            command: vec![
                "sh".into(),
                "-c".into(),
                "test -f \"$TOKEN_FILE\" && cat \"$TOKEN_FILE\"".into(),
            ],
            vault: crate::command::VaultRuntimeOptions {
                home: Some(vault_home),
                ..Default::default()
            },
        },
    ))
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["env_mappings"], 0);
    assert_eq!(output["file_mappings"], 1);
    assert_eq!(output["result"]["stdout"], "[REDACTED]");
    assert_eq!(output["result"]["exit_status"], 0);
}

#[test]
fn dispatch_vault_run_records_failure_audit_event() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let vault_home = temp.path().join("vault");
    let passphrase = "correct horse battery staple";
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase);
    capture_vault_passphrase().unwrap();
    let vault = Vault::resolve(Some(vault_home.clone())).unwrap();
    let passphrase = SecretString::from(passphrase.to_string());
    vault.init(&passphrase).unwrap();
    vault
        .set_secret(
            &passphrase,
            "api_token",
            SecretBytes::new(b"secret-value".to_vec()),
        )
        .unwrap();

    let error = dispatch_vault(crate::command::VaultCommand::Run(
        crate::command::VaultRunRequest {
            env: vec!["TOKEN=api_token".into()],
            files: Vec::new(),
            command: vec!["definitely-not-a-jig-vault-test-command".into()],
            vault: crate::command::VaultRuntimeOptions {
                home: Some(vault_home),
                ..Default::default()
            },
        },
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains("failed to run brokered command"));

    let verification = vault.verify_audit(&passphrase).unwrap();
    assert_eq!(verification.event_count, 4);
}

fn dispatch(ctx: &RepoContext, command: CommandKind) -> Result<Value> {
    super::dispatch(ctx, runtime_command_from_cli(command))
}

fn runtime_command_from_cli(command: CommandKind) -> RuntimeCommand {
    match command {
        CommandKind::Bootstrap(opts) => RuntimeCommand::Bootstrap(opts.into()),
        CommandKind::Check(command) => RuntimeCommand::Check(command.into()),
        CommandKind::SchemaDump(opts) => RuntimeCommand::SchemaDump(opts.into()),
        CommandKind::MigrationAdd(opts) => RuntimeCommand::MigrationAdd(opts.into()),
        CommandKind::AgentMap(command) => RuntimeCommand::AgentMap(command.into()),
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => {
            RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(opts.into())
        }
        CommandKind::Dev(opts) => RuntimeCommand::Dev(opts.into()),
        CommandKind::Proxy(command) => RuntimeCommand::Proxy(command.into()),
        CommandKind::Agent(command) => RuntimeCommand::Agent(command.into()),
        CommandKind::Work(command) => RuntimeCommand::Work(command.into()),
        CommandKind::Loop(command) => RuntimeCommand::Loop(command.into()),
        CommandKind::State(command) => RuntimeCommand::State(command.into()),
        CommandKind::Init(_)
        | CommandKind::Presets
        | CommandKind::Adopt(_)
        | CommandKind::Update(_)
        | CommandKind::Setup
        | CommandKind::Doctor
        | CommandKind::Info
        | CommandKind::Status(_)
        | CommandKind::Codex(_)
        | CommandKind::Prompt(_)
        | CommandKind::Vault(_)
        | CommandKind::Ui(_)
        | CommandKind::Mcp => {
            panic!("runtime test helper only accepts runtime commands")
        }
    }
}

#[test]
fn dispatch_routes_state_summary() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(&ctx, CommandKind::State(crate::cli::StateCommand::Summary)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "state summary");
    assert_eq!(output["counts"]["receipts"], 0);
}

#[test]
fn dispatch_distinguishes_work_status_from_state_summary() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(&ctx, CommandKind::Work(crate::cli::WorkCommand::Status)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command"], "work status");
}

#[cfg(feature = "dev-proxy")]
#[test]
fn dispatch_routes_proxy_list_through_dev_proxy_feature() {
    use crate::cli::{CommandKind, ProxyCommand, ProxyListOpts, ProxyRuntimeOpts};

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let state_dir = temp.path().join("missing-proxy-state");

    let output = dispatch(
        &ctx,
        CommandKind::Proxy(ProxyCommand::List(ProxyListOpts {
            raw: false,
            proxy: ProxyRuntimeOpts {
                state_dir: Some(state_dir.clone()),
                ..ProxyRuntimeOpts::default()
            },
        })),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(
        output["state_dir"].as_str(),
        Some(state_dir.to_str().unwrap())
    );
    assert!(output["routes"].as_array().unwrap().is_empty());
    assert!(!state_dir.exists());
}

#[test]
fn tool_no_receipt_skips_receipt_append() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
rust_test_command = "printf 'command tool ran\n'"
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
    let output = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckCommand::Test(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: true,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn native_tool_no_receipt_skips_receipt_append() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
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
        CommandKind::Check(crate::cli::CheckCommand::Contract(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: true,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn failed_tool_error_remains_primary_when_receipt_append_fails() {
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
        CommandKind::Check(crate::cli::CheckCommand::Test(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}

#[test]
fn collect_result_keeps_failed_tool_context_when_receipt_append_fails() {
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
    let error = tool_execution::execute_manifest_tool_result_without_worktree_fingerprint(
        &ctx,
        crate::tool_defs::tool::TEST,
        json!({}),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}
