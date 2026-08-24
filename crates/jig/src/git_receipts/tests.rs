use super::*;
use std::cell::Cell;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::tempdir;

const AMBIENT_REDIRECT_HELPER_REPOSITORY: &str = "JIG_GIT_RECEIPTS_AMBIENT_HELPER_REPOSITORY";
const AMBIENT_REDIRECT_HELPER_EXPECTED_HEAD: &str = "JIG_GIT_RECEIPTS_AMBIENT_HELPER_EXPECTED_HEAD";
const AMBIENT_REDIRECT_HELPER_EXPECTED_FINGERPRINT: &str =
    "JIG_GIT_RECEIPTS_AMBIENT_HELPER_EXPECTED_FINGERPRINT";
const AMBIENT_REDIRECT_HELPER_TEST: &str =
    "git_receipts::tests::source_authority_ambient_redirect_helper";

#[test]
fn read_only_git_commands_disable_optional_locks() {
    let mut command = Command::new("git");
    command.env("GIT_OPTIONAL_LOCKS", "1");

    configure_read_only_git_environment(&mut command);

    assert_eq!(
        command
            .get_envs()
            .find(|(name, _)| *name == OsStr::new("GIT_OPTIONAL_LOCKS"))
            .and_then(|(_, value)| value),
        Some(OsStr::new("0"))
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_before_fingerprint_git_spawn_remains_typed() {
    let temp = tempdir().unwrap();
    let calls = Cell::new(0);
    let error = repo_worktree_fingerprint_with_cancellation(temp.path(), &|| {
        let current = calls.get();
        calls.set(current + 1);
        current == 1
    })
    .unwrap_err();

    assert!(is_git_receipt_collection_cancellation(&error), "{error:#}");
    assert_eq!(calls.get(), 2);
}

#[test]
fn source_authority_ignores_ambient_repository_redirects() {
    let repository = tempdir().unwrap();
    let redirected = tempdir().unwrap();
    for (root, contents) in [
        (repository.path(), "repository"),
        (redirected.path(), "redirected"),
    ] {
        run_git(root, &["init"]);
        run_git(root, &["config", "user.email", "fixture@example.com"]);
        run_git(root, &["config", "user.name", "Fixture"]);
        std::fs::write(root.join("tracked.txt"), contents).unwrap();
        run_git(root, &["add", "tracked.txt"]);
        run_git(root, &["commit", "-m", "fixture"]);
    }
    let expected_head = git_text(repository.path(), &["rev-parse", "HEAD"]);
    let expected_fingerprint = repo_worktree_fingerprint(repository.path()).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", AMBIENT_REDIRECT_HELPER_TEST, "--nocapture"])
        .env(
            AMBIENT_REDIRECT_HELPER_REPOSITORY,
            repository.path().as_os_str(),
        )
        .env(AMBIENT_REDIRECT_HELPER_EXPECTED_HEAD, expected_head)
        .env(
            AMBIENT_REDIRECT_HELPER_EXPECTED_FINGERPRINT,
            expected_fingerprint,
        )
        .env("GIT_DIR", redirected.path().join(".git"))
        .env("GIT_WORK_TREE", redirected.path())
        .env("GIT_INDEX_FILE", redirected.path().join(".git/index"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "ambient redirect helper failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn source_authority_ambient_redirect_helper() {
    let Some(repository) = std::env::var_os(AMBIENT_REDIRECT_HELPER_REPOSITORY) else {
        return;
    };
    let repository = PathBuf::from(repository);
    let expected_head = std::env::var(AMBIENT_REDIRECT_HELPER_EXPECTED_HEAD).unwrap();
    let expected_fingerprint = std::env::var(AMBIENT_REDIRECT_HELPER_EXPECTED_FINGERPRINT).unwrap();

    assert_eq!(
        repository_source_snapshot(&repository).unwrap().head_commit,
        Some(expected_head)
    );
    assert_eq!(
        repo_worktree_fingerprint(&repository).unwrap(),
        expected_fingerprint
    );
}

#[test]
fn fingerprint_hash_staging_checks_cancellation_between_chunks() {
    let input = vec![b'x'; FINGERPRINT_HASH_WRITE_CHUNK * 3];
    let checks = Cell::new(0);
    let mut staged = Vec::new();

    let error = write_fingerprint_hash_input(&mut staged, &input, &|| {
        let current = checks.get();
        checks.set(current + 1);
        current >= 1
    })
    .unwrap_err();

    assert!(is_git_receipt_collection_cancellation(&error));
    assert_eq!(staged.len(), FINGERPRINT_HASH_WRITE_CHUNK);
}

#[test]
fn parse_diff_stat_output_counts_binary_files_without_swallowing_other_errors() {
    let diff_stat = parse_diff_stat_output("12\t3\tsrc/main.rs\n-\t-\tassets/logo.png\n").unwrap();
    assert_eq!(diff_stat.files, 2);
    assert_eq!(diff_stat.insertions, 12);
    assert_eq!(diff_stat.deletions, 3);
}

#[test]
fn parse_diff_stat_output_rejects_invalid_counts() {
    let error = parse_diff_stat_output("oops\t3\tsrc/main.rs\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Invalid git diff --numstat insertions count"));
}

#[test]
fn collect_git_receipt_metadata_records_git_failures() {
    let temp = tempdir().unwrap();
    let metadata = collect_git_receipt_metadata(temp.path());

    assert!(metadata.changed_paths.is_empty());
    assert_eq!(metadata.changed_path_count, None);
    assert!(!metadata.changed_paths_truncated);
    assert_eq!(metadata.changed_paths_digest, None);
    assert_eq!(metadata.diff_stat.files, 0);
    assert!(metadata.git_status_error.is_some());
    assert!(metadata.git_diff_stat_error.is_some());
    assert!(metadata.worktree_fingerprint.is_none());
    assert!(metadata.worktree_fingerprint_error.is_some());
}

#[test]
fn cancelled_receipt_metadata_starts_no_git_subcollection() {
    let temp = tempdir().unwrap();
    let checks = Cell::new(0);

    let metadata = collect_git_receipt_metadata_with_cancellation(temp.path(), &|| {
        checks.set(checks.get() + 1);
        true
    });

    assert!(metadata.changed_paths.is_empty());
    assert_eq!(metadata.changed_path_count, None);
    assert_eq!(metadata.diff_stat.files, 0);
    assert!(
        metadata
            .git_status_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert!(
        metadata
            .git_diff_stat_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert!(metadata.worktree_fingerprint.is_none());
    assert!(
        metadata
            .worktree_fingerprint_error
            .as_deref()
            .is_some_and(|error| error.contains("collection was cancelled"))
    );
    assert_eq!(checks.get(), 3);
}

#[test]
fn changed_paths_preserve_spaces_and_rename_paths() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("old name.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "old name.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    run_git(temp.path(), &["mv", "old name.txt", "new name.txt"]);
    std::fs::write(temp.path().join("loose note.txt"), "untracked").unwrap();

    let paths = repo_changed_paths(temp.path()).unwrap();

    assert!(paths.contains(&"new name.txt".to_string()));
    assert!(paths.contains(&"old name.txt".to_string()));
    assert!(paths.contains(&"loose note.txt".to_string()));
}

#[test]
fn affected_changed_paths_include_commits_and_the_current_worktree() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("api")).unwrap();
    std::fs::create_dir_all(temp.path().join("web")).unwrap();
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join("api/base.go"), "package api\n").unwrap();
    std::fs::write(temp.path().join("web/base.ts"), "export {};\n").unwrap();
    std::fs::write(temp.path().join(".agent/state/runs.jsonl"), "base\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "base fixture"]);
    let base = git_text(temp.path(), &["rev-parse", "HEAD"]);

    std::fs::write(temp.path().join("api/committed.go"), "package api\n").unwrap();
    run_git(temp.path(), &["add", "api/committed.go"]);
    run_git(temp.path(), &["commit", "-m", "committed change"]);
    std::fs::write(temp.path().join("web/base.ts"), "export const value = 1;\n").unwrap();
    std::fs::write(temp.path().join("api/untracked.go"), "package api\n").unwrap();
    std::fs::write(
        temp.path().join(".agent/state/runs.jsonl"),
        "base\nruntime\n",
    )
    .unwrap();

    let paths = repo_changed_paths_since(temp.path(), &base).unwrap();

    assert_eq!(
        paths,
        ["api/committed.go", "api/untracked.go", "web/base.ts"]
    );
    assert!(repo_changed_paths_since(temp.path(), "HEAD^{commit}").is_ok());
}

#[test]
fn affected_changed_paths_separate_presence_only_ignored_dotenv_inputs() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join("api")).unwrap();
    std::fs::write(
        temp.path().join(".gitignore"),
        ".env\n.env.*\n**/.env\n**/.env.*\ngenerated/\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("README.md"), "fixture\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "base fixture"]);
    let base = git_text(temp.path(), &["rev-parse", "HEAD"]);
    std::fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://example\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("api/.env.local"), "FEATURE=true\n").unwrap();
    std::fs::create_dir_all(temp.path().join("generated")).unwrap();
    std::fs::write(temp.path().join("generated/.env"), "GENERATED=true\n").unwrap();
    std::fs::write(temp.path().join("ignored.txt"), "not an execution input\n").unwrap();
    std::fs::write(
        temp.path().join(".gitignore"),
        ".env\n.env.*\n**/.env\n**/.env.*\ngenerated/\nignored.txt\n",
    )
    .unwrap();

    let paths = repo_changed_paths_since(temp.path(), &base).unwrap();
    let observed = repo_observed_ignored_dotenv_paths(temp.path()).unwrap();

    assert_eq!(paths, [".gitignore"]);
    assert_eq!(observed, [".env", "api/.env.local"]);
}

