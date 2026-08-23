use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::context::RepoContext;
use crate::test_env::TestRepoBuilder;
use crate::tool_defs::{kind, tool};

fn write_policy_repo(root: &Path) {
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    TestRepoBuilder::new(root)
        .config(
            r#"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
}

fn write_footprint_contract_repo(root: &Path, footprint: &str) {
    TestRepoBuilder::new(root)
        .config(format!(
            r#"
harness_footprint = "{footprint}"
bootstrap_command = "true"
"#
        ))
        .required_commands(["bootstrap_command"])
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": kind::COMMAND,
            "description": "Bootstrap.",
            "command": "bootstrap_command"
        }))
        .tool(json!({
            "name": tool::CONTRACT_CHECK,
            "kind": kind::NATIVE,
            "description": "Contract check."
        }))
        .write();
}

fn write_sqlx_policy_repo(root: &Path) {
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    TestRepoBuilder::new(root)
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "migrations"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
}

fn write_go_postgres_policy_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .config(
            r#"
backend_language = "go"
go_database = "postgres"
migration_dir = "internal/database/migrations"
"#,
        )
        .contract_version(2)
        .write();
}

fn write_schema_policy_repo(root: &Path, schema_dump_command: &str) {
    write_schema_policy_repo_with_timeout(root, schema_dump_command, None);
}

fn write_schema_policy_repo_with_timeout(
    root: &Path,
    schema_dump_command: &str,
    command_timeout_seconds: Option<u64>,
) {
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    let execution_config = command_timeout_seconds.map_or_else(String::new, |seconds| {
        format!(
            r#"
[execution]
command_timeout_seconds = {seconds}
"#
        )
    });
    TestRepoBuilder::new(root)
        .config(format!(
            r#"
sqlx_enabled = true
schema_dump_enabled = true
rust_migration_dir = "migrations"
schema_dump_command = "{}"
rust_test_command = "cargo test"
{}
"#,
            schema_dump_command
                .replace('\\', "\\\\")
                .replace('"', "\\\""),
            execution_config,
        ))
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
}

fn write_policy_config(root: &Path, config: &str) {
    TestRepoBuilder::new(root).config(config).write_config();
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {}", args.join(" "));
}

fn init_git(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.com"]);
}

#[test]
fn controlled_output_text_rejects_missing_and_incomplete_captures() {
    assert!(
        controlled_output_text(None, "stdout")
            .unwrap_err()
            .to_string()
            .contains("not captured")
    );
    let incomplete = BoundedProcessOutput {
        bytes: b"partial".to_vec(),
        truncated: false,
        complete: false,
    };

    assert!(
        controlled_output_text(Some(incomplete), "stderr")
            .unwrap_err()
            .to_string()
            .contains("did not complete")
    );
}

#[test]
fn contract_check_allows_minimal_footprint_to_omit_launcher_files() {
    let temp = tempdir().unwrap();
    write_footprint_contract_repo(temp.path(), "minimal");

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[test]
fn contract_validation_rejects_a_whitespace_only_migration_directory() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
harness_footprint = "minimal"
sqlx_enabled = true
rust_migration_dir = "   "
"#,
        )
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = validate_contract(&ctx).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("migration_dir is empty and no legacy rust_migration_dir fallback")
    );
}

#[test]
fn contract_check_still_validates_minimal_commands_and_tools() {
    let temp = tempdir().unwrap();
    write_footprint_contract_repo(temp.path(), "minimal");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("bootstrap_command = \"true\"", "bootstrap_command = \"  \"");
    fs::write(&config_path, config).unwrap();
    let contract_path = temp.path().join(".agent/jig-contract.json");
    let mut contract =
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&contract_path).unwrap())
            .unwrap();
    contract["tools"].as_array_mut().unwrap().push(json!({
        "name": "jig.unsupported",
        "kind": kind::NATIVE,
        "description": "Unsupported."
    }));
    fs::write(
        &contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(
        output
            .stderr
            .contains("Command key bootstrap_command is empty"),
        "{}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("Unsupported native tool: jig.unsupported"),
        "{}",
        output.stderr
    );
}

