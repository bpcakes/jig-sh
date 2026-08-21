use std::fs;

use jig_vault::{SecretBytes, Vault};
use secrecy::{ExposeSecret, SecretString};
use tempfile::tempdir;

use common::*;

use crate::cli::{
    CommandKind, MigrationCommand, SqlxCommand, SqlxMigrationCommand, SqlxSchemaCommand,
};
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
    let vault = Vault::resolve_for_test(Some(vault_home.clone())).unwrap();
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
    let vault = Vault::resolve_for_test(Some(vault_home.clone())).unwrap();
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
    let vault = Vault::resolve_for_test(Some(vault_home.clone())).unwrap();
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
        CommandKind::Check(command) => RuntimeCommand::Check(command.try_into().unwrap()),
        CommandKind::Migration(MigrationCommand::Add(opts)) => {
            RuntimeCommand::MigrationAdd(opts.into())
        }
        CommandKind::Sqlx(SqlxCommand::Migration(SqlxMigrationCommand::Add(opts))) => {
            RuntimeCommand::MigrationAdd(opts.into())
        }
        CommandKind::Sqlx(SqlxCommand::Schema(SqlxSchemaCommand::Dump(opts))) => {
            RuntimeCommand::Sqlx(crate::command::SqlxCommand::SchemaDump(opts.into()))
        }
        CommandKind::SchemaDump(opts) => {
            RuntimeCommand::Sqlx(crate::command::SqlxCommand::SchemaDump(opts.into()))
        }
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
        | CommandKind::RuntimeCompatible(_)
        | CommandKind::Presets
        | CommandKind::Adopt(_)
        | CommandKind::Update(_)
        | CommandKind::Setup
        | CommandKind::Doctor
        | CommandKind::Info(_)
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
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Test(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: true,
            }),
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn repository_check_explains_and_executes_the_legacy_default_profile() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
fmt_command = "printf 'fmt ran\n'"
test_command = "printf 'test ran\n'"

[work]
checks = ["jig.fmt_check", "jig.test"]
"#,
        )
        .required_commands(["fmt_command", "test_command"])
        .tool(json!({
            "name": "jig.fmt_check",
            "kind": "command",
            "description": "Run formatting.",
            "command": "fmt_command"
        }))
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run tests.",
            "command": "test_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let explained = super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: true,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, false),
            },
        )),
    )
    .unwrap();
    assert_eq!(explained["executed"], false);
    assert_eq!(explained["plan"]["profile"], "verify");
    assert_eq!(explained["plan"]["targets"].as_array().unwrap().len(), 2);
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());

    let executed = super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, false),
            },
        )),
    )
    .unwrap();
    assert_eq!(executed["ok"], true);
    assert_eq!(executed["executed"], true);
    assert_eq!(executed["results"].as_array().unwrap().len(), 2);
    assert_eq!(executed["results"][0]["target"]["component"], "repo");
}

#[test]
fn repository_check_persists_queryable_runs_and_target_receipts() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
fmt_command = "printf 'fmt ran\n'"
test_command = "printf 'test ran\n'"

[work]
checks = ["jig.fmt_check", "jig.test"]
"#,
        )
        .required_commands(["fmt_command", "test_command"])
        .tool(json!({
            "name": "jig.fmt_check",
            "kind": "command",
            "description": "Run formatting.",
            "command": "fmt_command"
        }))
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run tests.",
            "command": "test_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(Some("plan_work".into()), true),
            },
        )),
    )
    .unwrap();
    let run_id = output["run"]["run_id"].as_str().unwrap();

    let durable = crate::state::run_by_id(&ctx, run_id).unwrap();
    assert_eq!(durable.work_plan_id.as_deref(), Some("plan_work"));
    assert_eq!(durable.result.status, jig_contract::RunStatus::Completed);
    assert_eq!(
        durable.result.conclusion,
        Some(jig_contract::RunConclusion::Success)
    );
    assert_eq!(durable.result.targets.len(), 2);
    assert!(
        durable
            .result
            .targets
            .iter()
            .all(|target| target.receipt_id.is_some())
    );

    let receipts = crate::state::receipts_list(
        &ctx,
        crate::state::ReceiptListFilter {
            session_id: None,
            plan_id: Some("plan_work".into()),
            tool_name: None,
            failed_only: false,
            limit: 20,
        },
    )
    .unwrap();
    let receipts = receipts["receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 2);
    assert!(receipts.iter().all(|receipt| receipt["run_id"] == run_id));
    assert!(receipts.iter().all(|receipt| receipt["target"].is_object()));
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["config_digest"].as_str().is_some())
    );
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["input_digest"].as_str().is_some())
    );
}