#[test]
fn affected_changed_paths_exclude_staged_agent_metadata_and_runtime_state() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join(".agent/jig-contract.json"), "{}\n").unwrap();
    std::fs::write(temp.path().join(".agent/state/runs.jsonl"), "base\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "base fixture"]);
    let base = git_text(temp.path(), &["rev-parse", "HEAD"]);

    std::fs::write(
        temp.path().join(".agent/jig-contract.json"),
        "{\"version\":6}\n",
    )
    .unwrap();
    run_git(temp.path(), &["add", ".agent/jig-contract.json"]);
    std::fs::write(
        temp.path().join(".agent/state/runs.jsonl"),
        "base\nruntime\n",
    )
    .unwrap();

    let paths = repo_changed_paths_since(temp.path(), &base).unwrap();

    assert!(paths.is_empty());
}

#[test]
fn affected_changed_paths_exclude_committed_agent_metadata() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join(".agent")).unwrap();
    std::fs::write(temp.path().join(".agent/jig-contract.json"), "{}\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "base fixture"]);
    let base = git_text(temp.path(), &["rev-parse", "HEAD"]);
    std::fs::write(
        temp.path().join(".agent/jig-contract.json"),
        "{\"version\":6}\n",
    )
    .unwrap();
    run_git(temp.path(), &["add", ".agent/jig-contract.json"]);
    run_git(temp.path(), &["commit", "-m", "contract change"]);

    let paths = repo_changed_paths_since(temp.path(), &base).unwrap();

    assert!(paths.is_empty());
}

