use super::*;

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
