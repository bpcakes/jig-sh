//! New-passphrase policy boundaries for bootstrap vault setup.

use secrecy::SecretString;

use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

use super::*;

/// Accepted by the earlier 12-byte floor, rejected by the current policy.
const HISTORICAL_PASSPHRASE: &str = "legacy-pass-15b";

fn repo_with_vault_scope(root: &std::path::Path) -> std::path::PathBuf {
    let repo = root.join("repo");
    TestRepoBuilder::new(&repo)
        .config(
            r#"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_123"
allow_global = false
"#,
        )
        .required_commands(["bootstrap_command"])
        .write();
    repo
}

fn scoped_vault_status(repo: &std::path::Path) -> serde_json::Value {
    let ctx = RepoContext::load_from_root(repo.to_path_buf()).unwrap();
    runtime::dispatch_vault(crate::command::VaultCommand::Status(
        crate::command::VaultStatusRequest {
            vault: runtime::repo_vault_options_for_context(&ctx).unwrap(),
        },
    ))
    .unwrap()
}

fn pre_capture_adopt() -> BootstrapVaultPlan {
    prepare_bootstrap_vault_with_availability(
        BootstrapVaultIntent::Initialize,
        BootstrapInputMode::NoInput,
        BootstrapPassphraseAvailability::Environment,
        BootstrapVaultCommand::Adopt,
    )
    .unwrap()
}

#[test]
fn bootstrap_reusing_an_existing_vault_keeps_its_historical_credential() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let repo = repo_with_vault_scope(temp.path());
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-base"));
    let home = scoped_vault_status(&repo)["vault_home"]
        .as_str()
        .unwrap()
        .to_owned();
    let historical = SecretString::from(HISTORICAL_PASSPHRASE.to_owned());
    assert!(jig_vault::validate_new_vault_passphrase(&historical).is_err());
    jig_vault::Vault::resolve_for_test(Some(home.into()))
        .unwrap()
        .init_format_for_test(&historical, jig_vault::LATEST_VAULT_FORMAT_VERSION)
        .unwrap();
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", HISTORICAL_PASSPHRASE);

    assert_eq!(pre_capture_adopt(), BootstrapVaultPlan::PreCaptured);
    assert!(std::env::var_os("JIG_VAULT_PASSPHRASE").is_none());
    let report =
        ensure_bootstrap_vault(repo.to_str().unwrap(), BootstrapVaultPlan::PreCaptured).unwrap();
    let report = serde_json::to_value(report).unwrap();
    assert_eq!(report["initialized"], true);
    assert_eq!(report["created"], false);
}

#[test]
fn bootstrap_rejects_a_guessable_candidate_only_when_initializing() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let repo = repo_with_vault_scope(temp.path());
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-base"));
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", HISTORICAL_PASSPHRASE);

    assert_eq!(pre_capture_adopt(), BootstrapVaultPlan::PreCaptured);
    let error = ensure_bootstrap_vault(repo.to_str().unwrap(), BootstrapVaultPlan::PreCaptured)
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains(jig_vault::NEW_VAULT_PASSPHRASE_POLICY),
        "{error}"
    );
    assert!(
        error.contains(runtime::VAULT_PASSPHRASE_OPERATOR_GUIDANCE),
        "{error}"
    );
    assert!(error.contains("repo files were written"), "{error}");
    assert!(!error.contains(HISTORICAL_PASSPHRASE), "{error}");
    assert_eq!(scoped_vault_status(&repo)["exists"], false);
}

#[test]
fn every_bootstrap_command_defers_policy_until_initialization() {
    // A forced `init` can keep an existing `[vault].scope_id`, so neither
    // command may reject a credential before it knows a vault is created.
    for command in [BootstrapVaultCommand::Init, BootstrapVaultCommand::Adopt] {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", HISTORICAL_PASSPHRASE);
        let plan = prepare_bootstrap_vault_with_availability(
            BootstrapVaultIntent::Initialize,
            BootstrapInputMode::NoInput,
            BootstrapPassphraseAvailability::Environment,
            command,
        )
        .unwrap();
        assert_eq!(plan, BootstrapVaultPlan::PreCaptured);
        assert!(std::env::var_os("JIG_VAULT_PASSPHRASE").is_none());
    }
}