#[test]
fn affected_changed_paths_exclude_untracked_agent_metadata() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("README.md"), "fixture\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "base fixture"]);
    let base = git_text(temp.path(), &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(temp.path().join(".agent")).unwrap();
    std::fs::write(temp.path().join(".agent/jig-contract.json"), "{}\n").unwrap();

    let paths = repo_changed_paths_since(temp.path(), &base).unwrap();

    assert!(paths.is_empty());
}

#[test]
fn affected_changed_paths_reject_invalid_bases_without_option_injection() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("base.txt"), "base\n").unwrap();
    run_git(temp.path(), &["add", "base.txt"]);
    run_git(temp.path(), &["commit", "-m", "base fixture"]);

    let error = repo_changed_paths_since(temp.path(), "--help")
        .unwrap_err()
        .to_string();

    assert!(error.contains("Invalid affected Git base"));
    assert!(error.contains("--help"));
}

#[test]
fn receipt_metadata_excludes_agent_state_from_paths_and_diff_stat() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join("src.rs"), "one\n").unwrap();
    std::fs::write(temp.path().join(".agent/state/receipts.jsonl"), "old\n").unwrap();
    run_git(temp.path(), &["add", "."]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::write(temp.path().join("src.rs"), "one\ntwo\n").unwrap();
    std::fs::write(
        temp.path().join(".agent/state/receipts.jsonl"),
        "old\nnew\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("note.txt"), "untracked\n").unwrap();
    std::fs::write(
        temp.path().join(".agent/state/untracked.jsonl"),
        "ignored by receipt metadata\n",
    )
    .unwrap();

    let metadata = collect_git_receipt_metadata_without_worktree_fingerprint(temp.path());

    assert_eq!(metadata.changed_paths, ["note.txt", "src.rs"]);
    assert_eq!(metadata.changed_path_count, Some(2));
    assert!(!metadata.changed_paths_truncated);
    assert!(metadata.changed_paths_digest.is_some());
    assert_eq!(metadata.diff_stat.files, 1);
    assert_eq!(metadata.diff_stat.insertions, 1);
    assert_eq!(metadata.diff_stat.deletions, 0);
}

#[test]
fn changed_path_preview_is_bounded_sorted_and_digest_covers_the_full_set() {
    let paths = (0..105)
        .rev()
        .map(|index| format!("src/path-{index:03}.rs"))
        .chain(["src/path-042.rs".to_string()])
        .collect::<Vec<_>>();
    let bounded = bounded_changed_paths(paths.clone());
    let reordered = bounded_changed_paths(paths.into_iter().rev().collect());

    assert_eq!(bounded.preview.len(), MAX_RECEIPT_CHANGED_PATHS);
    assert_eq!(bounded.total, 105);
    assert!(bounded.truncated);
    assert_eq!(bounded.preview[0], "src/path-000.rs");
    assert_eq!(bounded.preview[99], "src/path-099.rs");
    assert!(bounded.digest.starts_with("sha256:"));
    assert_eq!(bounded.digest, reordered.digest);
    assert_eq!(bounded.preview, reordered.preview);

    let preview_only_digest = changed_paths_digest(&bounded.preview);
    assert_ne!(bounded.digest, preview_only_digest);
}

#[cfg(unix)]
#[test]
fn porcelain_z_parser_preserves_non_utf8_path_bytes() {
    let entries = parse_porcelain_status_z(b"?? bad\xFFname\0").unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].path.as_os_str().as_encoded_bytes(),
        b"bad\xFFname"
    );
}

