use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn status_provider_answers_survive_effective_options_and_render_serialization() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
sqlx_enabled = false
schema_dump_enabled = false

[[status.providers]]
id = "factorish.example"
argv = ["ruby", "scripts/status.rb", "--jig-v1"]
timeout_seconds = 45
"#,
    )
    .unwrap();

    let input = AnswerInput::from_file(&path).unwrap();
    let effective = input.effective_opts(&AnswerOpts::default()).unwrap();
    let provider = &effective.status.as_ref().unwrap().providers[0];
    assert_eq!(provider.id, "factorish.example");
    assert_eq!(provider.timeout_seconds, 45);

    let rendered = RenderAnswers::from_answers_file(&path).unwrap();
    let value = serde_json::to_value(rendered).unwrap();
    assert_eq!(value["jig_version"], "0.2.0-beta.1");
    assert_eq!(
        value["status"]["providers"][0]["argv"],
        serde_json::json!(["ruby", "scripts/status.rb", "--jig-v1"])
    );
    assert_eq!(value["status"]["providers"][0]["timeout_seconds"], 45);
}

#[test]
fn migration_dir_survives_effective_options_and_render_serialization() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"
backend_language = "go"
go_database = "postgres"
migration_dir = "database/migrations"
sqlx_enabled = false
schema_dump_enabled = false
"#,
    )
    .unwrap();

    let input = AnswerInput::from_file(&path).unwrap();
    let effective = input.effective_opts(&AnswerOpts::default()).unwrap();
    assert_eq!(
        effective.migration_dir.as_deref(),
        Some("database/migrations")
    );

    let rendered = RenderAnswers::from_answers_file(&path).unwrap();
    let value = serde_json::to_value(rendered).unwrap();
    assert_eq!(value["migration_dir"], "database/migrations");
}

#[test]
fn generated_go_format_check_propagates_parser_failures_and_ignores_ignored_files() {
    let rendered = RawAnswers {
        repo_name: Some("demo".into()),
        backend_language: Some(BackendLanguage::Go),
        go_database: Some(GoDatabase::None),
        sqlx_enabled: Some(false),
        schema_dump_enabled: Some(false),
        ..RawAnswers::default()
    }
    .resolve(None)
    .unwrap();

    assert!(rendered.go_fmt_check_command.contains("set -o pipefail"));
    assert!(rendered.go_fmt_check_command.contains("git ls-files"));
    assert!(rendered.go_fmt_check_command.contains("--exclude-standard"));
    assert!(rendered.go_fmt_check_command.contains("xargs -0 gofmt -l"));
    assert!(rendered.go_fmt_check_command.contains(") || exit $?"));
}

#[test]
fn recopy_normalizes_exact_former_generated_sqlx_default() {
    let metadata_dir = "db/sqlx metadata";
    let former_default = format!(
        "SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} cargo sqlx prepare --check --workspace -- --workspace --all-targets",
        shell_quote(metadata_dir)
    );
    let mut raw = RawAnswers {
        repo_name: Some("demo".into()),
        sqlx_enabled: Some(true),
        rust_migration_dir: Some("migrations".into()),
        rust_sqlx_metadata_dir: Some(metadata_dir.into()),
        schema_dump_enabled: Some(false),
        sqlx_check_command: Some(former_default.clone()),
        ..RawAnswers::default()
    };

    raw.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(raw.sqlx_check_command, None);
    let rendered = raw.resolve(None).unwrap();
    assert_eq!(
        rendered.sqlx_check_command,
        format!(
            "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} sqlx prepare --check --workspace -- --workspace --all-targets",
            shell_quote(metadata_dir)
        )
    );

    let mut customized = RawAnswers {
        rust_sqlx_metadata_dir: Some(metadata_dir.into()),
        sqlx_check_command: Some(format!("{former_default} --custom")),
        ..RawAnswers::default()
    };
    customized.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(
        customized.sqlx_check_command.as_deref(),
        Some(format!("{former_default} --custom").as_str())
    );
}
