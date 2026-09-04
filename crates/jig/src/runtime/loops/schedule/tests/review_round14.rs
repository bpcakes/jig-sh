use std::fs;

use tempfile::tempdir;

use super::super::super::engine::tick_with_observer;
use super::super::super::state::LOOP_RUNTIME_DIR;
use super::super::NoopExecutionObserver;
use crate::command::LoopTickRequest;
use crate::context::RepoContext;
use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};

#[test]
fn non_codex_tick_refuses_to_create_an_unignored_loop_runtime() {
    let _env_lock = lock_env();
    let _git = EnvVarGuard::set(crate::bootstrap::GIT_BIN_ENV, std::ffi::OsStr::new("git"));
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let init = std::process::Command::new("git")
        .current_dir(temp.path())
        .arg("init")
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    fs::write(temp.path().join(".gitignore"), "").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = tick_with_observer(
        &ctx,
        LoopTickRequest {
            workflow: Some("noop-status".into()),
            lease_ttl_seconds: None,
            max_attempts: None,
            backoff_seconds: None,
        },
        &mut NoopExecutionObserver,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Loop runtime root is not ignored")
    );
    assert!(!temp.path().join(LOOP_RUNTIME_DIR).exists());
}
