//! A linked worktree's own vault from an earlier Jig version keeps resolving
//! after the upgrade, so persisted secrets are never stranded; sharing starts
//! only once the operator migrates it.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::test_env::{CurrentDirGuard, EnvVarGuard};

use super::super::super::VAULT_HOME_ENV;
use super::{Fixture, assert_operator_routed, git, home_of, status_for, write_vault_file};

struct WorktreeLocal {
    fixture: Fixture,
    main: PathBuf,
    worktree: PathBuf,
    own_home: PathBuf,
    shared_home: PathBuf,
}

fn worktree_local() -> WorktreeLocal {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    let own_home = fixture.home_for(&worktree);
    let shared_home = fixture.home_for(&main);
    write_vault_file(&own_home);
    WorktreeLocal {
        fixture,
        main,
        worktree,
        own_home,
        shared_home,
    }
}

/// Resolves the worktree and asserts it kept its own vault, returning the
/// migration guidance.
fn kept_guidance(local: &WorktreeLocal) -> String {
    let output = status_for(&local.worktree).unwrap();
    assert_eq!(home_of(&output), local.own_home);
    assert_eq!(output["exists"], true);
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
    assert_eq!(output["vault_worktree_local"], true);
    let guidance = output["vault_worktree_local_guidance"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        guidance.contains("keeps its own repo-scoped vault"),
        "{guidance}"
    );
    for path in [&local.own_home, &local.shared_home, &local.main] {
        assert!(guidance.contains(&path.display().to_string()), "{guidance}");
    }
    assert_operator_routed(&guidance);
    guidance
}

#[test]
fn worktree_local_vault_keeps_resolving_when_shared_home_is_absent() {
    let local = worktree_local();

    let guidance = kept_guidance(&local);

    assert!(
        guidance.contains(&format!(
            "rename {} to exactly {}",
            local.own_home.display(),
            local.shared_home.display()
        )),
        "{guidance}"
    );
    // The kept vault is reachable from the worktree itself, so the guidance
    // no longer needs an explicit-home inspection step.
    assert!(
        !guidance.contains(&format!("--home {}", local.own_home.display())),
        "{guidance}"
    );
    assert!(!local.shared_home.exists());
}

#[test]
fn worktree_local_guidance_requires_removing_an_empty_shared_home_first() {
    let local = worktree_local();
    fs::create_dir_all(&local.shared_home).unwrap();

    let guidance = kept_guidance(&local);

    assert!(
        guidance.contains("exists but has no vault.json"),
        "{guidance}"
    );
    assert!(guidance.contains("remove that directory"), "{guidance}");
    assert!(
        guidance.contains(&format!(
            "rename {} to exactly {}",
            local.own_home.display(),
            local.shared_home.display()
        )),
        "{guidance}"
    );
}

#[test]
fn worktree_local_vault_wins_when_both_vaults_hold_data() {
    let local = worktree_local();
    write_vault_file(&local.shared_home);

    let guidance = kept_guidance(&local);

    assert!(guidance.contains("Both vaults hold data"), "{guidance}");
}

#[test]
fn moving_the_worktree_local_vault_starts_sharing() {
    let local = worktree_local();
    fs::create_dir_all(local.shared_home.parent().unwrap()).unwrap();
    fs::rename(&local.own_home, &local.shared_home).unwrap();

    let output = status_for(&local.worktree).unwrap();

    assert_eq!(home_of(&output), local.shared_home);
    assert_eq!(output["exists"], true);
    assert_eq!(
        output["vault_main_checkout_root"],
        local.main.display().to_string()
    );
    assert_eq!(output["vault_worktree_local"], false);
    assert_eq!(output["vault_worktree_local_guidance"], Value::Null);
}

#[test]
fn worktree_local_guidance_paths_are_absolute_with_a_relative_vault_home() {
    let local = worktree_local();
    let _cwd = CurrentDirGuard::set(&local.fixture.root);
    let _relative = EnvVarGuard::set(VAULT_HOME_ENV, "vault-base");

    let output = status_for(&local.worktree).unwrap();

    assert_eq!(output["vault_worktree_local"], true);
    let guidance = output["vault_worktree_local_guidance"].as_str().unwrap();
    for path in [&local.own_home, &local.shared_home] {
        assert!(path.is_absolute());
        assert!(guidance.contains(&path.display().to_string()), "{guidance}");
    }
}

#[test]
fn unverified_worktree_keeps_its_own_vault_until_repaired() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    let moved = fixture.root.join("wt-moved");
    fs::rename(&worktree, &moved).unwrap();
    let own_home = fixture.home_for(&moved);
    write_vault_file(&own_home);

    let output = status_for(&moved).unwrap();

    assert_eq!(home_of(&output), own_home);
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
    assert_eq!(output["vault_worktree_local"], true);
    let guidance = output["vault_worktree_local_guidance"].as_str().unwrap();
    assert!(guidance.contains("could not verify"), "{guidance}");
    assert!(guidance.contains("back-link"), "{guidance}");
    assert!(guidance.contains("git worktree repair"), "{guidance}");
    assert!(!guidance.contains("Refusing"), "{guidance}");
    assert!(
        guidance.contains(&own_home.display().to_string()),
        "{guidance}"
    );

    // A repaired link is verified, but the existing vault still wins until
    // the operator migrates it.
    git(&moved, &["worktree", "repair"]);
    let output = status_for(&moved).unwrap();
    assert_eq!(home_of(&output), own_home);
    let guidance = output["vault_worktree_local_guidance"].as_str().unwrap();
    assert!(
        guidance.contains(&fixture.home_for(&main).display().to_string()),
        "{guidance}"
    );
}

#[test]
fn forged_worktree_pointer_keeps_its_own_vault_and_never_reaches_the_victim() {
    let fixture = Fixture::new();
    let victim = fixture.main_checkout("victim", &[]);
    fixture.linked_worktree(&victim, "victim-wt");
    write_vault_file(&fixture.home_for(&victim));
    let intruder = fixture.root.join("intruder");
    fs::create_dir(&intruder).unwrap();
    fs::write(
        intruder.join(".git"),
        format!(
            "gitdir: {}\n",
            victim.join(".git/worktrees/victim-wt").display()
        ),
    )
    .unwrap();
    let intruder_home = fixture.home_for(&intruder);
    write_vault_file(&intruder_home);

    let output = status_for(&intruder).unwrap();

    assert_eq!(home_of(&output), intruder_home);
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
    let guidance = output["vault_worktree_local_guidance"].as_str().unwrap();
    assert!(
        !guidance.contains(&fixture.home_for(&victim).display().to_string()),
        "{guidance}"
    );
}
