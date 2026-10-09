use super::*;
use std::cell::Cell;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::tempdir;

mod comparison_scope;
mod comparison_scope_regressions;
mod fail_closed;
mod fingerprint_changes;
mod git_isolation;
mod non_utf8_paths;
mod tracker_state;

#[test]
fn repository_source_identity_ignores_agent_only_commits() {
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init", "-q"]);
    run_git(temp.path(), &["config", "user.name", "Jig Test"]);
    run_git(
        temp.path(),
        &["config", "user.email", "jig@example.invalid"],
    );
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join("source.txt"), "source\n").unwrap();
    std::fs::write(temp.path().join(".agent/state/receipts.jsonl"), "first\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial", "-q"]);
    let first = repository_source_snapshot(temp.path())
        .unwrap()
        .worktree_fingerprint;

    std::fs::write(temp.path().join(".agent/state/receipts.jsonl"), "second\n").unwrap();
    run_git(temp.path(), &["add", ".agent/state/receipts.jsonl"]);
    run_git(temp.path(), &["commit", "-m", "state", "-q"]);
    let second = repository_source_snapshot(temp.path())
        .unwrap()
        .worktree_fingerprint;

    assert_eq!(first, second);
}

#[test]
fn repository_source_identity_changes_with_committed_source() {
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init", "-q"]);
    run_git(temp.path(), &["config", "user.name", "Jig Test"]);
    run_git(
        temp.path(),
        &["config", "user.email", "jig@example.invalid"],
    );
    std::fs::write(temp.path().join("source.txt"), "first\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "first", "-q"]);
    let first = repository_source_snapshot(temp.path())
        .unwrap()
        .worktree_fingerprint;

    std::fs::write(temp.path().join("source.txt"), "second\n").unwrap();
    run_git(temp.path(), &["add", "source.txt"]);
    run_git(temp.path(), &["commit", "-m", "second", "-q"]);
    let second = repository_source_snapshot(temp.path())
        .unwrap()
        .worktree_fingerprint;

    assert_ne!(first, second);
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
