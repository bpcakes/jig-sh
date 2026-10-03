#![cfg(unix)]

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::command::{VaultRuntimeOptions, VaultScopeSelection, VaultStatusRequest};
use crate::test_env::{EnvLockGuard, EnvVarGuard, lock_env};

use super::super::{VAULT_HOME_ENV, status};

mod forgery;
mod orphan;

const SCOPE_ID: &str = "scope_worktree";
const REPO_NAME: &str = "vault-consumer-fixture";

/// Temporary fixture root with a private vault base. Field order matters: the
/// environment override must be restored before the environment lock drops.
struct Fixture {
    _vault_home: EnvVarGuard,
    _env: EnvLockGuard,
    _temp: TempDir,
    root: PathBuf,
    base: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let env = lock_env();
        let temp = tempfile::tempdir().unwrap();
        // Expected digests hash canonical roots (macOS /var is /private/var).
        let root = temp.path().canonicalize().unwrap();
        // A `.git` directory ceiling keeps the walk-up inside the fixture even
        // when the temporary directory itself lies in a linked worktree.
        fs::create_dir(root.join(".git")).unwrap();
        let base = root.join("vault-base");
        let vault_home = EnvVarGuard::set(VAULT_HOME_ENV, &base);
        Self {
            _vault_home: vault_home,
            _env: env,
            _temp: temp,
            root,
            base,
        }
    }

    /// Creates a main checkout with one commit that tracks `directories`.
    fn main_checkout(&self, name: &str, directories: &[&str]) -> PathBuf {
        let main = self.root.join(name);
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "--template="]);
        fs::write(main.join("README.md"), "fixture\n").unwrap();
        for directory in directories {
            fs::create_dir_all(main.join(directory)).unwrap();
            fs::write(main.join(directory).join("README.md"), "fixture\n").unwrap();
        }
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "fixture"]);
        main
    }

    fn linked_worktree(&self, main: &Path, name: &str) -> PathBuf {
        let worktree = self.root.join(name);
        git(
            main,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
            ],
        );
        worktree
    }

    /// Namespace that hashes `root` exactly as given.
    fn home_for(&self, root: &Path) -> PathBuf {
        self.base.join("scopes").join(expected_scope_dir(root))
    }
}

/// Runs Git with no global or system config and no inherited repository
/// redirection, so developer settings cannot change the metadata it writes.
fn git(cwd: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    crate::bootstrap::scrub_known_repository_git_environment(&mut command);
    let output = command
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
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

/// Independent recomputation of the persisted v2 namespace recipe.
fn expected_scope_dir(root: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(b"jig-vault-repo-scope-v2\0");
    digest.update(root.as_os_str().as_bytes());
    digest.update(b"\0");
    digest.update(SCOPE_ID.as_bytes());
    let hex = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("repo-{hex}")
}

fn status_for(root: &Path) -> Result<Value> {
    status(VaultStatusRequest {
        vault: VaultRuntimeOptions::repo(SCOPE_ID, REPO_NAME, root),
    })
}

fn home_of(output: &Value) -> PathBuf {
    PathBuf::from(output["vault_home"].as_str().unwrap())
}

fn write_vault_file(home: &Path) {
    fs::create_dir_all(home).unwrap();
    fs::write(home.join("vault.json"), "{}").unwrap();
}

/// Agents act on error text, so recovery that passes `--home` or moves vault
/// directories must follow the operator routing and never precede it.
fn assert_operator_routed(error: &str) {
    let routing = error
        .find(super::VAULT_STORAGE_OPERATOR_STEP)
        .unwrap_or_else(|| panic!("missing operator routing: {error}"));
    for step in ["--home", "rename ", "remove ", "move "] {
        assert!(
            !error[..routing].contains(step),
            "{step:?} precedes operator routing: {error}"
        );
    }
}

#[test]
fn checkout_namespace_digest_is_unchanged() {
    let fixture = Fixture::new();
    let plain = fixture.root.join("plain");
    fs::create_dir(&plain).unwrap();
    let main = fixture.main_checkout("main", &[]);

    for root in [&plain, &main] {
        let output = status_for(root).unwrap();
        assert_eq!(home_of(&output), fixture.home_for(root));
        assert_eq!(output["vault_scope"], "repo");
        assert_eq!(output["vault_main_checkout_root"], Value::Null);
    }
    assert!(!fixture.base.exists());
}

#[test]
fn linked_worktree_shares_main_checkout_vault_home() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");

    let main_output = status_for(&main).unwrap();
    let worktree_output = status_for(&worktree).unwrap();

    assert_eq!(home_of(&worktree_output), home_of(&main_output));
    assert_eq!(home_of(&worktree_output), fixture.home_for(&main));
    assert_eq!(
        worktree_output["vault_main_checkout_root"],
        main.display().to_string()
    );
    assert_eq!(main_output["vault_main_checkout_root"], Value::Null);
    assert_eq!(worktree_output["vault_scope"], "repo");
    assert_eq!(worktree_output["vault_scope_id"], SCOPE_ID);
    assert!(!fixture.base.exists(), "scope derivation must not create");
}

#[test]
fn linked_worktree_maps_nested_jig_root_onto_main_checkout() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &["services/api"]);
    let worktree = fixture.linked_worktree(&main, "wt");

    let main_nested = status_for(&main.join("services/api")).unwrap();
    let worktree_nested = status_for(&worktree.join("services/api")).unwrap();

    assert_eq!(home_of(&worktree_nested), home_of(&main_nested));
    assert_ne!(home_of(&worktree_nested), fixture.home_for(&main));
    assert_eq!(
        worktree_nested["vault_main_checkout_root"],
        main.join("services/api").display().to_string()
    );
}

