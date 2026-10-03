//! Vault scope selection when a discovered repository configuration does not
//! load.

use crate::command::{VaultRuntimeOptions, VaultScopeSelection, VaultStatusRequest};
use crate::test_env::{CurrentDirGuard, TestRepoBuilder, lock_env};

use super::*;

fn assert_explains_unsupported_harness_stubs(error: &str) {
    for expected in [
        "Vault scope selection could not load the Jig repository configuration",
        "Do not delete or bypass an existing repository's configuration",
        "fix the reported problem (for example by updating Jig) or ask the operator",
        "Placeholder harness files created only to reach the vault are unsupported",
        "Outside a Jig repository, `jig vault` uses the user-level vault",
        "`--home DIR` (a private directory outside any repository) for diagnostics",
        "Reported problem: ",
    ] {
        assert!(error.contains(expected), "missing {expected:?}: {error}");
    }
    // Agents act on this text: it must not steer them toward writing harness
    // files, creating a vault, or handling the passphrase.
    for unwanted in ["jig adopt", "--write", "vault init", "JIG_VAULT_PASSPHRASE"] {
        assert!(
            !error.contains(unwanted),
            "unexpected {unwanted:?}: {error}"
        );
    }
}

fn write_vault_only_stub(root: &std::path::Path) {
    std::fs::write(
        root.join(".jig.toml"),
        "[vault]\nscope = \"repo\"\nscope_id = \"ExampleVault\"\n",
    )
    .unwrap();
}

fn status_command(scope: VaultScopeSelection) -> crate::command::VaultCommand {
    crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions { home: None, scope },
    })
}

#[test]
fn vault_only_jig_toml_stub_fails_closed_with_harness_guidance() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    write_vault_only_stub(temp.path());
    let _cwd = CurrentDirGuard::set(temp.path());
    let mut command = status_command(VaultScopeSelection::Auto);

    let error = format!("{:#}", apply_repo_vault_scope(&mut command).unwrap_err());

    assert!(error.contains("missing field `_src_path`"), "{error}");
    assert_explains_unsupported_harness_stubs(&error);
    assert!(matches!(
        vault_options_mut(&mut command).scope,
        VaultScopeSelection::Auto
    ));
}

#[test]
fn global_selection_with_unloadable_repo_config_fails_closed() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    write_vault_only_stub(temp.path());
    let _cwd = CurrentDirGuard::set(temp.path());
    let mut command = status_command(VaultScopeSelection::Global);

    let error = format!("{:#}", apply_repo_vault_scope(&mut command).unwrap_err());

    assert!(error.contains("missing field `_src_path`"), "{error}");
    assert_explains_unsupported_harness_stubs(&error);
}

#[test]
fn contract_stub_without_repository_model_explains_harness_stubs() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .contract_version(6)
        .write();
    let _cwd = CurrentDirGuard::set(temp.path());
    let mut command = status_command(VaultScopeSelection::Auto);

    let error = format!("{:#}", apply_repo_vault_scope(&mut command).unwrap_err());

    assert!(
        error.contains("jig contract version 6 requires [repository]"),
        "{error}"
    );
    assert_explains_unsupported_harness_stubs(&error);
}

#[test]
fn directory_without_jig_toml_keeps_the_user_level_vault() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());
    let mut command = status_command(VaultScopeSelection::Auto);

    apply_repo_vault_scope(&mut command).unwrap();

    let options = vault_options_mut(&mut command);
    assert!(options.home.is_none());
    assert!(matches!(options.scope, VaultScopeSelection::Auto));
}
