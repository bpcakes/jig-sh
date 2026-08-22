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

[[loop.workflows]]
id = "nightly-maintenance"
kind = "codex_task"
schedule = "0 2 * * *"
timezone = "Europe/Prague"
prompt_file = ".agent/tasks/nightly-maintenance.md"
codex_home = "work"
model = "gpt-5.6-terra"
sandbox = "workspace-write"
checkout = "worktree"
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
    assert!(error.contains("can set codex_home only when kind is 'pr_manager' or 'codex_task'"));
}

#[test]
fn loop_config_rejects_invalid_schedule_and_timezone() {
    let invalid_schedule: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 0 2 * * *"
timezone = "UTC"
prompt_file = ".agent/tasks/nightly.md"
"#,
    )
    .unwrap();
    let error = validate_config(&invalid_schedule).unwrap_err().to_string();
    assert!(error.contains("invalid five-field cron schedule"));

    let invalid_timezone: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
timezone = "Prague"
prompt_file = ".agent/tasks/nightly.md"
"#,
    )
    .unwrap();
    let error = validate_config(&invalid_timezone).unwrap_err().to_string();
    assert!(error.contains("invalid IANA timezone 'Prague'"));
}

#[test]
fn loop_config_rejects_schedule_without_a_calendar_occurrence() {
    let impossible: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "impossible"
kind = "codex_task"
schedule = "0 0 31 6 *"
timezone = "UTC"
prompt_file = ".agent/tasks/impossible.md"
"#,
    )
    .unwrap();

    let error = validate_config(&impossible).unwrap_err().to_string();
    assert!(error.contains("has no possible calendar occurrence"));

    let leap_day: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "leap-day"
kind = "codex_task"
schedule = "0 0 29 2 *"
timezone = "UTC"
prompt_file = ".agent/tasks/leap-day.md"
"#,
    )
    .unwrap();

    validate_config(&leap_day).unwrap();
}

#[test]
fn loop_config_requires_safe_codex_task_fields() {
    let missing_prompt: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
"#,
    )
    .unwrap();
    let error = validate_config(&missing_prompt).unwrap_err().to_string();
    assert!(error.contains("requires prompt_file"));

    let escaping_prompt: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
prompt_file = "../outside.md"
"#,
    )
    .unwrap();
    let error = validate_config(&escaping_prompt).unwrap_err().to_string();
    assert!(error.contains("repository-relative path without '..'"));

    let full_access: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "nightly"
kind = "codex_task"
schedule = "0 2 * * *"
prompt_file = ".agent/tasks/nightly.md"
sandbox = "danger-full-access"
"#,
    )
    .unwrap();
    let error = validate_config(&full_access).unwrap_err().to_string();
    assert!(error.contains("sandbox must be 'read-only' or 'workspace-write'"));
}

#[test]
fn loop_config_rejects_task_fields_on_other_workflows() {
    let config: RepoConfig = toml::from_str(
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[[loop.workflows]]
id = "status"
kind = "noop_status"
prompt_file = ".agent/tasks/status.md"
"#,
    )
    .unwrap();

    let error = validate_config(&config).unwrap_err().to_string();
    assert!(error.contains("can set prompt_file only when kind = 'codex_task'"));
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
