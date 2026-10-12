use std::fs;

use tempfile::tempdir;

use super::*;
use crate::test_env::{EnvVarGuard, lock_env};
use jig_git::GIT_BIN_ENV;

const LEGACY_RECEIPT_JOURNAL: &str = ".agent/state/receipts.jsonl";

fn fixture() -> (tempfile::TempDir, RepoContext) {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    let git = std::process::Command::new("git")
        .current_dir(temp.path())
        .arg("init")
        .output()
        .unwrap();
    assert!(git.status.success(), "{git:?}");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    (temp, ctx)
}

fn commit_all(ctx: &RepoContext) -> String {
    git_stdout(ctx, ctx.root(), ["add", "."], &mut NoopExecutionObserver).unwrap();
    git_stdout(
        ctx,
        ctx.root(),
        [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.com",
            "commit",
            "-m",
            "fixture",
        ],
        &mut NoopExecutionObserver,
    )
    .unwrap();
    git_stdout(
        ctx,
        ctx.root(),
        ["rev-parse", "HEAD"],
        &mut NoopExecutionObserver,
    )
    .unwrap()
}

#[test]
fn unknown_final_repository_head_requires_attention_and_a_dispatch_stop() {
    use std::os::unix::fs::PermissionsExt as _;

    let _env_lock = lock_env();
    let (temp, ctx) = fixture();
    let git = temp.path().join("git-final-head-fails.sh");
    fs::write(
        &git,
        r#"#!/bin/sh
case " $* " in
  *" status --porcelain=v1 "*) exit 0 ;;
  *" rev-parse HEAD "*) exit 7 ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git, permissions).unwrap();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
    let checkout = PreparedCheckout::Repo {
        path: temp.path().to_path_buf(),
        initial_head: "initial-head".into(),
    };

    let completion = checkout.finish(TaskOutcome::Succeeded, &ctx);

    assert_eq!(
        completion.report.repository_revision_state(),
        RepositoryRevisionState::Unknown
    );
    assert!(completion.report.repository_requires_attention());
    assert!(
        completion
            .error
            .as_deref()
            .is_some_and(|error| error.contains("checkout HEAD"))
    );
    assert!(
        completion
            .report
            .repository_revision_state()
            .requires_dispatch_stop()
    );
}

#[test]
fn operational_state_is_reported_but_never_exempted_from_dirty_status() {
    let _env_lock = lock_env();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
    let (_temp, ctx) = fixture();
    fs::write(ctx.root().join(".gitignore"), ".agent/.cache/\n").unwrap();
    let initial_head = commit_all(&ctx);
    fs::create_dir_all(ctx.state_dir()).unwrap();
    fs::write(ctx.state_file("runs.jsonl"), "example operational change\n").unwrap();

    let result = PreparedCheckout::Repo {
        path: ctx.root().into(),
        initial_head,
    }
    .finish(TaskOutcome::Succeeded, &ctx);

    assert!(result.report.repository_requires_attention());
    let value = result.report.value();
    assert_eq!(value["dirty"], true);
    assert!(value.get("receipt_append_valid").is_none(), "{value:#}");
    assert_eq!(
        value["diagnostics"]["reasons"],
        json!(["operational_state_changes"]),
        "{value:#}"
    );
    assert_eq!(
        value["diagnostics"]["observed_paths"],
        json!([".agent/state/runs.jsonl"])
    );
}

#[test]
fn legacy_receipt_journal_changes_dirty_the_shared_checkout() {
    let _env_lock = lock_env();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
    let (_temp, ctx) = fixture();
    let journal = ctx.root().join(LEGACY_RECEIPT_JOURNAL);
    fs::create_dir_all(journal.parent().unwrap()).unwrap();
    fs::write(&journal, "{\"id\":\"receipt_committed\"}\n").unwrap();
    let initial_head = commit_all(&ctx);
    fs::write(
        &journal,
        "{\"id\":\"receipt_committed\"}\n{\"id\":\"receipt_uncommitted\"}\n",
    )
    .unwrap();

    let result = PreparedCheckout::Repo {
        path: ctx.root().into(),
        initial_head,
    }
    .finish(TaskOutcome::Succeeded, &ctx);

    assert!(result.report.repository_requires_attention());
    let value = result.report.value();
    assert_eq!(value["dirty"], true);
    assert_eq!(
        value["diagnostics"]["observed_paths"],
        json!([LEGACY_RECEIPT_JOURNAL])
    );
}

#[test]
fn isolated_task_changes_retain_its_worktree_without_dirtying_shared_checkout() {
    let _env_lock = lock_env();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, std::ffi::OsStr::new("git"));
    let (_temp, ctx) = fixture();
    let initial_head = commit_all(&ctx);
    let isolated = tempdir().unwrap();
    let path = isolated.path().join("example-task");
    git_stdout(
        &ctx,
        ctx.root(),
        [
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
        &mut NoopExecutionObserver,
    )
    .unwrap();
    fs::write(path.join("example-change.txt"), "changed\n").unwrap();

    let result = PreparedCheckout::Worktree {
        repo_root: ctx.root().into(),
        path,
        initial_head,
    }
    .finish(TaskOutcome::Succeeded, &ctx);

    assert!(!result.report.repository_requires_attention());
    let value = result.report.value();
    assert_eq!(value["mode"], "worktree");
    assert_eq!(value["retained"], true);
    assert_eq!(value["dirty"], true);
    assert!(!git_is_dirty(&ctx, ctx.root(), &mut NoopExecutionObserver).unwrap());
}
