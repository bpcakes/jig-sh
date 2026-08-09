use crate::command::{
    VaultInjectRequest, VaultReadRequest, VaultRepoScope, VaultRuntimeOptions, VaultScopeSelection,
    VaultStatusRequest,
};
use crate::test_env::{CurrentDirGuard, EnvVarGuard, TestRepoBuilder, lock_env};

use super::*;

#[test]
fn repo_vault_scope_is_applied_to_auto_options() {
    let mut options = VaultRuntimeOptions::default();
    let repo_root = std::path::PathBuf::from("/repo");

    apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", &repo_root)),
        false,
    )
    .unwrap();

    match options.scope {
        VaultScopeSelection::Repo(VaultRepoScope {
            scope_id,
            repo_name,
            repo_root: actual_root,
        }) => {
            assert_eq!(scope_id, "scope_1");
            assert_eq!(repo_name, "demo");
            assert_eq!(actual_root, repo_root);
        }
        other => panic!("expected repo scope, got {other:?}"),
    }
}

#[test]
fn repo_vault_scope_rejects_global_when_disallowed() {
    let mut options = VaultRuntimeOptions {
        home: None,
        scope: VaultScopeSelection::Global,
    };

    let error = apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", "/repo")),
        false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("allow_global is false"));
}

#[test]
fn repo_vault_scope_allows_global_when_configured() {
    let mut options = VaultRuntimeOptions {
        home: None,
        scope: VaultScopeSelection::Global,
    };

    apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", "/repo")),
        true,
    )
    .unwrap();

    assert!(matches!(options.scope, VaultScopeSelection::Global));
}

#[test]
fn repo_vault_scope_leaves_auto_legacy_without_repo_scope() {
    let mut options = VaultRuntimeOptions::default();

    apply_repo_vault_scope_to_options(&mut options, None, false).unwrap();

    assert!(matches!(options.scope, VaultScopeSelection::Auto));
}

#[test]
fn repo_vault_scope_home_override_bypasses_repo_policy() {
    let home = std::path::PathBuf::from("/tmp/custom-vault");
    let mut options = VaultRuntimeOptions {
        home: Some(home.clone()),
        scope: VaultScopeSelection::Global,
    };

    apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", "/repo")),
        false,
    )
    .unwrap();

    assert_eq!(options.home.as_deref(), Some(home.as_path()));
    assert!(matches!(options.scope, VaultScopeSelection::Global));
}

#[test]
fn explicit_vault_home_bypasses_repo_context_loading() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join(".jig.toml"),
        r#"[vault]
scope = "repo"
"#,
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());
    let explicit_home = temp.path().join("explicit-vault");
    let mut command = crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions {
            home: Some(explicit_home.clone()),
            ..Default::default()
        },
    });

    apply_repo_vault_scope(&mut command).unwrap();

    let options = vault_options_mut(&mut command);
    assert_eq!(options.home.as_deref(), Some(explicit_home.as_path()));
    assert!(matches!(options.scope, VaultScopeSelection::Auto));
}

#[test]
fn malformed_repo_vault_config_blocks_status_without_home_override() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
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
"#,
        )
        .required_commands(["bootstrap_command"])
        .write();
    let _cwd = CurrentDirGuard::set(temp.path());
    let mut command = crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions::default(),
    });

    let error = apply_repo_vault_scope(&mut command).unwrap_err();
    let error = format!("{error:#}");

    assert!(
        error.contains("[vault].scope_id is required"),
        "unexpected error: {error}"
    );
}

#[test]
fn vault_options_mut_reaches_nested_status_command() {
    let mut command = crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions::default(),
    });

    vault_options_mut(&mut command).scope = VaultScopeSelection::Global;

    match command {
        crate::command::VaultCommand::Status(request) => {
            assert!(matches!(request.vault.scope, VaultScopeSelection::Global));
        }
        other => panic!("expected status command, got {other:?}"),
    }
}

fn read_request() -> crate::command::VaultCommand {
    crate::command::VaultCommand::Read(VaultReadRequest {
        reference: "jig://Production/PASSWORD".parse().unwrap(),
        reveal: false,
        out_file: None,
        overwrite: false,
        vault: VaultRuntimeOptions::default(),
    })
}

fn inject_request(input: impl Into<std::path::PathBuf>) -> crate::command::VaultCommand {
    crate::command::VaultCommand::Inject(VaultInjectRequest {
        input: input.into(),
        template: None,
        reveal: false,
        out_file: None,
        overwrite: false,
        vault: VaultRuntimeOptions::default(),
    })
}