#[test]
fn contract_check_still_requires_launcher_files_for_full_footprint() {
    let temp = tempdir().unwrap();
    write_footprint_contract_repo(temp.path(), "full");

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(
        output.stderr.contains("Missing .mcp.json."),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Missing scripts/jig launcher."),
        "{}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("Missing scripts/install-jig.sh installer."),
        "{}",
        output.stderr
    );
}

#[test]
fn contract_check_rejects_make_tool_kind() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"
"#,
    );
    TestRepoBuilder::new(temp.path())
        .required_commands(["bootstrap_command"])
        .tool(json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Run contract check." }))
        .tool(json!({ "name": "jig.legacy_make", "kind": "make", "description": "Unsupported legacy make tool." }))
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(
        output
            .stderr
            .contains("Unsupported tool kind for jig.legacy_make: make"),
        "{}",
        output.stderr
    );
}

#[test]
fn contract_check_accepts_dynamic_command_map_tools() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"
rust_fmt_check_command = "true"
rust_clippy_command = "true"
rust_test_command = "true"

[commands]
typescript_lint_command = "scripts/check-webapps.sh lint"
"#,
    );
    TestRepoBuilder::new(temp.path())
        .required_commands(["bootstrap_command", "rust_fmt_check_command", "rust_clippy_command", "rust_test_command", "typescript_lint_command"])
        .tools([
            json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }),
            json!({ "name": tool::FMT_CHECK, "kind": kind::COMMAND, "description": "Format.", "command": "rust_fmt_check_command" }),
            json!({ "name": tool::CLIPPY, "kind": kind::COMMAND, "description": "Clippy.", "command": "rust_clippy_command" }),
            json!({ "name": tool::TEST, "kind": kind::COMMAND, "description": "Test.", "command": "rust_test_command" }),
            json!({ "name": tool::TYPESCRIPT_LINT, "kind": kind::COMMAND, "description": "Run TypeScript lint.", "command": "typescript_lint_command" }),
            json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Contract check." }),
        ])
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[test]
fn contract_check_does_not_require_undeclared_rust_gate_tools() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"

[commands]
typescript_lint_command = "npm run lint"
"#,
    );
    TestRepoBuilder::new(temp.path())
        .required_commands(["bootstrap_command", "typescript_lint_command"])
        .tool(json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": tool::TYPESCRIPT_LINT, "kind": kind::COMMAND, "description": "Run TypeScript lint.", "command": "typescript_lint_command" }))
        .tool(json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Contract check." }))
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[test]
fn contract_check_requires_declared_rust_gate_tools() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"
rust_fmt_check_command = "true"
rust_clippy_command = "true"
rust_test_command = "true"
"#,
    );
    TestRepoBuilder::new(temp.path())
        .required_commands(["bootstrap_command", "rust_fmt_check_command", "rust_clippy_command", "rust_test_command"])
        .tools([
            json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }),
            json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Contract check." }),
        ])
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains(tool::FMT_CHECK), "{}", output.stderr);
    assert!(output.stderr.contains(tool::CLIPPY), "{}", output.stderr);
    assert!(output.stderr.contains(tool::TEST), "{}", output.stderr);
}

#[test]
fn contract_check_requires_generated_typescript_gate_tools() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
"#,
    );
    TestRepoBuilder::new(temp.path())
        .required_commands(["bootstrap_command"])
        .tool(json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Contract check." }))
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(
        output.stderr.contains(tool::TYPESCRIPT_LINT),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains(tool::TYPESCRIPT_TYPECHECK),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains(tool::TYPESCRIPT_BUILD),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains(tool::TYPESCRIPT_COVERAGE),
        "{}",
        output.stderr
    );
}

