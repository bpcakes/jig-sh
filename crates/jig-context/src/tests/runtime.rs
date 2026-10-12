use super::*;

#[test]
fn runtime_commands_still_require_adopted_repo_context() {
    let temp = tempdir().unwrap();
    let error = find_repo_root_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("Could not find repo root containing .jig.toml"));
}

#[test]
fn load_optional_returns_none_outside_adopted_repo() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());

    let result = RepoContext::load_optional();
    assert!(result.unwrap().is_none());
}

#[test]
fn runtime_cache_paths_share_the_generated_installer_layout() {
    let temp = tempdir().unwrap();
    assert_eq!(
        runtime_cache_base(temp.path()),
        temp.path().join(FALLBACK_RUNTIME_CACHE_BASE)
    );
    assert_eq!(
        runtime_profile_cache_name(4, RuntimeCacheProfile::Default),
        "contract-4"
    );
    assert_eq!(
        runtime_profile_cache_name(4, RuntimeCacheProfile::Runtime),
        "contract-4-runtime"
    );
    std::fs::create_dir(temp.path().join(".git")).unwrap();
    assert_eq!(
        runtime_profile_cache_path(temp.path(), 4, RuntimeCacheProfile::Runtime),
        temp.path().join(".git/jig-tools/contract-4-runtime")
    );
}
