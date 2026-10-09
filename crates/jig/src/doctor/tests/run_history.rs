use super::support::check_by_id;
use super::*;
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use super::support::write_doctor_fixture;
use crate::cli::format_doctor_summary_for_test as format_summary;
use crate::test_env::{CurrentDirGuard, lock_env};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctor_guides_existing_journals_to_local_history_without_mutation() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q"]);
    fs::create_dir_all(root.join(".agent/state")).unwrap();
    let journal = root.join(".agent/state/runs.jsonl");
    fs::write(&journal, "example local history\n").unwrap();
    git(root, &["add", ".agent/state/runs.jsonl"]);
    let index = fs::read(root.join(".git/index")).unwrap();

    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert_eq!(result.status, "tracked");
    assert!(!result.required);
    assert_eq!(result.data["ignored"], false);
    assert!(result.fix.as_deref().unwrap().contains(".gitignore"));
    assert!(
        result
            .fix
            .as_deref()
            .unwrap()
            .contains("git rm --cached -- .agent/state/runs.jsonl")
    );
    let fix = result.fix.as_deref().unwrap();
    assert!(fix.contains("Jig root (the directory containing `.jig.toml`)"));
    assert!(fix.contains("Before other clones pull the deletion commit, copy aside"));
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "example local history\n"
    );

    fs::write(root.join(".gitignore"), ".agent/state/runs.jsonl\n").unwrap();
    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert_eq!(result.status, "tracked");
    assert_eq!(result.data["ignored"], true);
    git(root, &["rm", "--cached", "--", ".agent/state/runs.jsonl"]);
    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert!(result.ok);
    assert_eq!(result.status, "local");
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "example local history\n"
    );

    fs::remove_file(root.join(".gitignore")).unwrap();
    let result = super::super::run_history::check_local_history(root, None).unwrap();
    assert_eq!(result.status, "not ignored");
    assert!(!result.fix.unwrap().contains("git rm"));
}

#[test]
fn public_doctor_reports_nested_jig_history_as_optional_without_mutation() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let git_root = temp.path();
    git(git_root, &["init", "-q"]);
    let root = git_root.join("ExampleProject");
    fs::create_dir(&root).unwrap();
    write_doctor_fixture(&root);
    fs::create_dir_all(root.join(".agent/state")).unwrap();
    let journal = root.join(".agent/state/runs.jsonl");
    let history = b"example local history\n";
    fs::write(&journal, history).unwrap();
    let ignore = root.join(".gitignore");
    let policy = b"# Example local ignore policy\n";
    fs::write(&ignore, policy).unwrap();
    git(git_root, &["add", "ExampleProject/.agent/state/runs.jsonl"]);
    let index = fs::read(git_root.join(".git/index")).unwrap();
    let _cwd = CurrentDirGuard::set(&root);

    let report = run_with_cancellation(&|| false).unwrap();

    assert_eq!(report["ok"], true, "{report:#}");
    let history_check = check_by_id(&report, "run_history");
    assert_eq!(history_check["status"], "tracked");
    assert_eq!(history_check["required"], false);
    assert_eq!(history_check["data"]["tracked"], true);
    assert_eq!(history_check["data"]["ignored"], false);
    assert_eq!(report["next_issue"]["id"], "run_history");
    assert_eq!(report["optional_setup"], history_check["fix"]);
    assert!(report["next_required_step"].is_null());
    let fix = report["optional_setup"].as_str().unwrap();
    assert!(fix.contains("Jig root (the directory containing `.jig.toml`)"));
    assert!(fix.contains("Before other clones pull the deletion commit, copy aside"));
    let summary = format_summary(&report);
    assert!(summary.contains("Local run history: optional setup"));
    assert!(summary.contains("Before other clones pull the deletion commit"));
    assert_eq!(fs::read(&journal).unwrap(), history);
    assert_eq!(fs::read(&ignore).unwrap(), policy);
    assert_eq!(fs::read(git_root.join(".git/index")).unwrap(), index);
}

#[test]
fn doctor_history_probe_skips_non_git_roots_and_reports_git_errors() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    assert!(super::super::run_history::check_local_history(temp.path(), None).is_none());
    fs::write(temp.path().join(".git"), "gitdir: missing-git-directory\n").unwrap();
    let result = super::super::run_history::check_local_history(temp.path(), None).unwrap();
    assert_eq!(result.status, "unverified");
    assert!(!result.ok);
}