#[test]
fn contract_check_does_not_require_generated_typescript_gates_for_legacy_contracts() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
"#,
    );
    TestRepoBuilder::new(temp.path())
        .contract_version(2)
        .required_commands(["bootstrap_command"])
        .tool(json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Contract check." }))
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[test]
fn contract_check_reports_missing_feature_declared_command_map_entry() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    write_policy_config(
        temp.path(),
        r#"bootstrap_command = "true"
rust_fmt_check_command = "true"
rust_clippy_command = "true"
rust_test_command = "true"
"#,
    );
    TestRepoBuilder::new(temp.path())
        .required_commands(["bootstrap_command", "rust_fmt_check_command", "rust_clippy_command", "rust_test_command", "typescript_lint_command"])
        .tools([
            json!({ "name": tool::BOOTSTRAP, "kind": kind::COMMAND, "description": "Bootstrap.", "command": "bootstrap_command" }),
            json!({ "name": tool::FMT_CHECK, "kind": kind::COMMAND, "description": "Format.", "command": "rust_fmt_check_command" }),
            json!({ "name": tool::CLIPPY, "kind": kind::COMMAND, "description": "Clippy.", "command": "rust_clippy_command" }),
            json!({ "name": tool::TEST, "kind": kind::COMMAND, "description": "Test.", "command": "rust_test_command" }),
            json!({ "name": tool::TYPESCRIPT_LINT, "kind": kind::COMMAND, "description": "Run TypeScript lint.", "command": "typescript_lint_command" }),
            json!({ "name": tool::CONTRACT_CHECK, "kind": kind::NATIVE, "description": "Contract check." }),
        ])
        .write_contract();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx);

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("missing in [commands]"));
}

#[test]
fn migration_immutability_parses_nul_name_status_entries() {
    let bytes = b"A\0migrations/002_added.up.sql\0M\0migrations/001_changed.up.sql\0R100\0migrations/001_old.up.sql\0migrations/001_new.up.sql\0D\0migrations/001_deleted.down.sql\0T\0migrations/001_type.sql\0";

    let violations = migration_immutability_violations(bytes);

    assert_eq!(violations.len(), 4);
    assert!(violations.iter().all(|violation| {
        !violation.contains("002_added") && violation.contains("Existing migration files")
    }));
    assert!(violations.iter().any(|violation| {
        violation.contains("Rename detected (R100)")
            && violation.contains("migrations/001_old.up.sql -> migrations/001_new.up.sql")
    }));
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("Change detected (M)")
                && violation.contains("001_changed"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("Change detected (D)")
                && violation.contains("001_deleted"))
    );
    assert!(violations.iter().any(
        |violation| violation.contains("Change detected (T)") && violation.contains("001_type")
    ));
}

#[test]
fn migration_immutability_ignores_truncated_rename_entry() {
    let violations = migration_immutability_violations(b"R100\0migrations/old.sql\0");

    assert!(violations.is_empty());
}

#[test]
fn migration_immutability_prefers_the_backend_neutral_directory() {
    let temp = tempdir().unwrap();
    let migration_dir = temp.path().join("internal/database/migrations");
    let legacy_dir = temp.path().join("legacy-migrations");
    fs::create_dir_all(&migration_dir).unwrap();
    fs::create_dir_all(&legacy_dir).unwrap();
    fs::write(migration_dir.join("00001_app.sql"), "-- initial\n").unwrap();
    fs::write(legacy_dir.join("00001_legacy.sql"), "-- initial\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
backend_language = "go"
go_database = "postgres"
migration_dir = "internal/database/migrations"
rust_migration_dir = "legacy-migrations"
"#,
        )
        .write();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    fs::write(migration_dir.join("00001_app.sql"), "-- changed\n").unwrap();
    fs::write(legacy_dir.join("00001_legacy.sql"), "-- changed\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "change migrations"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = check_migration_immutability(
        &ctx,
        &MigrationImmutabilityInput {
            changed_against: "HEAD^".into(),
        },
    )
    .unwrap();
    let violations = output["violations"].as_array().unwrap();

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0]
            .as_str()
            .unwrap()
            .contains("internal/database/migrations/00001_app.sql")
    );
}

