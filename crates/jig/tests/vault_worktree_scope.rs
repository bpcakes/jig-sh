#![cfg(unix)]

//! A linked Git worktree resolves its main checkout's repo-scoped vault through
//! the real CLI, independent of Git environment redirection.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use jig_vault::{FieldKind, FieldMutation, SecretBytes, Vault, VaultReference};
use secrecy::SecretString;

#[path = "vault_worktree_scope/regressions.rs"]
mod regressions;

const PASSPHRASE: &str = "test-only-worktree-passphrase";
const FIELD_VALUE: &str = "test-only-worktree-field-value";
const REFERENCE: &str = "jig://Example/TOKEN";

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_main_checkout(main: &Path) {
    std::fs::create_dir_all(main.join(".agent")).unwrap();
    std::fs::write(
        main.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/test-only-template"
_commit = "test-only"
repo_name = "vault-consumer-fixture"
default_branch = "main"
jig_version = "{}"
contract_check_command = "true"

[vault]
scope = "repo"
scope_id = "scope_vault_worktree_acceptance"
allow_global = false
"#,
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::write(
        main.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": env!("CARGO_PKG_VERSION"),
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();
    git(main, &["init", "-q", "--template="]);
    git(main, &["add", "."]);
    git(main, &["commit", "-q", "-m", "fixture"]);
}

fn jig(cwd: &Path, vault_base: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(cwd)
        .args(args)
        .env("JIG_VAULT_HOME", vault_base)
        .env_remove("JIG_VAULT_PASSPHRASE")
        .env_remove("JIG_VAULT_NEW_PASSPHRASE")
        .env_remove("JIG_VAULT_PASSPHRASE_WITHHELD")
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR");
    command
}

fn json(label: &str, output: &Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{label} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_value_free(label: &str, output: &Output) {
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains(FIELD_VALUE), "{label} leaked a field value");
        assert!(!text.contains(PASSPHRASE), "{label} leaked the passphrase");
    }
}

/// Initializes the main checkout's vault directly so unlocking stays cheap.
fn initialize_vault(home: PathBuf) {
    let vault = Vault::resolve_for_test(Some(home)).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .apply_field_batch(
            &passphrase,
            vec![FieldMutation::set(
                REFERENCE.parse::<VaultReference>().unwrap(),
                FieldKind::Concealed,
                SecretBytes::new(FIELD_VALUE.as_bytes().to_vec()),
            )],
        )
        .unwrap();
}

#[test]
fn linked_worktree_cli_shares_the_main_checkout_vault() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let main = root.join("main");
    let worktree = root.join("wt");
    let vault_base = root.join("vault-base");
    write_main_checkout(&main);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );

    let main_status = json(
        "main status",
        &jig(&main, &vault_base, &["--json", "vault", "status"])
            .output()
            .unwrap(),
    );
    assert_eq!(
        main_status["vault_main_checkout_root"],
        serde_json::Value::Null
    );
    let home = PathBuf::from(main_status["vault_home"].as_str().unwrap());
    initialize_vault(home.clone());

    let status = json(
        "worktree status",
        &jig(&worktree, &vault_base, &["--json", "vault", "status"])
            .output()
            .unwrap(),
    );
    assert_eq!(status["exists"], true);
    assert_eq!(status["vault_home"], home.display().to_string());
    assert_eq!(status["vault_scope"], "repo");
    assert_eq!(
        status["vault_main_checkout_root"],
        main.display().to_string()
    );

    let output = jig(
        &worktree,
        &vault_base,
        &["--json", "vault", "field", "list"],
    )
    .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
    .output()
    .unwrap();
    assert_value_free("worktree field list", &output);
    let fields = json("worktree field list", &output);
    assert_eq!(fields["fields"][0]["reference"], REFERENCE);
    assert_eq!(fields["vault_home"], home.display().to_string());

    let info = json(
        "worktree info",
        &jig(&worktree, &vault_base, &["--json", "info"])
            .output()
            .unwrap(),
    );
    assert_eq!(info["capabilities"]["vault_initialized"], true);
    assert_eq!(
        info["capabilities"]["vault_main_checkout_root"],
        main.display().to_string()
    );

    let human = jig(&worktree, &vault_base, &["vault", "status"])
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .contains(&format!("Shared with main checkout: {}", main.display())),
        "{}",
        String::from_utf8_lossy(&human.stdout)
    );
}

#[test]
fn worktree_vault_scope_ignores_git_environment_redirects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let main = root.join("main");
    let worktree = root.join("wt");
    let unrelated = root.join("unrelated");
    let vault_base = root.join("vault-base");
    write_main_checkout(&main);
    std::fs::create_dir_all(&unrelated).unwrap();
    git(&unrelated, &["init", "-q", "--template="]);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );

    let plain = json(
        "worktree status",
        &jig(&worktree, &vault_base, &["--json", "vault", "status"])
            .output()
            .unwrap(),
    );
    let redirected = json(
        "redirected worktree status",
        &jig(&worktree, &vault_base, &["--json", "vault", "status"])
            .env("GIT_DIR", unrelated.join(".git"))
            .env("GIT_COMMON_DIR", unrelated.join(".git"))
            .env("GIT_WORK_TREE", &unrelated)
            .output()
            .unwrap(),
    );

    assert_eq!(redirected["vault_home"], plain["vault_home"]);
    assert_eq!(
        redirected["vault_main_checkout_root"],
        main.display().to_string()
    );
}
