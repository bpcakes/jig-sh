
#[cfg(unix)]
#[test]
fn scaffold_rendered_rust_is_formatted_across_names_databases_and_migration_paths() {
    let planning_root = tempdir().unwrap();
    let names = [
        ("manual-qa", "node22-npm12-sqlite-web".to_string()),
        ("width-40", format!("r{}", "a".repeat(39))),
        ("width-52", format!("r{}", "a".repeat(51))),
        ("width-71", format!("r{}", "a".repeat(70))),
        ("supported-max-216", format!("r{}", "a".repeat(215))),
    ];
    for (label, name) in &names {
        let expected_len = match *label {
            "manual-qa" => 23,
            "width-40" => 40,
            "width-52" => 52,
            "width-71" => 71,
            "supported-max-216" => 216,
            _ => unreachable!(),
        };
        assert_eq!(name.len(), expected_len, "{label}");
    }

    for db in [ScaffoldDb::None, ScaffoldDb::Sqlite, ScaffoldDb::Postgres] {
        let db_label = match db {
            ScaffoldDb::None => "none",
            ScaffoldDb::Sqlite => "sqlite",
            ScaffoldDb::Postgres => "postgres",
        };
        for (name_label, repo_name) in &names {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: vec![
                        ScaffoldFrontend {
                            name: "web".into(),
                            kind: ScaffoldFrontendKind::Spa,
                            custom_default_name: false,
                        },
                        ScaffoldFrontend {
                            name: "admin".into(),
                            kind: ScaffoldFrontendKind::Admin,
                            custom_default_name: false,
                        },
                    ],
                    frontend_list: Vec::new(),
                },
                &AnswerOpts {
                    repo_name: Some(repo_name.clone()),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(&plan, &format!("{db_label}/{name_label}"));
        }
    }

    for db in [ScaffoldDb::Sqlite, ScaffoldDb::Postgres] {
        let db_label = match db {
            ScaffoldDb::Sqlite => "sqlite",
            ScaffoldDb::Postgres => "postgres",
            ScaffoldDb::None => unreachable!(),
        };
        for migration_len in [13, 80, 216] {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                &AnswerOpts {
                    repo_name: Some("demo".into()),
                    rust_migration_dir: Some("m".repeat(migration_len)),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(
                &plan,
                &format!("{db_label}/migration-width-{migration_len}"),
            );
        }
    }
}

#[test]
fn go_react_postgres_renders_go_contract_and_database_boundaries() {
    let planning_root = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            }],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            go_module: Some("github.com/acme/demo".into()),
            ..AnswerOpts::default()
        },
        planning_root.path(),
    )
    .unwrap()
    .unwrap();

    let rendered = plan.render_files().unwrap();
    let paths = rendered
        .iter()
        .map(|file| file.relative.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert!(paths.contains("go.mod"));
    assert!(paths.contains("cmd/api/main.go"));
    assert!(paths.contains("cmd/api/main_test.go"));
    assert!(paths.contains("cmd/api/database_command.go"));
    assert!(paths.contains("cmd/openapi/main.go"));
    assert!(paths.contains("sqlc.yaml"));
    assert!(paths.contains("internal/database/migrations/00001_app_metadata.sql"));
    assert!(paths.contains("internal/database/database_test.go"));
    assert!(paths.contains("scripts/test-postgres.sh"));
    assert!(paths.contains("internal/database/sqlc/db.go"));
    assert!(!paths.contains("Cargo.toml"));

    let go_mod = rendered
        .iter()
        .find(|file| file.relative == "go.mod")
        .unwrap();
    assert!(go_mod.contents.contains("module github.com/acme/demo"));
    assert!(go_mod.contents.contains("go 1.26.0"));
    assert!(go_mod.contents.contains("github.com/joho/godotenv"));
    assert!(go_mod.contents.contains("tool ("));
    let api_main = rendered
        .iter()
        .find(|file| file.relative == "cmd/api/main.go")
        .unwrap();
    assert!(api_main.contents.contains("godotenv.Load()"));
    assert!(api_main.contents.contains("net.Listen(\"tcp\", cfg.Address)"));
    assert!(
        api_main
            .contents
            .contains("func serve(ctx context.Context, server *http.Server, listener net.Listener) error")
    );
    assert!(api_main.contents.contains("server.Shutdown(shutdownCtx)"));
    assert!(api_main.contents.contains("serveErr := <-serverDone"));
    let api_main_test = rendered
        .iter()
        .find(|file| file.relative == "cmd/api/main_test.go")
        .unwrap();
    assert!(
        api_main_test
            .contents
            .contains("func TestServeWaitsForInflightRequestsDuringShutdown")
    );
    let database_command = rendered
        .iter()
        .find(|file| file.relative == "cmd/api/database_command.go")
        .unwrap();
    assert!(
        database_command
            .contents
            .contains("--bootstrap-database")
    );
    let database = rendered
        .iter()
        .find(|file| file.relative == "internal/database/database.go")
        .unwrap();
    assert!(database.contents.contains("func Bootstrap("));
    assert!(database.contents.contains("CREATE DATABASE"));
    let bootstrap_start = database.contents.find("func Bootstrap(").unwrap();
    let open_start = database.contents.find("func Open(").unwrap();
    let migrate_start = database.contents.find("func migrate(").unwrap();
    assert!(
        database.contents[bootstrap_start..open_start]
            .contains("if err := migrate(ctx, databaseURL); err != nil")
    );
    assert!(
        !database.contents[open_start..migrate_start]
            .contains("migrate(ctx, databaseURL)")
    );
    let database_test = rendered
        .iter()
        .find(|file| file.relative == "internal/database/database_test.go")
        .unwrap();
    assert!(database_test.contents.contains("database.Bootstrap(ctx, databaseURL)"));
    let playwright = rendered
        .iter()
        .find(|file| file.relative == "web/playwright.config.ts")
        .unwrap();
    assert!(
        playwright
            .contents
            .contains("go run ./cmd/api --bootstrap-database")
    );
    let contracts = rendered
        .iter()
        .find(|file| file.relative == "scripts/contracts.mjs")
        .unwrap();
    assert!(contracts.contents.contains(r#"run("go", ["run", "./cmd/openapi""#));
    assert!(!contracts.contents.contains(r#"run("cargo""#));
    let httpapi_test = rendered
        .iter()
        .find(|file| file.relative == "internal/httpapi/httpapi_test.go")
        .unwrap();
    assert!(httpapi_test.contents.contains("func TestOpenAPIIsCurrent"));
    assert!(httpapi_test.contents.contains("public OpenAPI document is stale"));
    assert!(
        httpapi_test
            .contents
            .contains(r#"filepath.Join("..", "..", "openapi", "public.json")"#)
    );
    assert!(!httpapi_test.contents.contains("runtime.Caller"));
}

#[test]
fn go_react_web_workflow_observes_the_complete_application_contract() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("go-contract-workflow");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            }],
            frontend_list: Vec::new(),
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("go-contract-workflow".into()),
            go_module: Some("example.com/go-contract-workflow".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let workflow =
        fs::read_to_string(destination.join(".github/workflows/webapp-checks.yml")).unwrap();
    for path in [
        r#"- "cmd/**""#,
        r#"- "internal/**""#,
        r#"- "openapi/**""#,
        r#"- "packages/public-api-client/**""#,
    ] {
        assert_eq!(workflow.matches(path).count(), 2, "missing {path}");
    }

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow = fs::read_to_string(
            destination
                .join(".github/workflows")
                .join(workflow_name),
        )
        .unwrap();
        assert!(
            workflow.contains("actions-rust-lang/setup-rust-toolchain@v1"),
            "{workflow_name} must install Rust before invoking scripts/jig"
        );
        assert!(workflow.contains("cache-dependency-path: go.mod"));
        assert_eq!(
            workflow
                .matches(r#"- "internal/database/queries/**""#)
                .count(),
            2
        );
        assert_eq!(
            workflow
                .matches(r#"- "internal/database/migrations/**""#)
                .count(),
            2
        );
    }

    let go_tests =
        fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    assert_eq!(go_tests.matches(r#"- ".go-version""#).count(), 2);

    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert!(browser_e2e.contains("cache-dependency-path: go.mod"));

    let config = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(config.contains(r#"migration_dir = "internal/database/migrations""#));
    assert!(!config.contains("rust_migration_dir ="));
    let contract =
        fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""name": "jig.migration_add""#));
    let root_guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("business logic in the owning package"));
    assert!(!root_guide.contains("business logic in the owning crate"));
    assert!(root_guide.contains("## Backend Guide Conventions"));
    assert!(root_guide.contains("scripts/jig migration add NAME"));
    let go_mod = fs::read_to_string(destination.join("go.mod")).unwrap();
    assert!(!go_mod.contains("github.com/pressly/goose/v3/cmd/goose"));
    let policy =
        fs::read_to_string(destination.join(".github/workflows/repo-policy.yml")).unwrap();
    let policy: serde_json::Value = serde_yaml_ng::from_str(&policy).unwrap();
    assert!(policy["jobs"]["migration-immutability"].is_object());
    assert!(policy["jobs"]["sqlx-unchecked-queries"].is_null());

    let config_path = destination.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        r#"migration_dir = "internal/database/migrations""#,
        r#"migration_dir = "database/migrations""#,
    );
    fs::write(&config_path, config).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow = fs::read_to_string(
            destination
                .join(".github/workflows")
                .join(workflow_name),
        )
        .unwrap();
        assert_eq!(workflow.matches(r#"- "database/migrations/**""#).count(), 2);
        assert!(!workflow.contains(r#"- "internal/database/migrations/**""#));
    }
}

#[test]
fn go_react_rejects_missing_module_sqlite_and_admin() {
    let planning_root = tempdir().unwrap();
    let missing_module = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts::default(),
        planning_root.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(missing_module.contains("--go-module"));

    let sqlite = ScaffoldOpts {
        preset: Some(ScaffoldPreset::GoReact),
        db: Some(ScaffoldDb::Sqlite),
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts {
        go_module: Some("example.com/demo".into()),
        ..AnswerOpts::default()
    })
    .unwrap_err()
    .to_string();
    assert!(sqlite.contains("does not support --db sqlite"));

    let admin = ScaffoldOpts {
        preset: Some(ScaffoldPreset::GoReact),
        db: Some(ScaffoldDb::None),
        frontends: vec![ScaffoldFrontend {
            name: "admin".into(),
            kind: ScaffoldFrontendKind::Admin,
            custom_default_name: false,
        }],
        frontend_list: Vec::new(),
    }
    .validate_init_invariants(&AnswerOpts {
        go_module: Some("example.com/demo".into()),
        ..AnswerOpts::default()
    })
    .unwrap_err()
    .to_string();
    assert!(admin.contains("separate privileged API and client boundary"));
}