#[test]
fn migration_add_creates_slugged_migration_files() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = migration_add(&ctx, "Create Users!").unwrap();

    assert_eq!(output.exit_status, 0);
    let entries = fs::read_dir(temp.path().join("migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .any(|entry| entry.ends_with("_create_users.up.sql"))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.ends_with("_create_users.down.sql"))
    );
}

#[test]
fn migration_add_uses_a_neutral_only_sqlx_directory() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
migration_dir = "database/migrations"
"#,
        )
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    migration_add(&ctx, "Create Users").unwrap();

    assert_eq!(
        fs::read_dir(temp.path().join("database/migrations"))
            .unwrap()
            .count(),
        2
    );
}

#[cfg(unix)]
#[test]
fn migration_add_rejects_a_symlinked_migration_directory() {
    use std::os::unix::fs::symlink;

    let repository = tempdir().unwrap();
    let outside = tempdir().unwrap();
    TestRepoBuilder::new(repository.path())
        .config(
            r#"
sqlx_enabled = true
migration_dir = "database/migrations"
"#,
        )
        .write();
    fs::create_dir(repository.path().join("database")).unwrap();
    symlink(
        outside.path(),
        repository.path().join("database/migrations"),
    )
    .unwrap();
    let ctx = RepoContext::load_from(repository.path()).unwrap();

    let error = migration_add(&ctx, "Create Users").unwrap_err().to_string();

    assert!(error.contains("is a symlink"), "{error}");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn migration_add_creates_a_goose_migration_for_go_postgres() {
    let temp = tempdir().unwrap();
    write_go_postgres_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = migration_add(&ctx, "Create Users!").unwrap();

    assert_eq!(output.exit_status, 0);
    let entries = fs::read_dir(temp.path().join("internal/database/migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("_create_users.sql")
    );
    assert_eq!(
        fs::read_to_string(&entries[0]).unwrap(),
        "-- +goose Up\n-- forward migration: create_users\n\n-- +goose Down\n-- rollback migration: create_users\n"
    );
}

#[test]
fn migration_add_rejects_when_sqlx_is_disabled() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "create users").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured SQLx or Go/PostgreSQL migration backend")
    );
}

#[test]
fn migration_add_rejects_names_without_slug_content() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "!!!").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must contain at least one alphanumeric")
    );
}

#[test]
fn schema_check_reports_stale_schema_dump() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && printf 'changed\\n' > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(output.stderr.contains("docs/schema"));
    assert_eq!(
        fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
        "stable\n",
        "a read-only schema check must restore generator output"
    );
    assert!(
        Command::new("git")
            .current_dir(temp.path())
            .args(["status", "--porcelain", "--", "docs/schema"])
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn schema_check_preserves_preexisting_schema_edits_without_running_the_generator() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "printf 'generator-ran\\n' > generator-marker && printf 'changed\\n' > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::write(temp.path().join("docs/schema/tables.sql"), "local edit\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("already has uncommitted changes"));
    assert_eq!(
        fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
        "local edit\n"
    );
    assert!(!temp.path().join("generator-marker").exists());
}

#[test]
fn schema_check_discards_new_files_staged_by_the_generator() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && printf 'new\\n' > docs/schema/new.sql && git add docs/schema/new.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(!temp.path().join("docs/schema/new.sql").exists());
    assert!(
        Command::new("git")
            .current_dir(temp.path())
            .args(["status", "--porcelain", "--", "docs/schema"])
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn schema_check_isolates_unrelated_generator_writes_and_reads_untracked_inputs() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "printf 'mutated\\n' > unrelated.txt && cat schema-input > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::write(temp.path().join("schema-input"), "stable\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert!(!temp.path().join("unrelated.txt").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("schema-input")).unwrap(),
        "stable\n"
    );
}

