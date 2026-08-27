use super::*;
use crate::test_env::{CurrentDirGuard, EnvVarGuard, TestRepoBuilder, lock_env};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn contract_digest_uses_canonical_execution_authority() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let config_path = temp.path().join(".jig.toml");
    let original_source = fs::read_to_string(&config_path).unwrap();
    let snapshot = load_config_snapshot(&config_path).unwrap();
    let manifest_text = fs::read_to_string(temp.path().join(".agent/jig-contract.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();

    let expected = contract_source_digest(&snapshot.config, &manifest).unwrap();
    fs::write(
        &config_path,
        format!(
            "{}\n[dev]\nproxy_port = 2456\n# local runtime settings and comments are not execution authority\n",
            original_source.trim_end()
        ),
    )
    .unwrap();
    let comment_only = load_config_snapshot(&config_path).unwrap();
    assert_eq!(
        contract_source_digest(&comment_only.config, &manifest).unwrap(),
        expected
    );

    let changed_source = format!(
        "{}\n[commands]\nrust_test_command = \"cargo nextest run\"\n",
        original_source.trim_end()
    );
    fs::write(&config_path, changed_source).unwrap();
    let changed = load_config_snapshot(&config_path).unwrap();
    assert_ne!(
        contract_source_digest(&changed.config, &manifest).unwrap(),
        expected
    );

    for authority_change in [
        "harness_footprint = \"minimal\"\n",
        "[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\n",
        "[work]\nchecks = [\"jig.contract_check\"]\n",
    ] {
        fs::write(
            &config_path,
            format!("{}\n{authority_change}", original_source.trim_end()),
        )
        .unwrap();
        let changed = load_config_snapshot(&config_path).unwrap();
        assert_ne!(
            contract_source_digest(&changed.config, &manifest).unwrap(),
            expected,
            "native contract-check input must participate in execution authority: {authority_change}"
        );
    }
}

#[test]
fn contract_digest_preserves_forward_compatible_manifest_fields() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .tool(json!({
            "name": "jig.future",
            "kind": "native",
            "description": "Future-compatible fixture.",
            "future_policy": {"mode": "original"},
        }))
        .write();
    let path = temp.path().join(".agent/jig-contract.json");
    let original = RepoContext::load_from(temp.path()).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["unmodeled_authority"] = json!("must not be dropped from the digest");
    manifest["tools"][0]["future_policy"]["mode"] = json!("changed");
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

    let changed = RepoContext::load_from(temp.path()).unwrap();

    assert_ne!(changed.contract_digest(), original.contract_digest());
}

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
fn supported_command_keys_are_backed_by_repo_config() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
contract_check_command = "scripts/jig check contract"
migration_add_command = "scripts/jig migration-add \"$NAME\""
rust_clippy_command = "cargo clippy"
rust_fmt_check_command = "cargo fmt --check"
rust_test_command = "cargo test"
rust_test_locked_command = "cargo test --locked"
schema_check_command = "scripts/jig check schema"
schema_dump_command = "scripts/dump-schema.sh"
sqlx_check_command = "cargo sqlx prepare --check"

[commands]
go_fmt_check_command = "gofmt -w ."
go_lint_command = "go vet ./..."
go_test_command = "go test ./..."
go_test_locked_command = "go test -mod=readonly ./..."
sqlc_check_command = "go tool sqlc diff"
typescript_build_command = "scripts/check-webapps.sh build"
typescript_coverage_command = "scripts/check-webapps.sh coverage"
typescript_lint_command = "scripts/check-webapps.sh lint"
typescript_typecheck_command = "scripts/check-webapps.sh typecheck"
"#,
    )
    .unwrap();
    let supported_command_keys = jig_features::supported_command_keys();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": supported_command_keys,
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for key in jig_features::supported_command_keys() {
        assert!(ctx.command_for_key(key).is_ok(), "{key}");
    }
}

#[test]
fn migration_directory_accepts_neutral_config_and_falls_back_to_legacy_rust_config() {
    let neutral = tempdir().unwrap();
    TestRepoBuilder::new(neutral.path())
        .config(
            r#"
migration_dir = "internal/database/migrations"
"#,
        )
        .write();
    let neutral_ctx = RepoContext::load_from(neutral.path()).unwrap();
    assert_eq!(neutral_ctx.migration_dir(), "internal/database/migrations");

    let legacy = tempdir().unwrap();
    TestRepoBuilder::new(legacy.path())
        .config(r#"rust_migration_dir = "migrations""#)
        .write();
    let legacy_ctx = RepoContext::load_from(legacy.path()).unwrap();
    assert_eq!(legacy_ctx.migration_dir(), "migrations");
}

#[test]
fn sqlx_migration_directory_rejects_divergent_compatibility_keys() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
migration_dir = "database/migrations"
rust_migration_dir = "legacy-migrations"
"#,
        )
        .write();

    let error = RepoContext::load_from(temp.path()).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must identify the same SQLx migration directory")
    );
}