#[test]
fn worktree_fingerprint_changes_when_untracked_file_content_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::write(temp.path().join("new.txt"), "one").unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();
    std::fs::write(temp.path().join("new.txt"), "two").unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[test]
fn worktree_fingerprint_changes_when_ignored_dotenv_content_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join(".gitignore"), ".env\n").unwrap();
    run_git(temp.path(), &["add", ".gitignore"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::write(temp.path().join(".env"), "EXAMPLE_VALUE=one\n").unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::write(temp.path().join(".env"), "EXAMPLE_VALUE=two\n").unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[test]
fn worktree_fingerprint_prunes_dotenv_files_in_wholly_ignored_directories() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join(".gitignore"), "generated/\n").unwrap();
    run_git(temp.path(), &["add", ".gitignore"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    std::fs::create_dir_all(temp.path().join("generated")).unwrap();
    std::fs::write(temp.path().join("generated/.env"), "VALUE=one\n").unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::write(temp.path().join("generated/.env"), "VALUE=two\n").unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_eq!(first, second);
    assert!(
        repo_observed_ignored_dotenv_paths(temp.path())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn source_projections_observe_ignored_dotenv_and_dirty_submodule_edits() {
    let _env = crate::test_env::lock_env();
    let submodule = tempdir().unwrap();
    run_git(submodule.path(), &["init"]);
    run_git(
        submodule.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(submodule.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(submodule.path().join("source.txt"), "committed\n").unwrap();
    std::fs::write(submodule.path().join(".gitignore"), ".env\n").unwrap();
    run_git(submodule.path(), &["add", "source.txt", ".gitignore"]);
    run_git(submodule.path(), &["commit", "-m", "submodule fixture"]);

    let repository = tempdir().unwrap();
    run_git(repository.path(), &["init"]);
    run_git(
        repository.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(repository.path(), &["config", "user.name", "Fixture"]);
    run_git(
        repository.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.path().to_str().unwrap(),
            "vendor/example",
        ],
    );
    run_git(repository.path(), &["commit", "-am", "parent fixture"]);
    let base = git_text(repository.path(), &["rev-parse", "HEAD"]);

    std::fs::write(
        repository.path().join("vendor/example/.env"),
        "EXAMPLE_VALUE=local\n",
    )
    .unwrap();
    assert!(
        repo_changed_paths_since(repository.path(), &base)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repo_observed_ignored_dotenv_paths(repository.path()).unwrap(),
        ["vendor/example/.env"]
    );

    let submodule_source = repository.path().join("vendor/example/source.txt");
    std::fs::write(&submodule_source, "first dirty value\n").unwrap();
    let first = repo_worktree_fingerprint(repository.path()).unwrap();
    std::fs::write(&submodule_source, "second dirty value\n").unwrap();
    let second = repo_worktree_fingerprint(repository.path()).unwrap();

    assert_ne!(first, second);
}

#[test]
fn worktree_fingerprint_changes_when_the_clean_head_commit_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "one\n").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "first fixture"]);
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::write(temp.path().join("tracked.txt"), "two\n").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "second fixture"]);
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[test]
fn worktree_fingerprint_ignores_clean_commits_that_only_change_agent_state() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    std::fs::write(temp.path().join("tracked.txt"), "source\n").unwrap();
    std::fs::write(temp.path().join(".agent/state/receipts.jsonl"), "first\n").unwrap();
    run_git(temp.path(), &["add", "tracked.txt", ".agent"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::write(
        temp.path().join(".agent/state/receipts.jsonl"),
        "first\nsecond\n",
    )
    .unwrap();
    run_git(temp.path(), &["add", ".agent/state/receipts.jsonl"]);
    run_git(temp.path(), &["commit", "-m", "record agent evidence"]);
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_eq!(first, second);
}

#[test]
fn worktree_fingerprint_changes_when_large_untracked_file_content_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let large_path = temp.path().join("large.bin");
    let fixed_mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::write(
        &large_path,
        vec![b'a'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::File::open(&large_path)
        .unwrap()
        .set_modified(fixed_mtime)
        .unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();

    std::fs::write(
        &large_path,
        vec![b'b'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::File::open(&large_path)
        .unwrap()
        .set_modified(fixed_mtime)
        .unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

    assert_ne!(first, second);
}

#[cfg(unix)]
#[test]
fn worktree_fingerprint_changes_when_untracked_symlink_target_changes() {
    let _env = crate::test_env::lock_env();
    let temp = tempdir().unwrap();
    run_git(temp.path(), &["init"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.com"],
    );
    run_git(temp.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
    run_git(temp.path(), &["add", "tracked.txt"]);
    run_git(temp.path(), &["commit", "-m", "initial fixture"]);
    let first_target = temp.path().join("outside-one");
    let second_target = temp.path().join("outside-two");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&first_target, &link).unwrap();
    let first = repo_worktree_fingerprint(temp.path()).unwrap();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second_target, &link).unwrap();
    let second = repo_worktree_fingerprint(temp.path()).unwrap();

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

fn git_text(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
