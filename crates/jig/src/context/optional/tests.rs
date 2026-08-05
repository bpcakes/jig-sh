use super::*;
use crate::test_env::{CurrentDirGuard, EnvVarGuard, TestRepoBuilder, lock_env};
use tempfile::tempdir;

#[test]
fn quiet_optional_context_ignores_an_invalid_override_without_a_local_repo() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing");
    let _repo_root = EnvVarGuard::set(JIG_REPO_ROOT_ENV, &missing);
    let _cwd = CurrentDirGuard::set(temp.path());

    assert!(RepoContext::load_optional_strict().is_err());
    assert!(RepoContext::load_optional_quiet().unwrap().is_none());
}

#[test]
fn quiet_optional_context_falls_back_to_the_current_valid_repo() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let missing = temp.path().join("missing");
    let _repo_root = EnvVarGuard::set(JIG_REPO_ROOT_ENV, &missing);
    let _cwd = CurrentDirGuard::set(temp.path());

    assert!(RepoContext::load_optional_strict().is_err());
    let ctx = RepoContext::load_optional_quiet().unwrap().unwrap();
    assert_eq!(ctx.root(), temp.path().canonicalize().unwrap());
}
