use super::*;
use crate::test_support::{CurrentDirGuard, EnvVarGuard, TestRepoBuilder, lock_env};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn load_optional_ignores_stale_jig_repo_root() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing");
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing);
    let _cwd = CurrentDirGuard::set(temp.path());

    let result = RepoContext::load_optional();
    assert!(result.unwrap().is_none());
}

#[test]
fn load_optional_ignores_non_repo_jig_repo_root() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let non_repo = temp.path().join("non-repo");
    fs::create_dir_all(&non_repo).unwrap();
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &non_repo);
    let _cwd = CurrentDirGuard::set(temp.path());

    let result = RepoContext::load_optional();
    assert!(result.unwrap().is_none());
}

#[test]
fn load_optional_ignores_empty_jig_repo_root() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", "");
    let _cwd = CurrentDirGuard::set(temp.path());

    let result = RepoContext::load_optional();
    assert!(result.unwrap().is_none());
}

#[test]
fn repo_vault_config_is_loaded() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[vault]
scope = "repo"
scope_id = "scope_1"
allow_global = true
"#,
    )
    .unwrap();

    validate_config(&config).unwrap();
    assert_eq!(config.vault.repo_scope_id(), Some("scope_1"));
    assert!(config.vault.allow_global());
}

#[test]
fn repo_vault_config_requires_scope_id() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[vault]
scope = "repo"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("scope_id is required"));
}

mod agent_config;
mod apps;
mod backend;
mod commands;
mod contract;
mod loop_config;
mod runtime;
mod strict_config;
mod tracker_authority;
