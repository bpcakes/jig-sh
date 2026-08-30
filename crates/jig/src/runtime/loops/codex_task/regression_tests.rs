#[cfg(unix)]
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicUsize, Ordering};

use tempfile::tempdir;

use super::*;
use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

struct CancelAfterStart(AtomicUsize);

impl crate::execution::ExecutionObserver for CancelAfterStart {}

impl crate::execution::ExecutionCancellation for CancelAfterStart {
    fn cancelled(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst) > 0
    }
}

struct CancelWhenPresent(PathBuf);

impl crate::execution::ExecutionObserver for CancelWhenPresent {}

impl crate::execution::ExecutionCancellation for CancelWhenPresent {
    fn cancelled(&self) -> bool {
        self.0.exists()
    }
}

#[cfg(unix)]
#[test]
fn prompt_reader_rejects_a_fifo_without_waiting_for_input() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    let fifo = temp.path().join("tasks/prompt.fifo");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo_path` is a live NUL-terminated string and the mode
    // contains only ordinary permission bits.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
    // Keep both peers open so this test remains bounded even if the prompt
    // opener accidentally becomes blocking again.
    let _reader = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo)
        .unwrap();
    let _writer = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&fifo)
        .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = read_prompt(&ctx, Path::new("tasks/prompt.fifo"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("not a regular file"), "{error}");
}

#[test]
fn normal_untracked_mode_still_detects_an_untracked_directory() {
    let _env_lock = lock_env();
    let repo = tempdir().unwrap();
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
    fs::create_dir(repo.path().join("untracked-directory")).unwrap();
    fs::write(
        repo.path().join("untracked-directory/new-file"),
        "untracked\n",
    )
    .unwrap();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, OsStr::new("git"));
    let ctx = RepoContext::load_from(repo.path()).unwrap();

    assert!(git_is_dirty(&ctx, repo.path(), &mut NoopExecutionObserver).unwrap());
}

#[cfg(unix)]
#[test]
fn failed_worktree_add_cleans_its_partial_checkout() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = lock_env();
    let repo = tempdir().unwrap();
    TestRepoBuilder::new(repo.path())
        .required_commands(Vec::<String>::new())
        .write();
    let git = repo.path().join("git-failed-add");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
case " $* " in
  *" rev-parse HEAD "*) printf 'initial-head\n' ;;
  *" worktree add "*)
    previous=
    for argument in "$@"; do
      if [ "$previous" = "--detach" ]; then mkdir -p "$argument"; fi
      previous=$argument
    done
    exit 9
    ;;
  *" worktree remove "*)
    for argument in "$@"; do target=$argument; done
    rmdir "$target"
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
    let ctx = RepoContext::load_from(repo.path()).unwrap();

    let error = prepare_checkout(
        &ctx,
        &test_worktree_workflow(),
        "item-1",
        CodexTaskCheckout::Worktree,
        &mut NoopExecutionObserver,
    )
    .err()
    .expect("failed worktree add must fail preparation")
    .to_string();

    assert!(error.contains("status 9"), "{error}");
    let task_root = repo.path().join(LOOP_RUNTIME_DIR).join("worktrees/tasks");
    assert_eq!(fs::read_dir(task_root).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn failed_worktree_add_does_not_report_an_absent_leftover() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = lock_env();
    let repo = tempdir().unwrap();
    TestRepoBuilder::new(repo.path())
        .required_commands(Vec::<String>::new())
        .write();
    let git = repo.path().join("git-failed-before-add");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
case " $* " in
  *" rev-parse HEAD "*) printf 'initial-head\n' ;;
  *" worktree add "*) exit 9 ;;
  *" worktree remove "*) exit 4 ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
    let ctx = RepoContext::load_from(repo.path()).unwrap();

    let error = prepare_checkout(
        &ctx,
        &test_worktree_workflow(),
        "item-1",
        CodexTaskCheckout::Worktree,
        &mut NoopExecutionObserver,
    )
    .err()
    .expect("failed worktree add must fail preparation")
    .to_string();

    assert!(error.contains("status 9"), "{error}");
    assert!(!error.contains("partial Codex task worktree"), "{error}");
    assert!(!error.contains("cleanup failed"), "{error}");
}

#[cfg(unix)]
#[test]
fn cancelled_worktree_add_reports_cleanup_failure_and_blocks_reuse() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = lock_env();
    let repo = tempdir().unwrap();
    TestRepoBuilder::new(repo.path())
        .required_commands(Vec::<String>::new())
        .write();
    let git = repo.path().join("git-cancelled-add");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
case " $* " in
  *" rev-parse HEAD "*) printf 'initial-head\n' ;;
  *" worktree add "*)
    previous=
    for argument in "$@"; do
      if [ "$previous" = "--detach" ]; then mkdir -p "$argument"; fi
      previous=$argument
    done
    : > add-started
    sleep 60
    ;;
  *" worktree remove "*) exit 4 ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
    let ctx = RepoContext::load_from(repo.path()).unwrap();
    let workflow = test_worktree_workflow();
    let mut observer = CancelWhenPresent(repo.path().join("add-started"));

    let error = prepare_checkout(
        &ctx,
        &workflow,
        "item-1",
        CodexTaskCheckout::Worktree,
        &mut observer,
    )
    .err()
    .expect("cancelled worktree add must fail preparation")
    .to_string();

    assert!(error.contains("cancelled"), "{error}");
    assert!(
        error.contains("partial Codex task worktree may remain"),
        "{error}"
    );
    assert!(error.contains("cleanup failed"), "{error}");
    let retry = prepare_checkout(
        &ctx,
        &workflow,
        "item-1",
        CodexTaskCheckout::Worktree,
        &mut NoopExecutionObserver,
    )
    .err()
    .expect("leftover worktree must block reuse")
    .to_string();
    assert!(retry.contains("worktree already exists"), "{retry}");
}

fn test_worktree_workflow() -> ResolvedWorkflow {
    ResolvedWorkflow {
        id: "test-task".into(),
        kind: super::super::workflow::CODEX_TASK_KIND.into(),
        enabled: true,
        configured: true,
        lease_ttl_seconds: 60,
        max_attempts: 1,
        backoff_seconds: 1,
        codex_home_configured: None,
        schedule: None,
        codex_task: None,
    }
}

#[cfg(unix)]
#[test]
fn checkout_git_honors_cancellation_timeout_and_output_limits() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = lock_env();
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!("{config}\n[execution]\ncommand_timeout_seconds = 1\n"),
    )
    .unwrap();
    let git = temp.path().join("git-stub");
    fs::write(&git, "#!/bin/sh\nsleep 60\n").unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    let _git = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let cancelled = git_output(
        &ctx,
        temp.path(),
        ["status"],
        &mut CancelAfterStart(AtomicUsize::new(0)),
    )
    .expect_err("cancellation must fail checkout Git")
    .to_string();
    assert!(cancelled.contains("cancelled"), "{cancelled}");

    let timed_out = git_output(&ctx, temp.path(), ["status"], &mut NoopExecutionObserver)
        .expect_err("timeout must fail checkout Git")
        .to_string();
    assert!(
        timed_out.contains("timed out after 1 seconds"),
        "{timed_out}"
    );

    fs::write(
        &git,
        "#!/bin/sh\ndd if=/dev/zero bs=1048576 count=5 2>/dev/null\n",
    )
    .unwrap();
    let overflow = git_output(&ctx, temp.path(), ["status"], &mut NoopExecutionObserver)
        .expect_err("output overflow must fail checkout Git")
        .to_string();
    assert!(overflow.contains("capture limit"), "{overflow}");
    assert!(
        git_is_dirty(&ctx, temp.path(), &mut NoopExecutionObserver).unwrap(),
        "overflowing status output itself proves that the checkout is dirty"
    );
}
