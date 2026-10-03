//! A linked worktree's own vault from an earlier Jig version must block the
//! switch to the shared namespace instead of being silently shadowed.

use std::fs;
use std::path::PathBuf;

use crate::test_env::{CurrentDirGuard, EnvVarGuard};

use super::super::super::VAULT_HOME_ENV;
use super::{Fixture, assert_operator_routed, home_of, status_for, write_vault_file};

struct Orphan {
    fixture: Fixture,
    main: PathBuf,
    worktree: PathBuf,
    own_home: PathBuf,
    shared_home: PathBuf,
}

fn orphan() -> Orphan {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    let own_home = fixture.home_for(&worktree);
    let shared_home = fixture.home_for(&main);
    write_vault_file(&own_home);
    Orphan {
        fixture,
        main,
        worktree,
        own_home,
        shared_home,
    }
}

fn assert_names_both_homes(orphan: &Orphan, error: &str) {
    assert!(
        error.contains("already has its own repo-scoped vault"),
        "{error}"
    );
    assert!(error.contains("Refusing to switch away"), "{error}");
    assert!(
        error.contains(&orphan.own_home.display().to_string()),
        "{error}"
    );
    assert!(
        error.contains(&orphan.shared_home.display().to_string()),
        "{error}"
    );
    assert!(
        error.contains(&orphan.main.display().to_string()),
        "{error}"
    );
    assert_operator_routed(error);
}

#[test]
fn orphan_blocks_shared_namespace_when_shared_home_is_absent() {
    let orphan = orphan();

    let error = format!("{:#}", status_for(&orphan.worktree).unwrap_err());

    assert_names_both_homes(&orphan, &error);
    assert!(
        error.contains(&format!(
            "rename {} to exactly {}",
            orphan.own_home.display(),
            orphan.shared_home.display()
        )),
        "{error}"
    );
    assert!(error.contains("--home"), "{error}");
    assert!(!orphan.shared_home.exists());
}

#[test]
fn orphan_requires_removing_an_empty_shared_home_first() {
    let orphan = orphan();
    fs::create_dir_all(&orphan.shared_home).unwrap();

    let error = format!("{:#}", status_for(&orphan.worktree).unwrap_err());

    assert_names_both_homes(&orphan, &error);
    assert!(error.contains("exists but has no vault.json"), "{error}");
    assert!(error.contains("remove that directory first"), "{error}");
    assert!(
        error.contains(&format!(
            "rename {} to exactly {}",
            orphan.own_home.display(),
            orphan.shared_home.display()
        )),
        "{error}"
    );
}

#[test]
fn orphan_reports_when_both_vaults_hold_data() {
    let orphan = orphan();
    write_vault_file(&orphan.shared_home);

    let error = format!("{:#}", status_for(&orphan.worktree).unwrap_err());

    assert_names_both_homes(&orphan, &error);
    assert!(error.contains("Both vaults hold data"), "{error}");
    assert!(
        error.contains(&format!("--home {}", orphan.own_home.display())),
        "{error}"
    );
}

#[test]
fn renaming_orphan_to_shared_home_restores_resolution() {
    let orphan = orphan();
    fs::create_dir_all(orphan.shared_home.parent().unwrap()).unwrap();
    fs::rename(&orphan.own_home, &orphan.shared_home).unwrap();

    let output = status_for(&orphan.worktree).unwrap();

    assert_eq!(home_of(&output), orphan.shared_home);
    assert_eq!(output["exists"], true);
    assert_eq!(
        output["vault_main_checkout_root"],
        orphan.main.display().to_string()
    );
}

#[test]
fn orphan_paths_are_absolute_with_a_relative_vault_home() {
    let orphan = orphan();
    let _cwd = CurrentDirGuard::set(&orphan.fixture.root);
    let _relative = EnvVarGuard::set(VAULT_HOME_ENV, "vault-base");

    let error = format!("{:#}", status_for(&orphan.worktree).unwrap_err());

    assert_names_both_homes(&orphan, &error);
}
