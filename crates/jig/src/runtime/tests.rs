use std::fs;
use std::time::{Duration, Instant};

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

#[test]
fn named_v6_checks_preserve_feature_specific_unavailable_diagnostics() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = crate::execution::NoopExecutionObserver;

    let direct_error = dispatch_named_check(
        &ctx,
        "sqlc",
        crate::tool_defs::tool::SQLC_CHECK,
        crate::command::ToolRequest::default(),
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    let flagged_error = dispatch_repository_check(
        &ctx,
        crate::command::RepositoryCheckRequest {
            selectors: vec!["sqlc".into()],
            profile: None,
            affected_base: None,
            explain: true,
            fail_fast: false,
            tool: crate::command::ToolRequest::default(),
        },
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    assert!(
        direct_error.contains("go_database is not \"postgres\""),
        "{direct_error}"
    );
    assert_eq!(flagged_error, direct_error);
}

#[test]
fn cli_runtime_rejects_migration_add_for_versioned_artifacts_without_mutation() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
"#,
        )
        .tool(serde_json::json!({
            "name": "jig.migration_add",
            "kind": "native",
            "description": "Add migration."
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = super::dispatch(
        &ctx,
        RuntimeCommand::MigrationAdd(crate::command::MigrationAddRequest {
            name: "create_users".into(),
            tool: crate::command::ToolRequest::default(),
        }),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured Rust migration layout does not permit flat migration stubs")
    );
    assert!(!temp.path().join("schema").exists());
}

#[test]
fn cli_runtime_rejects_command_backed_migration_add_for_versioned_artifacts_without_mutation() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
migration_add_command = "mkdir -p schema && touch schema/should-not-exist.sql"
"#,
        )
        .required_commands(["migration_add_command"])
        .tool(serde_json::json!({
            "name": "jig.migration_add",
            "kind": "command",
            "description": "Add migration.",
            "command": "migration_add_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = super::dispatch(
        &ctx,
        RuntimeCommand::MigrationAdd(crate::command::MigrationAddRequest {
            name: "create_users".into(),
            tool: crate::command::ToolRequest::default(),
        }),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured Rust migration layout does not permit flat migration stubs")
    );
    assert!(!temp.path().join("schema").exists());
}

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

#[test]
fn runtime_state_summary_polls_operation_cancellation_during_collection() {
    struct CancelAfterBoundary(std::cell::Cell<usize>);

    impl crate::execution::ExecutionObserver for CancelAfterBoundary {}

    impl crate::execution::ExecutionCancellation for CancelAfterBoundary {
        fn cancelled(&self) -> bool {
            let polls = self.0.get() + 1;
            self.0.set(polls);
            polls > 1
        }
    }

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelAfterBoundary(std::cell::Cell::new(0));

    let error = dispatch_with_observer(
        &ctx,
        RuntimeCommand::State(crate::command::StateCommand::Summary),
        &mut observer,
    )
    .unwrap_err()
    .to_string();

    assert_eq!(error, "status collection was cancelled");
}

#[test]
fn runtime_does_not_reclassify_committed_work_as_cancelled() {
    struct CancelAfterEntry(std::cell::Cell<usize>);

    impl crate::execution::ExecutionObserver for CancelAfterEntry {}

    impl crate::execution::ExecutionCancellation for CancelAfterEntry {
        fn cancelled(&self) -> bool {
            let polls = self.0.get() + 1;
            self.0.set(polls);
            polls > 1
        }
    }

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelAfterEntry(std::cell::Cell::new(0));

    let output = dispatch_with_observer(
        &ctx,
        RuntimeCommand::Work(crate::command::WorkCommand::Start(
            crate::command::WorkStartRequest {
                title: "Example committed work".into(),
                body: Some("Regression fixture for the durable commit boundary.".into()),
                body_file: None,
                base: None,
            },
        )),
        &mut observer,
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    assert!(
        crate::state::open_plan_summaries(&ctx)
            .unwrap()
            .iter()
            .any(|plan| plan["plan_id"] == plan_id),
        "the successful result must identify the committed plan"
    );
    assert_eq!(observer.0.get(), 1, "dispatch must not poll after commit");
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
            crate::cli::CheckCommand::Test(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: true,
                },
                selectors: Vec::new(),
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
fn supported_legacy_contract_executes_declared_loc_command_without_native_fallback() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join("Example.ts"), "one\ntwo\n").unwrap();
    let checker = r#"#!/usr/bin/env bash
set -eu
line_count="$(wc -l < Example.ts | tr -d ' ')"
if [ "$line_count" -gt 2 ]; then
  printf 'Example.ts is too large\n' >&2
  exit 1
fi
printf 'legacy command LOC passed\n'
"#;
    fs::write(temp.path().join("scripts/check-file-loc.sh"), checker).unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(5)
        .config(
            r#"
[commands]
rust_file_loc_command = "bash scripts/check-file-loc.sh"

[work]
checks = ["jig.rust_file_loc"]
"#,
        )
        .required_commands(["rust_file_loc_command"])
        .tool(json!({
            "name": "jig.rust_file_loc",
            "kind": "command",
            "description": "Run the repository-owned file LOC check.",
            "command": "rust_file_loc_command"
        }))
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = super::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: vec!["rust-file-loc".into()],
                profile: None,
                affected_base: None,
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, false),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "legacy command LOC passed\n"
    );
    assert_eq!(output["run"]["targets"][0]["target"]["component"], "repo");
    assert_eq!(
        output["run"]["targets"][0]["target"]["action"],
        "rust-file-loc"
    );
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
    crate::state::seed_open_plan_for_test(&ctx, "plan_work", "Work", "Body").unwrap();

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
fn repository_affected_check_explains_and_executes_only_matching_v6_targets() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "[repository]\ndefault_check_profile = \"verify\"",
        "[repository]\ndefault_check_profile = \"verify\"\naffected_ignore = [\".env\", \".env.*\", \"**/.env\", \"**/.env.*\"]",
    );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["affected_ignore"] = json!([".env", ".env.*", "**/.env", "**/.env.*"]);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join(".gitignore"), ".env\n.env.*\n").unwrap();
    init_git_repo(temp.path());
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://example.invalid/app\n",
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let request = |explain| {
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: Some("HEAD".into()),
                explain,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, true),
            },
        ))
    };

    let dotenv_only = super::dispatch(&ctx, request(true)).unwrap();

    assert!(
        dotenv_only["plan"]["targets"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    fs::write(
        temp.path().join("api/example.go"),
        "package example\n\nconst changed = true\n",
    )
    .unwrap();
    let explained = super::dispatch(&ctx, request(true)).unwrap();

    assert_eq!(explained["executed"], false);
    assert_eq!(explained["plan"]["affected_base"], "HEAD");
    assert_eq!(explained["plan"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(
        explained["plan"]["targets"][0]["target"],
        json!({"component": "api", "action": "test"})
    );
    assert!(
        explained["plan"]["targets"][0]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason["kind"] == "direct_input" && reason["path"] == "api/example.go"
            })
    );

    let executed = super::dispatch(&ctx, request(false)).unwrap();

    assert_eq!(executed["ok"], true);
    assert_eq!(executed["results"].as_array().unwrap().len(), 1);
    assert_eq!(executed["results"][0]["target"]["component"], "api");
    assert_eq!(executed["run"]["targets"].as_array().unwrap().len(), 1);
}

mod agent;
mod common;
mod loops;
mod mcp;
mod repository_execution;
mod work;