#[test]
fn repository_execution_rejects_a_stale_plan_before_creating_a_run() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
test_command = "printf 'test ran\n'"

[work]
checks = ["jig.test"]
"#,
        )
        .required_commands(["test_command"])
        .tool(json!({
            "name": "jig.test",
            "kind": "command",
            "description": "Run tests.",
            "command": "test_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();
    fs::write(temp.path().join("changed-after-plan.txt"), "changed\n").unwrap();

    let error = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("stale or was modified"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn repository_execution_records_cancelled_results_for_every_target() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
first_command = "printf 'first\n'"
second_command = "printf 'second\n'"

[work]
checks = ["jig.first", "jig.second"]
"#,
        )
        .required_commands(["first_command", "second_command"])
        .tool(json!({
            "name": "jig.first",
            "kind": "command",
            "description": "Run first.",
            "command": "first_command"
        }))
        .tool(json!({
            "name": "jig.second",
            "kind": "command",
            "description": "Run second.",
            "command": "second_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan =
        crate::repository::plan_run(&ctx, &catalog, crate::repository::PlanRunRequest::default())
            .unwrap();

    let execution = super::run_execution::execute_check_run(
        &ctx,
        &catalog,
        plan,
        super::run_execution::ExecuteCheckRunRequest {
            work_plan_id: None,
            record_receipts: true,
            fail_fast: false,
        },
        &|| true,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Cancelled)
    );
    assert_eq!(execution.run.result.targets.len(), 2);
    assert!(execution.run.result.targets.iter().all(|target| {
        target.status == jig_contract::RunStatus::Completed
            && target.conclusion == Some(jig_contract::RunConclusion::Cancelled)
            && target.receipt_id.is_some()
    }));
}

#[test]
fn repository_check_collects_failures_unless_fail_fast_is_explicit() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
failing_command = "printf 'failed\n' >&2; exit 7"
later_command = "printf 'later\n' > later-ran.txt"

[work]
checks = ["jig.a_fail", "jig.z_later"]
"#,
        )
        .required_commands(["failing_command", "later_command"])
        .tool(json!({
            "name": "jig.a_fail",
            "kind": "command",
            "description": "Fail first.",
            "command": "failing_command"
        }))
        .tool(json!({
            "name": "jig.z_later",
            "kind": "command",
            "description": "Run later.",
            "command": "later_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let request = |fail_fast| {
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast,
                tool: crate::command::ToolRequest::new(None, false),
            },
        ))
    };

    let collected = super::dispatch(&ctx, request(false)).unwrap();
    assert_eq!(collected["ok"], false);
    assert_eq!(collected["results"].as_array().unwrap().len(), 2);
    assert!(temp.path().join("later-ran.txt").exists());

    fs::remove_file(temp.path().join("later-ran.txt")).unwrap();
    let stopped = super::dispatch(&ctx, request(true)).unwrap();
    assert_eq!(stopped["ok"], false);
    assert_eq!(stopped["results"].as_array().unwrap().len(), 1);
    assert!(!temp.path().join("later-ran.txt").exists());
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
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Contract(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: true,
            }),
        )),
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
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Test(crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: false,
            }),
        )),
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
