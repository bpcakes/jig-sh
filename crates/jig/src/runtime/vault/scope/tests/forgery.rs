//! Adversarial linkage cases: a checkout must never reach another repository's
//! namespace through forged Git metadata or symlinks.

use std::fs;
use std::os::unix::fs::symlink;

use serde_json::Value;

use super::{Fixture, assert_operator_routed, git, home_of, status_for, write_vault_file};

fn assert_unverified(error: &str) {
    assert!(
        error.contains("could not verify it as a linked worktree"),
        "{error}"
    );
    assert!(error.contains("git worktree repair"), "{error}");
    assert!(error.contains("--home"), "{error}");
    assert_operator_routed(error);
}

#[test]
fn forged_pointer_to_another_worktrees_admin_dir_fails_closed() {
    let fixture = Fixture::new();
    let victim = fixture.main_checkout("victim", &[]);
    fixture.linked_worktree(&victim, "victim-wt");
    let victim_home = fixture.home_for(&victim);
    write_vault_file(&victim_home);
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

    let error = format!("{:#}", status_for(&intruder).unwrap_err());

    assert_unverified(&error);
    assert!(error.contains("back-link"), "{error}");
    assert_eq!(fs::read(victim_home.join("vault.json")).unwrap(), b"{}");
}

#[test]
fn forged_admin_dir_outside_common_worktrees_fails_closed() {
    let fixture = Fixture::new();
    let victim = fixture.main_checkout("victim", &[]);
    let intruder = fixture.root.join("intruder");
    let admin = intruder.join("fake/worktrees/x");
    fs::create_dir_all(&admin).unwrap();
    fs::write(
        admin.join("commondir"),
        format!("{}\n", victim.join(".git").display()),
    )
    .unwrap();
    fs::write(
        admin.join("gitdir"),
        format!("{}\n", intruder.join(".git").display()),
    )
    .unwrap();
    fs::write(
        intruder.join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .unwrap();

    let error = format!("{:#}", status_for(&intruder).unwrap_err());

    assert_unverified(&error);
    assert!(error.contains("is not directly inside"), "{error}");
}

#[test]
fn commondir_naming_another_repository_fails_closed() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let other = fixture.main_checkout("other", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    fs::write(
        main.join(".git/worktrees/wt/commondir"),
        format!("{}\n", other.join(".git").display()),
    )
    .unwrap();

    let error = format!("{:#}", status_for(&worktree).unwrap_err());

    assert_unverified(&error);
}

#[test]
fn moved_worktree_fails_closed_until_repaired() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    let moved = fixture.root.join("wt-moved");
    fs::rename(&worktree, &moved).unwrap();

    let error = format!("{:#}", status_for(&moved).unwrap_err());
    assert_unverified(&error);
    assert!(error.contains(&moved.display().to_string()), "{error}");

    git(&moved, &["worktree", "repair"]);
    let output = status_for(&moved).unwrap();
    assert_eq!(home_of(&output), fixture.home_for(&main));
}

#[test]
fn symlinked_dot_git_keeps_checkout_scope() {
    let fixture = Fixture::new();
    let victim = fixture.main_checkout("victim", &[]);
    let victim_worktree = fixture.linked_worktree(&victim, "victim-wt");
    let intruder = fixture.root.join("intruder");
    fs::create_dir(&intruder).unwrap();
    symlink(victim_worktree.join(".git"), intruder.join(".git")).unwrap();

    let output = status_for(&intruder).unwrap();

    assert_eq!(home_of(&output), fixture.home_for(&intruder));
    assert_ne!(home_of(&output), fixture.home_for(&victim));
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
}

#[test]
fn symlink_in_main_checkout_cannot_redirect_shared_namespace() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    // `other` is a non-Git Jig repository that a branch-local symlink in the
    // main checkout points at; the worktree has a real directory there.
    let other = fixture.root.join("other");
    fs::create_dir(&other).unwrap();
    symlink("../other", main.join("evil")).unwrap();
    fs::create_dir(worktree.join("evil")).unwrap();

    let output = status_for(&worktree.join("evil")).unwrap();

    assert_eq!(home_of(&output), fixture.home_for(&main.join("evil")));
    assert_ne!(home_of(&output), fixture.home_for(&other));
}

#[test]
fn symlink_to_another_git_repository_keeps_checkout_scope() {
    let fixture = Fixture::new();
    let main = fixture.main_checkout("main", &[]);
    let worktree = fixture.linked_worktree(&main, "wt");
    let other = fixture.main_checkout("other", &[]);
    symlink("../other", main.join("evil")).unwrap();
    fs::create_dir(worktree.join("evil")).unwrap();

    let output = status_for(&worktree.join("evil")).unwrap();

    assert_eq!(home_of(&output), fixture.home_for(&worktree.join("evil")));
    assert_ne!(home_of(&output), fixture.home_for(&other));
    assert_eq!(output["vault_main_checkout_root"], Value::Null);
}

#[test]
fn self_contained_fake_common_dir_cannot_reach_other_repository() {
    let fixture = Fixture::new();
    let other = fixture.root.join("other");
    fs::create_dir(&other).unwrap();
    // Everything below `sandbox` is writable by a confined agent: a fake
    // common directory, a linked-worktree claim that passes the proof, and a
    // symlink from the fake main checkout to `other`.
    let sandbox = fixture.root.join("sandbox");
    let admin = sandbox.join("m/.git/worktrees/x");
    fs::create_dir_all(&admin).unwrap();
    fs::write(admin.join("commondir"), "../..\n").unwrap();
    fs::write(
        admin.join("gitdir"),
        format!("{}\n", sandbox.join(".git").display()),
    )
    .unwrap();
    fs::write(sandbox.join(".git"), "gitdir: m/.git/worktrees/x\n").unwrap();
    symlink("../../other", sandbox.join("m/evil")).unwrap();
    fs::create_dir(sandbox.join("evil")).unwrap();

    let output = status_for(&sandbox.join("evil")).unwrap();

    assert_ne!(home_of(&output), fixture.home_for(&other));
    assert_eq!(home_of(&output), fixture.home_for(&sandbox.join("m/evil")));
}
