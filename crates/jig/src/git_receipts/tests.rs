use super::*;
use std::cell::Cell;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::tempdir;

const REDIRECT_HELPER_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_HELPER";
const REDIRECT_HELPER_ROOT_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_ROOT";
const REDIRECT_HELPER_WHOLE_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_WHOLE";
const REDIRECT_HELPER_SCOPE_ENV: &str = "JIG_TEST_GIT_RECEIPT_REDIRECT_SCOPE";
const REDIRECT_HELPER_TEST: &str = "git_receipts::tests::repository_redirect_environment_helper";
// Apple rejects invalid-byte path components with EILSEQ before these
// filesystem-backed fixtures can exercise Jig's path handling.
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_ENV: &str = "JIG_TEST_NON_UTF8_TMPDIR_HELPER";
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_ROOT_ENV: &str = "JIG_TEST_NON_UTF8_TMPDIR_ROOT";
#[cfg(all(unix, not(target_vendor = "apple")))]
const NON_UTF8_TMPDIR_HELPER_TEST: &str =
    "git_receipts::tests::canonical_diff_order_file_preserves_non_utf8_temporary_directory_helper";

include!("tests_parts/part_01.rs");
include!("tests_parts/part_02.rs");
include!("tests_parts/part_03.rs");
include!("tests_parts/part_04.rs");

#[path = "tests_parts/comparison_scope.rs"]
mod comparison_scope;
#[path = "tests_parts/comparison_scope_regressions.rs"]
mod comparison_scope_regressions;

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