#[test]
fn raw_vault_commands_reject_json_before_runtime_dispatch() {
    for command in [read_request(), inject_request("template")] {
        let error = validate_raw_vault_command(&command, true, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--json is not supported"));
        assert!(!error.contains("PASSWORD"));
    }
}

#[test]
fn json_and_terminal_refusals_precede_passphrase_capture_and_vault_access() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let passphrase = "correct horse battery staple";
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase);

    for (json_output, stdout_is_terminal) in [(true, false), (false, true)] {
        let vault_home = temp.path().join(format!(
            "absent-vault-{}-{}",
            u8::from(json_output),
            u8::from(stdout_is_terminal)
        ));
        let command = VaultCommand::Read(super::super::vault::VaultReadOpts {
            reference: "jig://Production/PASSWORD".parse().unwrap(),
            reveal: false,
            out_file: None,
            overwrite: false,
            vault: super::super::vault::VaultRuntimeOpts {
                home: Some(vault_home.clone()),
                global: false,
            },
        });

        let error =
            run_vault_command_with_stdout_terminal(command, json_output, stdout_is_terminal)
                .unwrap_err()
                .to_string();
        if json_output {
            assert!(error.contains("--json is not supported"));
        } else {
            assert!(error.contains("without --reveal"));
        }
        assert_eq!(
            std::env::var("JIG_VAULT_PASSPHRASE").as_deref(),
            Ok(passphrase)
        );
        assert!(!vault_home.exists());
    }
}

#[test]
fn raw_vault_commands_require_explicit_terminal_reveal() {
    for mut command in [read_request(), inject_request("template")] {
        let error = validate_raw_vault_command(&command, false, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without --reveal"));

        match &mut command {
            crate::command::VaultCommand::Read(request) => request.reveal = true,
            crate::command::VaultCommand::Inject(request) => request.reveal = true,
            other => panic!("expected raw command, got {other:?}"),
        }
        validate_raw_vault_command(&command, false, true).unwrap();
    }
}

#[test]
fn output_file_avoids_terminal_reveal_and_requires_overwrite_for_same_input() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("template");
    std::fs::write(&input, b"template").unwrap();

    let mut read = read_request();
    let crate::command::VaultCommand::Read(read) = &mut read else {
        unreachable!();
    };
    read.out_file = Some(temp.path().join("read-output"));
    validate_raw_vault_command(
        &crate::command::VaultCommand::Read(VaultReadRequest {
            reference: read.reference.clone(),
            reveal: read.reveal,
            out_file: read.out_file.clone(),
            overwrite: read.overwrite,
            vault: read.vault.clone(),
        }),
        false,
        true,
    )
    .unwrap();

    let mut inject = inject_request(&input);
    let crate::command::VaultCommand::Inject(request) = &mut inject else {
        unreachable!();
    };
    request.out_file = Some(input);
    let error = validate_raw_vault_command(&inject, false, false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("same file"));
    let crate::command::VaultCommand::Inject(request) = &mut inject else {
        unreachable!();
    };
    request.overwrite = true;
    validate_raw_vault_command(&inject, false, false).unwrap();
}

#[cfg(unix)]
#[test]
fn inject_same_file_check_detects_relative_aliases_and_hard_links() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("template"), b"template").unwrap();
    std::fs::hard_link(
        temp.path().join("template"),
        temp.path().join("template-link"),
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());

    assert!(
        input_and_output_are_same_file(
            std::path::Path::new("template"),
            std::path::Path::new("./template")
        )
        .unwrap()
    );
    assert!(
        input_and_output_are_same_file(
            std::path::Path::new("template"),
            std::path::Path::new("template-link")
        )
        .unwrap()
    );
    assert!(
        !input_and_output_are_same_file(std::path::Path::new("-"), std::path::Path::new("-"))
            .unwrap()
    );
}

#[test]
fn invalid_injection_input_fails_before_passphrase_capture_or_vault_creation() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let vault_home = temp.path().join("absent-vault-home");
    let output = temp.path().join("rendered-output");
    let missing = temp.path().join("missing-template");
    let malformed = temp.path().join("malformed-template");
    let oversized = temp.path().join("oversized-template");
    std::fs::write(&malformed, b"{{ jig://Production }}").unwrap();
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(u64::try_from(jig_vault::MAX_TEMPLATE_INPUT_LEN + 1).unwrap())
        .unwrap();
    let passphrase = "correct horse battery staple";
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", passphrase);

    for input in [missing, malformed, oversized] {
        let command = VaultCommand::Inject(super::super::vault::VaultInjectOpts {
            input,
            reveal: false,
            out_file: Some(output.clone()),
            overwrite: false,
            vault: super::super::vault::VaultRuntimeOpts {
                home: Some(vault_home.clone()),
                global: false,
            },
        });

        let error = run_vault_command(command, false).unwrap_err().to_string();
        assert!(!error.contains(passphrase));
        assert_eq!(
            std::env::var("JIG_VAULT_PASSPHRASE").as_deref(),
            Ok(passphrase)
        );
        assert!(!vault_home.exists());
        assert!(!output.exists());
    }
}
