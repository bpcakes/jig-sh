
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
    assert!(go_mod.contents.contains("tool ("));
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
            db: Some(ScaffoldDb::None),
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