#[test]
fn worktree_root_missing_from_main_checkout_maps_to_literal_path() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    fs::create_dir_all(worktree.join("services/new")).unwrap();

    let output = status_for(&worktree.join("services/new")).unwrap();

    assert_eq!(
        home_of(&output),
        fixture.home_for(&main.join("services/new"))
    );
    assert!(!main.join("services").exists());
}

#[test]
fn relative_worktree_pointers_resolve_to_main_checkout() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    fs::write(worktree.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
    fs::write(
        main.join(".git/worktrees/wt/gitdir"),
        "../../../../wt/.git\n",
    )
    .unwrap();

    let output = status_for(&worktree).unwrap();

    assert_eq!(home_of(&output), fixture.home_for(&main));
    assert_eq!(
        output["vault_main_checkout_root"],
        main.display().to_string()
    );
}

#[test]
fn unusable_git_pointers_keep_checkout_scope() {
    let fixture = Fixture::new();
    let checkout = fixture.root.join("checkout");
    fs::create_dir(&checkout).unwrap();
    fs::write(checkout.join("plain-file"), "fixture\n").unwrap();
    let file = checkout.join("plain-file");

    for pointer in [
        "not a pointer\n".to_owned(),
        "gitdir: missing/admin\n".to_owned(),
        format!("gitdir: {}\n", file.display()),
        format!("gitdir: {}\n", file.join("below").display()),
    ] {
        fs::write(checkout.join(".git"), &pointer).unwrap();
        let output = status_for(&checkout).unwrap();
        assert_eq!(home_of(&output), fixture.home_for(&checkout), "{pointer}");
        assert_eq!(output["vault_main_checkout_root"], Value::Null);
    }
}

#[test]
fn submodule_shaped_gitdir_keeps_checkout_scope() {
    let fixture = Fixture::new();
    let superproject = fixture.main_checkout("super", &[]);
    let modules = superproject.join(".git/modules/sub");
    fs::create_dir_all(&modules).unwrap();
    fs::write(modules.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(modules.join("config"), "[core]\n\tbare = false\n").unwrap();
    let submodule = superproject.join("sub");
    fs::create_dir(&submodule).unwrap();
    fs::write(submodule.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();

    let output = status_for(&submodule).unwrap();

    assert_eq!(home_of(&output), fixture.home_for(&submodule));
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
}

#[test]
fn bare_repository_worktrees_keep_checkout_scope() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let bare = fixture.root.join("bare.git");
    let dot_git_bare = fixture.root.join("holder/.git");
    git(
        &fixture.root,
        &[
            "clone",
            "-q",
            "--bare",
            main.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    git(
        &fixture.root,
        &[
            "clone",
            "-q",
            "--bare",
            main.to_str().unwrap(),
            dot_git_bare.to_str().unwrap(),
        ],
    );

    for (common, name) in [(&bare, "bare-wt"), (&dot_git_bare, "holder-wt")] {
        let worktree = fixture.root.join(name);
        git(
            &fixture.root,
            &[
                "--git-dir",
                common.to_str().unwrap(),
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
            ],
        );
        let output = status_for(&worktree).unwrap();
        assert_eq!(home_of(&output), fixture.home_for(&worktree), "{name}");
        assert_eq!(output["vault_main_checkout_root"], Value::Null);
    }
}

#[test]
fn nested_independent_repository_in_main_checkout_keeps_checkout_scope() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    // An untracked clone inside the main checkout is its own repository.
    fs::create_dir_all(main.join("vendor/lib")).unwrap();
    git(&main.join("vendor/lib"), &["init", "-q", "--template="]);
    fs::create_dir_all(worktree.join("vendor/lib")).unwrap();

    let output = status_for(&worktree.join("vendor/lib")).unwrap();

    assert_eq!(
        home_of(&output),
        fixture.home_for(&worktree.join("vendor/lib"))
    );
    assert_ne!(home_of(&output), fixture.home_for(&main.join("vendor/lib")));
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
}

#[test]
fn explicit_home_and_global_ignore_worktree_linkage() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    let explicit = fixture.root.join("explicit");

    let mut options = VaultRuntimeOptions::repo(SCOPE_ID, REPO_NAME, &worktree);
    options.home = Some(explicit.clone());
    let output = status(VaultStatusRequest { vault: options }).unwrap();
    assert_eq!(output["vault_scope"], "explicit-home");
    assert_eq!(home_of(&output), explicit);
    assert_eq!(output["vault_main_checkout_root"], Value::Null);

    let output = status(VaultStatusRequest {
        vault: VaultRuntimeOptions {
            home: None,
            scope: VaultScopeSelection::Global,
        },
    })
    .unwrap();
    assert_eq!(output["vault_scope"], "global");
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
}

#[test]
fn legacy_scope_recovery_is_routed_to_the_operator() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let legacy_home = fixture.base.join("scopes").join(SCOPE_ID);
    write_vault_file(&legacy_home);

    let error = format!("{:#}", status_for(&main).unwrap_err());

    assert!(
        error.contains("legacy repo-scoped vault data exists"),
        "{error}"
    );
    assert!(
        error.contains(&format!("--home {}", legacy_home.display())),
        "{error}"
    );
    assert_operator_routed(&error);
}
