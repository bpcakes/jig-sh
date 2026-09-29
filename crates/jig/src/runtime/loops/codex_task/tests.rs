use tempfile::tempdir;

use super::*;
use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

#[test]
fn prompt_reader_rejects_oversized_and_non_regular_files() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    fs::create_dir_all(temp.path().join("tasks/directory-prompt")).unwrap();
    fs::write(
        temp.path().join("tasks/oversized.md"),
        vec![b'x'; MAX_PROMPT_BYTES as usize + 1],
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let oversized = read_prompt(&ctx, Path::new("tasks/oversized.md"))
        .unwrap_err()
        .to_string();
    let directory = read_prompt(&ctx, Path::new("tasks/directory-prompt"))
        .unwrap_err()
        .to_string();

    assert!(oversized.contains("exceeds"), "{oversized}");
    assert!(directory.contains("not a regular file"), "{directory}");
}

#[test]
fn prompt_reader_reports_growth_past_limit_before_partial_utf8() {
    let mut prompt = vec![b'x'; MAX_PROMPT_BYTES as usize];
    prompt.push(0xc3);

    let error = decode_prompt(prompt, Path::new("tasks/growing.md"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("exceeds"), "{error}");
    assert!(!error.contains("UTF-8"), "{error}");
}

#[cfg(unix)]
#[test]
fn prompt_reader_preserves_repository_internal_symlinks() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    fs::create_dir_all(temp.path().join("tasks/prompts")).unwrap();
    fs::write(
        temp.path().join("tasks/prompts/actual.md"),
        "inside repository\n",
    )
    .unwrap();
    symlink("prompts/actual.md", temp.path().join("tasks/nightly.md")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let prompt = read_prompt(&ctx, Path::new("tasks/nightly.md")).unwrap();

    assert_eq!(prompt, "inside repository\n");
}

#[cfg(unix)]
#[test]
fn prompt_reader_rejects_absolute_symlink_targets() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(temp.path().join("tasks/actual.md"), "inside repository\n").unwrap();
    symlink(
        temp.path().join("tasks/actual.md"),
        temp.path().join("tasks/nightly.md"),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = read_prompt(&ctx, Path::new("tasks/nightly.md"))
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("must resolve inside the repository"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn prompt_reader_rejects_an_intermediate_escape_after_root_open() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(temp.path().join("tasks/nightly.md"), "inside repository\n").unwrap();
    fs::write(outside.path().join("nightly.md"), "outside repository\n").unwrap();
    let root = temp.path();
    let mut swapped = false;

    let error = open_prompt_file_with_observer(root, Path::new("tasks/nightly.md"), || {
        fs::rename(root.join("tasks"), root.join("tasks-original"))?;
        symlink(outside.path(), root.join("tasks"))?;
        swapped = true;
        Ok(())
    })
    .unwrap_err()
    .to_string();

    assert!(swapped);
    assert!(
        error.contains("must resolve inside the repository"),
        "{error}"
    );
}

#[test]
fn successful_worktree_cleanup_is_a_separate_finalization_phase() {
    let _env_lock = lock_env();
    let repo = tempdir().unwrap();
    let checkout_parent = tempdir().unwrap();
    TestRepoBuilder::new(repo.path())
        .required_commands(Vec::<String>::new())
        .write();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "fixture@example.com"],
        vec!["config", "user.name", "Fixture"],
        vec!["add", "."],
        vec!["commit", "-m", "fixture"],
    ] {
        let output = Command::new("git")
            .current_dir(repo.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let initial_head = String::from_utf8(
        Command::new("git")
            .current_dir(repo.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let checkout_path = checkout_parent.path().join("task");
    let output = Command::new("git")
        .current_dir(repo.path())
        .args([
            "worktree",
            "add",
            "--detach",
            checkout_path.to_str().unwrap(),
            &initial_head,
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let _git = EnvVarGuard::set(GIT_BIN_ENV, OsStr::new("git"));
    let ctx = RepoContext::load_from(repo.path()).unwrap();

    let completion = PreparedCheckout::Worktree {
        repo_root: repo.path().to_path_buf(),
        path: checkout_path.clone(),
        initial_head,
    }
    .finish(TaskOutcome::Succeeded, &ctx);

    assert!(completion.error.is_none(), "{:#?}", completion.error);
    assert_eq!(completion.report.retained_worktree(), None);
    assert!(!checkout_path.exists());
}