#[test]
fn schema_check_reads_ignored_dotenv_inputs_in_the_snapshot() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "cat .env > docs/schema/tables.sql");
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    fs::write(temp.path().join(".gitignore"), ".env\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::write(temp.path().join(".env"), "stable\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert_eq!(
        fs::read_to_string(temp.path().join(".env")).unwrap(),
        "stable\n"
    );
}

#[test]
fn schema_check_reads_initialized_submodule_worktrees_in_the_snapshot() {
    let dependency = tempdir().unwrap();
    fs::write(dependency.path().join("schema-input"), "stable\n").unwrap();
    init_git(dependency.path());
    git(dependency.path(), &["add", "."]);
    git(dependency.path(), &["commit", "-m", "baseline", "-q"]);

    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "cat vendor/example/schema-input > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(
        temp.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            dependency.path().to_str().unwrap(),
            "vendor/example",
        ],
    );
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[cfg(unix)]
#[test]
fn schema_check_preserves_untracked_symlinks_without_following_them() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "printf 'stable\\n' > docs/schema/tables.sql");
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    std::os::unix::fs::symlink("missing-target", temp.path().join("unrelated-link")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert!(
        fs::symlink_metadata(temp.path().join("unrelated-link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn controlled_native_output_is_bounded() {
    let mut command = Command::new("bash");
    command.args(["-c", "yes x | head -c 2000000"]);

    let output = controlled_output(
        &mut command,
        Instant::now() + Duration::from_secs(10),
        &|| false,
    )
    .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.contains("[output truncated by Jig]"));
    assert!(output.stdout.len() < 2_000_000);
}

#[test]
fn schema_check_supervises_timeout_and_descendant_cleanup() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("schema-descendant-survived");
    write_schema_policy_repo_with_timeout(
        temp.path(),
        &format!("(sleep 2; printf survived > '{}') & wait", marker.display()),
        Some(1),
    );
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = schema_check(&ctx).unwrap_err().to_string();

    assert!(error.contains("timed out after 1 seconds"), "{error}");
    std::thread::sleep(std::time::Duration::from_millis(1_250));
    assert!(
        !marker.exists(),
        "schema timeout left a configured-command descendant running"
    );
}

#[test]
fn schema_check_preserves_pre_start_cancellation() {
    struct Cancelled;

    impl crate::execution::ExecutionObserver for Cancelled {}

    impl crate::execution::ExecutionCancellation for Cancelled {
        fn cancelled(&self) -> bool {
            true
        }
    }

    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "exit 99");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = schema_check_with_observer(&ctx, &mut Cancelled).unwrap_err();

    assert!(matches!(error, ExecutionCommandError::CancelledBeforeStart));
}

#[test]
fn check_rust_file_loc_reports_oversized_tracked_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(
        temp.path().join("crates/app/src/large.rs"),
        "fn example() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &RustFileLocInput {
            staged: false,
            changed_against: None,
            all: true,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/large.rs"))
    );
}

#[test]
fn check_rust_file_loc_reports_oversized_staged_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(
        temp.path().join("crates/app/src/staged.rs"),
        "fn staged() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &RustFileLocInput {
            staged: true,
            changed_against: None,
            all: false,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/staged.rs"))
    );
}

#[test]
fn check_rust_file_loc_reports_oversized_changed_against_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(temp.path().join("crates/app/src/lib.rs"), "fn small() {}\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let base = super::git_text(temp.path(), &["rev-parse", "HEAD"]).unwrap();
    fs::write(
        temp.path().join("crates/app/src/large.rs"),
        "fn changed() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "large", "-q"]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &RustFileLocInput {
            staged: false,
            changed_against: Some(base.trim().to_string()),
            all: false,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/large.rs"))
    );
}
