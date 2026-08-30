use super::*;

#[test]
fn rust_cli_rejects_every_incompatible_cli_family() {
    let cases: &[(&[&str], &str)] = &[
        (&["--db", "none"], "--db"),
        (&["--frontend", "web"], "--frontend"),
        (&["--frontends", "web"], "--frontends"),
        (&["--frontend-app", "web:web:80"], "frontend_apps"),
        (
            &["--go-module", "example.com/ExampleProject"],
            "--go-module / go_module",
        ),
        (&["--rust-crate-root", "libs"], "rust_crate_roots"),
        (&["--sqlx-enabled", "true"], "sqlx_enabled = true"),
        (
            &["--schema-dump-enabled", "true"],
            "schema_dump_enabled = true",
        ),
        (
            &["--rust-migration-dir", "migrations"],
            "rust_migration_dir",
        ),
        (
            &["--rust-migration-layout", "flat_migrations"],
            "rust_migration_layout",
        ),
        (
            &["--rust-sqlx-metadata-dir", ".sqlx"],
            "rust_sqlx_metadata_dir",
        ),
        (&["--schema-dump-command", "dump"], "schema_dump_command"),
        (&["--schema-docs-dir", "docs/schema"], "schema_docs_dir"),
        (&["--schema-check-command", "check"], "schema_check_command"),
        (&["--sqlx-check-command", "check"], "sqlx_check_command"),
        (&["--migration-add-command", "add"], "migration_add_command"),
        (&["--dev-command", "serve"], "dev_command"),
        (
            &["--application-contracts-enabled", "true"],
            "application_contracts_enabled = true",
        ),
    ];

    for (extra, expected) in cases {
        let mut args = vec![
            "jig",
            "init",
            "ExampleCli",
            "--preset",
            "rust-cli",
            "--no-input",
            "--no-vault",
        ];
        args.extend_from_slice(extra);
        let mut opts = init_opts(&args);

        let error = prepare(&mut opts).unwrap_err().to_string();

        assert_eq!(error, incompatible_error(expected), "{extra:?}");
    }
}

#[test]
fn rust_cli_rejects_every_incompatible_answer_file_family() {
    let cases = [
        (
            "harness_footprint = \"minimal\"\n",
            "harness_footprint = \"minimal\"",
        ),
        ("backend_language = \"go\"\n", "backend_language = \"go\""),
        (
            "go_module = \"example.com/ExampleProject\"\n",
            "--go-module / go_module",
        ),
        ("go_database = \"none\"\n", "go_database"),
        ("rust_crate_roots = []\n", "rust_crate_roots"),
        ("rust_crate_roots = [\"libs\"]\n", "rust_crate_roots"),
        ("sqlx_enabled = true\n", "sqlx_enabled = true"),
        ("schema_dump_enabled = true\n", "schema_dump_enabled = true"),
        (
            "rust_migration_dir = \"migrations\"\n",
            "rust_migration_dir",
        ),
        ("migration_dir = \"migrations\"\n", "migration_dir"),
        (
            "rust_migration_layout = \"flat_migrations\"\n",
            "rust_migration_layout",
        ),
        (
            "rust_sqlx_metadata_dir = \".sqlx\"\n",
            "rust_sqlx_metadata_dir",
        ),
        ("schema_dump_command = \"dump\"\n", "schema_dump_command"),
        ("schema_docs_dir = \"docs/schema\"\n", "schema_docs_dir"),
        ("schema_check_command = \"check\"\n", "schema_check_command"),
        ("sqlx_check_command = \"check\"\n", "sqlx_check_command"),
        ("migration_add_command = \"add\"\n", "migration_add_command"),
        ("go_fmt_check_command = \"fmt\"\n", "go_fmt_check_command"),
        ("go_lint_command = \"lint\"\n", "go_lint_command"),
        ("go_test_command = \"test\"\n", "go_test_command"),
        (
            "go_test_locked_command = \"test\"\n",
            "go_test_locked_command",
        ),
        ("sqlc_check_command = \"sqlc\"\n", "sqlc_check_command"),
        (
            "typescript_lint_command = \"lint\"\n",
            "typescript_lint_command",
        ),
        (
            "typescript_typecheck_command = \"typecheck\"\n",
            "typescript_typecheck_command",
        ),
        (
            "typescript_build_command = \"build\"\n",
            "typescript_build_command",
        ),
        (
            "typescript_coverage_command = \"coverage\"\n",
            "typescript_coverage_command",
        ),
        ("dev_command = \"serve\"\n", "dev_command"),
        (
            "application_contracts_enabled = true\n",
            "application_contracts_enabled = true",
        ),
        (
            "[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            "frontend_apps",
        ),
        (
            "frontend_workspace_roots = [\"packages\"]\n",
            "frontend_workspace_roots",
        ),
        (
            "[[dev.apps]]\nname = \"worker\"\nargv = [\"cargo\", \"run\"]\n",
            "dev.apps",
        ),
        (
            "[repository]\ndefault_check_profile = \"verify\"\n",
            "repository",
        ),
        (
            "[commands]\nrepo_bootstrap_command = \"cargo fetch\"\n",
            "commands",
        ),
        ("[work]\nrequired_gates = []\n", "work"),
        ("[loop]\nworkflows = []\n", "loop"),
        (
            "unexpected_shape_authority = true\n",
            "unknown top-level answer key `unexpected_shape_authority`",
        ),
    ];
    let temp = tempdir().unwrap();

    for (index, (body, expected)) in cases.into_iter().enumerate() {
        let answers = temp.path().join(format!("answers-{index}.toml"));
        fs::write(&answers, body).unwrap();
        let mut opts = init_opts(&[
            "jig",
            "init",
            "ExampleCli",
            "--preset",
            "rust-cli",
            "--answers-file",
            answers.to_str().unwrap(),
            "--no-input",
            "--no-vault",
        ]);

        let error = prepare(&mut opts).unwrap_err().to_string();

        assert_eq!(error, incompatible_error(expected), "{body}");
    }
}

#[test]
fn rust_cli_answer_parse_errors_still_name_the_selected_preset_and_field() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(&answers, "[dev]\nproxy_por = 2455\n").unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleCli",
        "--preset",
        "rust-cli",
        "--answers-file",
        answers.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    let error = prepare(&mut opts).unwrap_err().to_string();

    assert!(error.contains("rust-cli"), "{error}");
    assert!(error.contains("proxy_por"), "{error}");
}

#[test]
fn rust_cli_treats_normalized_empty_optional_strings_as_absent() {
    let temp = tempdir().unwrap();
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        r#"rust_migration_dir = ""
migration_dir = ""
rust_sqlx_metadata_dir = ""
schema_dump_command = ""
schema_check_command = ""
sqlx_check_command = ""
migration_add_command = ""
go_module = ""
go_fmt_check_command = ""
go_lint_command = ""
go_test_command = ""
go_test_locked_command = ""
sqlc_check_command = ""
typescript_lint_command = ""
typescript_typecheck_command = ""
typescript_build_command = ""
typescript_coverage_command = ""
dev_command = ""
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "ExampleCli",
        "--preset",
        "rust-cli",
        "--answers-file",
        answers.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    prepare(&mut opts).unwrap();
}
