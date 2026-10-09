use super::*;

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
fn schema_docs_dir_is_repository_relative_normalized_and_literal() {
    for (value, expected) in [
        ("../schema", "stay inside"),
        ("docs//schema", "normalized"),
        (".", "dedicated directory"),
        (".agent/schema", "outside reserved"),
        (".git/schema", "outside reserved"),
        (".Agent/schema", "outside reserved"),
        ("generated/.GIT/schema", "outside reserved"),
        (":(exclude)docs/schema", "unsupported characters"),
    ] {
        let temp = tempdir().unwrap();
        crate::test_support::TestRepoBuilder::new(temp.path())
            .config(format!("schema_docs_dir = {value:?}\n"))
            .write();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains(expected), "unexpected error: {error}");
    }
    let hfs_alias = validate_schema_docs_dir("generated/\u{200c}.git/schema")
        .unwrap_err()
        .to_string();
    assert!(hfs_alias.contains("outside reserved"), "{hfs_alias}");

    let temp = tempdir().unwrap();
    crate::test_support::TestRepoBuilder::new(temp.path())
        .config("schema_docs_dir = \"artifacts/schema\"\n")
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert_eq!(ctx.schema_docs_dir(), "artifacts/schema");
}
