use super::*;

#[test]
fn scaffold_postgres_development_database_name_respects_identifier_limit() {
    let temp = tempdir().unwrap();
    let repo_name = "project".repeat(12);
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some(repo_name),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    let database_name = env_example
        .lines()
        .find_map(|line| line.strip_prefix("DATABASE_URL="))
        .and_then(|url| url.rsplit('/').next())
        .unwrap();
    assert_eq!(database_name.len(), 63);
    assert!(database_name.contains('_'));
}

#[test]
fn scaffold_db_defaults_set_sqlx_metadata_and_disable_schema_dump() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(answers.rust_sqlx_metadata_dir.as_deref(), Some(".sqlx"));
    assert_eq!(answers.schema_dump_enabled, Some(false));
}

#[test]
fn scaffold_bootstrap_command_records_shared_web_dependency_state() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
            ],
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    for package_manager in ["bun", "npm", "pnpm", "yarn"] {
        let mut answers = AnswerOpts {
            web_package_manager: Some(package_manager.into()),
            ..AnswerOpts::default()
        };
        plan.apply_answer_defaults(&mut answers);
        let bootstrap_command = answers.bootstrap_command.unwrap();
        assert!(bootstrap_command.ends_with("&& scripts/check-webapps.sh bootstrap"));
        assert_eq!(
            bootstrap_command
                .matches("scripts/check-webapps.sh bootstrap")
                .count(),
            1
        );
        assert!(!bootstrap_command.contains("cd web"));
        assert!(!bootstrap_command.contains("cd landing"));
    }

    let mut default_answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut default_answers);
    assert_eq!(default_answers.web_package_manager.as_deref(), Some("bun"));
    assert!(
        default_answers
            .bootstrap_command
            .unwrap()
            .ends_with("&& scripts/check-webapps.sh bootstrap")
    );
}

#[test]
fn scaffold_separates_dependency_bootstrap_from_database_setup() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    let command = answers.bootstrap_command.unwrap();
    assert_text_contains_none(&command, &["DATABASE_URL", "--bootstrap-database"]);
    let cargo_fetch = command.find("cargo fetch").unwrap();
    let frontend_bootstrap = command.find("scripts/check-webapps.sh bootstrap").unwrap();
    assert!(cargo_fetch < frontend_bootstrap);
    plan.write(temp.path(), false).unwrap();
    let setup = fs::read_to_string(temp.path().join("scripts/setup-database.sh")).unwrap();
    let env_check = setup
        .find("if [ -z \"${DATABASE_URL:-}\" ] && ! awk")
        .unwrap();
    let database_bootstrap = setup
        .find("cargo run -p demo-api -- --bootstrap-database")
        .unwrap();
    assert!(env_check < database_bootstrap);
}

#[test]
fn go_scaffold_separates_codegen_from_database_setup() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            go_module: Some("github.com/acme/demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(
        answers.migration_dir.as_deref(),
        Some("internal/database/migrations")
    );
    let command = answers.bootstrap_command.unwrap();
    let module_tidy = command.find("go mod tidy").unwrap();
    let frontend_bootstrap = command.find("scripts/check-webapps.sh bootstrap").unwrap();
    assert_text_contains_none(&command, &["DATABASE_URL", "--bootstrap-database"]);
    let sqlc_generate = command.find("go tool sqlc generate").unwrap();
    let contract_generate = command.find("node scripts/contracts.mjs generate").unwrap();
    assert!(module_tidy < frontend_bootstrap);
    assert!(frontend_bootstrap < sqlc_generate);
    assert!(sqlc_generate < contract_generate);
    plan.write(temp.path(), false).unwrap();
    let setup = fs::read_to_string(temp.path().join("scripts/setup-database.sh")).unwrap();
    let database_guard = setup.find("Missing DATABASE_URL").unwrap();
    let sqlc_generate = setup.find("go tool sqlc generate").unwrap();
    let database_bootstrap = setup.find("go run ./cmd/api --bootstrap-database").unwrap();
    assert!(database_guard < sqlc_generate && sqlc_generate < database_bootstrap);
}

#[test]
fn go_scaffold_without_postgres_does_not_emit_migration_configuration() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some("example-project".into()),
            go_module: Some("example.com/example-project".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(
        answers.go_database,
        Some(jig_context::backend::GoDatabase::None)
    );
    assert_eq!(answers.migration_dir, None);
}

#[test]
fn scaffold_db_rejects_explicit_sqlx_disabled_answer() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold --db requires SQLx"));
}