#[test]
fn migration_directories_must_be_portable_repository_relative_paths() {
    for (key, value) in [
        ("migration_dir", "../outside"),
        ("migration_dir", "/tmp/outside"),
        ("migration_dir", "."),
        ("rust_migration_dir", "C:/outside"),
        ("rust_migration_dir", "nested\\outside"),
    ] {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(format!("{key} = {value:?}"))
            .write();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

        assert!(error.contains(key), "{key}={value:?}: {error}");
        assert!(
            error.contains("repository-relative")
                || error.contains("stay inside")
                || error.contains("below the repository root"),
            "{key}={value:?}: {error}"
        );
    }
}

#[test]
fn backend_selectors_reject_unknown_config_values() {
    for (selector, expected) in [
        ("backend_language = \"ruby\"", "unknown variant `ruby`"),
        ("go_database = \"sqlite\"", "unknown variant `sqlite`"),
    ] {
        let config = format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
{selector}
"#
        );
        let error = toml::from_str::<RepoConfig>(&config)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "unexpected error: {error}");
    }
}

#[test]
fn postgres_go_database_requires_go_backend() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
go_database = "postgres"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert_eq!(
        error,
        "go_database = \"postgres\" requires backend_language = \"go\" in .jig.toml"
    );
}

#[test]
fn go_backend_rejects_rust_sqlx_capability() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
backend_language = "go"
sqlx_enabled = true
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert_eq!(
        error,
        "backend_language = \"go\" cannot be combined with sqlx_enabled = true in .jig.toml; Go repositories use go_database and Goose/sqlc, while SQLx is owned by the Rust backend"
    );
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

#[test]
fn loop_config_accepts_compiled_in_workflow_kinds() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[loop]
lease_ttl_seconds = 60
max_attempts = 2

[[loop.workflows]]
id = "status-check"
kind = "noop_status"

[[loop.workflows]]
id = "pr-status"
kind = "github_pr_status"

[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
codex_home = "work"
"#,
    )
    .unwrap();

    validate_config(&config).unwrap();
}

#[test]
fn loop_config_rejects_codex_home_for_non_codex_workflow() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "pr-status"
kind = "github_pr_status"
codex_home = "work"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("can set codex_home only when kind = 'pr_manager'"));
}

#[test]
fn loop_config_rejects_an_empty_codex_home() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "pr-manager"
kind = "pr_manager"
codex_home = ""
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("codex_home must not be empty"));
}

#[test]
fn loop_config_rejects_unknown_workflow_kinds() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "pr-manager"
kind = "github_pr_loop"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("Unsupported loop workflow kind 'github_pr_loop'"));
}

#[test]
fn loop_config_rejects_zero_backoff() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[loop]
backoff_seconds = 0
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("[loop].backoff_seconds must be greater than zero"));
}

#[test]
fn loop_config_rejects_colon_in_workflow_ids() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "status:check"
kind = "noop_status"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("Unsupported loop workflow id value 'status:check'"));
}

#[test]
fn dynamic_command_map_extends_contract_command_keys() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[commands]
web_audit_command = "npm run audit"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["web_audit_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert!(ctx.supports_command_key("web_audit_command"));
    assert_eq!(
        ctx.command_for_key("web_audit_command").unwrap(),
        "npm run audit"
    );
}

#[test]
fn invalid_command_map_keys_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[commands]
web_audit = "npm run audit"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Invalid [commands] key 'web_audit'"));
    assert!(error.contains("end command keys with '_command'"));
}

#[test]
fn command_map_keys_require_lowercase_letter_prefix() {
    assert!(is_safe_command_key("typescript_lint_command"));
    assert!(!is_safe_command_key("_command"));
    assert!(!is_safe_command_key("_typescript_lint_command"));
    assert!(!is_safe_command_key("1typescript_lint_command"));
    assert!(!is_safe_command_key("TypeScript_lint_command"));
}

#[test]
fn command_map_can_supply_legacy_command_keys() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[commands]
rust_test_command = "cargo nextest run"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["rust_test_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();

    assert_eq!(
        ctx.command_for_key("rust_test_command").unwrap(),
        "cargo nextest run"
    );
}

include!("tests_parts/part_02.rs");

mod runtime;
mod strict_config;

include!("tests_parts/part_01.rs");
