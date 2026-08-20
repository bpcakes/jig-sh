
#[test]
fn scaffold_uses_explicit_frontend_role_without_name_inference() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            frontend_apps: vec![
                FrontendApp {
                    name: "admin".into(),
                    dir: "plain-admin-name".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "operations".into(),
                    dir: "operations".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "admin".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["frontends"][0]["role"], "spa");
    assert_eq!(report["frontends"][0]["ui"]["style"], "radix-nova");
    assert!(temp.path().join("plain-admin-name/src/App.tsx").exists());
    assert!(
        temp.path()
            .join("plain-admin-name/components.json")
            .exists()
    );
    assert!(
        !temp
            .path()
            .join("plain-admin-name/src/components/ui/sidebar.tsx")
            .exists()
    );
    assert_eq!(report["frontends"][1]["role"], "admin");
    assert_eq!(report["frontends"][1]["ui"]["style"], "radix-nova");
    assert!(temp.path().join("operations/components.json").exists());
    assert!(
        temp.path()
            .join("operations/src/components/ui/sidebar.tsx")
            .exists()
    );

    for (index, dir) in [(0, "plain-admin-name"), (1, "operations")] {
        let ui = &report["frontends"][index]["ui"];
        let package: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path().join(dir).join("package.json")).unwrap())
                .unwrap();
        let components: serde_json::Value = serde_json::from_slice(
            &fs::read(temp.path().join(dir).join("components.json")).unwrap(),
        )
        .unwrap();
        let readme = fs::read_to_string(temp.path().join(dir).join("README.md")).unwrap();
        let cli_version = ui["cli_version"].as_str().unwrap();
        let preset = ui["preset"].as_str().unwrap();
        let base = ui["base"].as_str().unwrap();
        let base_display = format!("{}{}", base[..1].to_ascii_uppercase(), &base[1..]);
        let tailwind_major = ui["tailwind_major"].as_u64().unwrap();

        assert_eq!(package["dependencies"]["shadcn"], cli_version);
        assert_eq!(components["style"], ui["style"]);
        assert!(readme.contains(&format!("shadcn CLI {cli_version}")));
        assert!(readme.contains(&format!("`{preset}` preset")));
        assert!(readme.contains(&format!("{base_display} primitives")));
        assert!(readme.contains(&format!("Tailwind CSS {tailwind_major}")));
        assert!(readme.contains(&format!("shadcn@{cli_version} info")));
    }
}

#[test]
fn scaffold_rejects_unknown_frontend_role() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            frontend_apps: vec![FrontendApp {
                name: "console".into(),
                dir: "console".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "dashboard".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported frontend app role 'dashboard'"));
    assert!(error.contains("spa, admin, or astro"));
}

#[test]
fn scaffold_rejects_duplicate_and_unsafe_frontend_app_dirs() {
    let temp = tempdir().unwrap();
    let duplicate = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate.contains("Duplicate scaffold frontend 'web'"));

    let duplicate_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![
                FrontendApp {
                    name: "docs".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "marketing".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_dir.contains("Duplicate scaffold frontend dir 'shared'"));

    let duplicate_package_name = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![
                FrontendApp {
                    name: "foo_bar".into(),
                    dir: "foo_bar".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "foo-bar".into(),
                    dir: "foo-bar".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_package_name.contains("names 'foo_bar' and 'foo-bar' normalize"));
    assert!(duplicate_package_name.contains("workspace package name 'foo-bar'"));

    let unsafe_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "../web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(unsafe_dir.contains("Scaffold frontend dir must not contain '.' or '..'"));

    let empty_segment_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web//app".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(empty_segment_dir.contains("must not contain empty path segments"));

    let rust_root_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "ui".into(),
                dir: "crates/ui".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(rust_root_dir.contains("uses reserved directory 'crates/ui'"));
}

#[test]
fn scaffold_rejects_frontend_package_name_reserved_by_root_workspace() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("demo_workspace").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("frontend 'demo_workspace'"));
    assert!(error.contains("reserved root workspace package name 'demo-workspace'"));
}

#[test]
fn scaffold_rejects_mixed_scaffold_and_existing_frontend_app_inputs() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "admin".into(),
                dir: "admin".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot be combined with --frontend-app"));
}

#[test]
fn scaffold_rejects_frontend_dirs_reserved_for_rust_roots() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("apps").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("uses reserved directory 'apps'"));
}

#[test]
fn go_scaffold_rejects_direct_frontends_under_backend_roots() {
    let temp = tempdir().unwrap();
    for dir in ["cmd", "internal"] {
        let error = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::GoReact),
                db: Some(ScaffoldDb::None),
                frontends: vec![parse_scaffold_frontend(dir).unwrap()],
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                go_module: Some("example.com/example-project".into()),
                ..AnswerOpts::default()
            },
            temp.path(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(&format!("uses reserved directory '{dir}'")));
    }
}

#[test]
fn go_scaffold_rejects_answer_frontends_under_backend_roots() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts {
            go_module: Some("example.com/example-project".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "internal/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("uses reserved directory 'internal/web'"));
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

#[test]
fn scaffold_prefixes_repo_names_that_are_invalid_rust_crate_identifiers() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(plan.summary().contains("repo name app-123-type"));
    assert!(
        plan.sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'app-123-type'")
    );
    plan.write(temp.path(), false).unwrap();

    assert!(
        temp.path()
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
    let main_rs =
        fs::read_to_string(temp.path().join("apps/app-123-type-api/src/main.rs")).unwrap();
    assert!(main_rs.contains("use ::app_123_type_http as app_http_crate;"));
    assert!(main_rs.contains("app_http_crate::router"));
    let core_lib =
        fs::read_to_string(temp.path().join("crates/app-123-type-core/src/lib.rs")).unwrap();
    assert!(core_lib.contains("#[allow(clippy::useless_concat)]\npub const APP_NAME"));
    assert!(core_lib.contains("pub const APP_NAME: &str = concat!("));
    assert!(core_lib.contains("\"app-123-type\","));

    let mixed_case = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("MyApp".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    assert!(
        mixed_case
            .sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'myapp'")
    );
}

#[test]
fn run_init_sqlite_scaffold_keeps_sanitized_database_names_and_ignores_aligned() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["scaffold"]["repo_name"], "app-123-type");
    assert_eq!(output["scaffold"]["repo_name_sanitized_from"], "123-type");
    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("requested repo name '123-type' was normalized")
    }));
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"app-123-type\""));
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=app_123_type=info,app_123_type_api=info,tower_http=info\nDATABASE_URL=sqlite:app_123_type.db\n"
    );
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/app_123_type.db\n"));
    assert!(gitignore.contains("/app_123_type.db-*\n"));
    for database_file in [
        "app_123_type.db",
        "app_123_type.db-wal",
        "app_123_type.db-shm",
        "app_123_type.db-journal",
        "app_123_type.db-jig-migrate.lock",
    ] {
        fs::write(destination.join(database_file), "local database artifact").unwrap();
    }
    assert_eq!(
        git_stdout(
            &destination,
            [
                "check-ignore",
                "--",
                "app_123_type.db",
                "app_123_type.db-wal",
                "app_123_type.db-shm",
                "app_123_type.db-journal",
                "app_123_type.db-jig-migrate.lock",
            ],
        )
        .unwrap(),
        "app_123_type.db\napp_123_type.db-wal\napp_123_type.db-shm\napp_123_type.db-journal\napp_123_type.db-jig-migrate.lock"
    );
    assert!(
        destination
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
}

#[test]
fn scaffold_sqlite_branch_generates_sqlite_db_helper() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("db/migrations".into()),
            ci_github_runner: Some("macos-14".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["db"], "sqlite");
    let cargo_toml = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("\"sqlite\""));
    assert!(cargo_toml.contains("\"signal\", \"sync\", \"time\""));
    assert!(cargo_toml.contains("fs4 = \"0.13.1\""));
    assert!(cargo_toml.contains("url = \"2\""));
    assert!(cargo_toml.ends_with('\n'));
    assert_eq!(
        fs::read_to_string(temp.path().join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=demo=info,demo_api=info,tower_http=info\nDATABASE_URL=sqlite:demo.db\n"
    );
    let db_cargo = fs::read_to_string(temp.path().join("crates/demo-db/Cargo.toml")).unwrap();
    assert!(db_cargo.contains("anyhow.workspace = true"));
    assert!(db_cargo.contains("fs4.workspace = true"));
    assert!(db_cargo.contains("url.workspace = true"));
    assert!(db_cargo.contains("tokio.workspace = true"));
    let db_lib = fs::read_to_string(temp.path().join("crates/demo-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("SqlitePool"));
    assert!(db_lib.contains("sqlx::Sqlite::database_exists"));
    assert!(db_lib.contains("OpenOptions::new()"));
    assert!(db_lib.contains(".create_new(true)"));
    assert!(db_lib.contains("options.get_filename()"));
    assert!(db_lib.contains("fs::create_dir_all(parent)"));
    assert!(!db_lib.contains("sqlx::Sqlite::create_database"));
    assert!(db_lib.contains("create_if_missing"));
    assert!(db_lib.contains("concurrent_create_if_missing_calls_are_idempotent"));
    assert!(db_lib.contains("sqlx::migrate!(\n"));
    assert!(db_lib.contains("\"../../db/migrations\"\n        )"));
    assert!(db_lib.contains("DEFAULT_DB_TIMEOUT"));
    assert!(db_lib.contains("connect_with_timeout"));
    assert!(db_lib.contains("fs::canonicalize(&database_filename)"));
    assert!(db_lib.contains("sqlite_database_url_is_in_memory"));
    assert!(db_lib.contains("sqlite_database_url_semantics"));
    assert!(db_lib.contains("requires_single_connection_pool"));
    assert!(db_lib.contains("SqlitePoolOptions::new()"));
    assert!(db_lib.contains(".max_connections(1)"));
    assert!(db_lib.contains(".min_connections(1)"));
    assert!(db_lib.contains(".idle_timeout(None)"));
    assert!(db_lib.contains(".max_lifetime(None)"));
    assert!(db_lib.contains(".test_before_acquire(false)"));
    assert!(!db_lib.contains("num_idle()"));
    assert!(db_lib.contains("mirrors_sqlx_ordered_in_memory_cache_semantics"));
    assert!(db_lib.contains("in_memory_mode_ignores_an_existing_filename_for_locking"));
    assert!(db_lib.contains("create_if_missing_does_not_materialize_an_in_memory_filename"));
    assert!(db_lib.contains("symlink_aliases_share_the_canonical_migration_lock"));
    assert!(db_lib.contains("migrate_with_timeout"));
    assert!(db_lib.contains("static SQLITE_MIGRATION_LOCK"));
    assert!(db_lib.contains("fs4::fs_std::FileExt::try_lock_exclusive(&file)"));
    assert!(db_lib.contains("Ok(true) => return Ok(Some(file))"));
    assert!(db_lib.contains("Ok(false) =>"));
    assert!(!db_lib.contains("fs4::lock_contended_error"));
    assert!(db_lib.contains("in_memory_database_connects_and_migrates_without_a_file_lock"));
    assert!(db_lib.contains("private_cache_in_memory_pool_waits_for_the_active_checkout"));
    assert!(db_lib.contains("shared_in_memory_urls_keep_multiple_schema_aware_connections"));
    assert!(db_lib.contains("ordinary_file_pool_keeps_multiple_schema_aware_connections"));
    assert!(db_lib.contains("migration_mutex_is_shared_by_separate_in_memory_connections"));
    assert!(temp.path().join("db/migrations/.gitkeep").exists());
    let playwright = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    assert!(playwright.contains("E2E_DATABASE_URL"));
    assert!(playwright.contains("sqlite:${defaultDatabasePath}"));
    assert!(playwright.contains("demo_web_e2e.sqlite"));
    assert!(playwright.contains("-- --bootstrap-database"));
    assert!(playwright.contains("['','-shm','-wal','-journal']"));
    #[cfg(unix)]
    {
        let reset_line = playwright
            .lines()
            .find(|line| line.contains("node -e") && line.contains("fs.rmSync"))
            .unwrap()
            .trim();
        let reset_command = reset_line
            .strip_prefix('`')
            .and_then(|line| line.strip_suffix("`,"))
            .unwrap()
            .replace("${defaultDatabasePath}", ".agent/tmp/demo_web_e2e.sqlite");
        let database = temp.path().join(".agent/tmp/demo_web_e2e.sqlite");
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        for suffix in ["", "-shm", "-wal", "-journal"] {
            fs::write(format!("{}{}", database.display(), suffix), "stale\n").unwrap();
        }
        assert!(
            std::process::Command::new("bash")
                .args(["-c", &reset_command])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        for suffix in ["", "-shm", "-wal", "-journal"] {
            assert!(!Path::new(&format!("{}{}", database.display(), suffix)).exists());
        }
    }
    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], "macos-14");
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(!workflow.contains("image: postgres"));
    assert!(!workflow.contains("E2E_DATABASE_URL"));
    assert!(workflow.contains(r#"- "db/migrations/**""#));
    assert!(workflow.contains(r#"- ".sqlx/**""#));
    assert!(workflow.contains(r#"SQLX_OFFLINE: "true""#));
}

#[test]
fn scaffold_output_paths_include_template_collision_candidates() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let paths = plan.output_paths();
    for expected in [
        ".env.example",
        "Cargo.toml",
        "crates/demo-http/Cargo.toml",
        "crates/demo-http/AGENTS.md",
        "crates/demo-http/src/lib.rs",
        "crates/demo-db/Cargo.toml",
        "crates/demo-db/AGENTS.md",
        "crates/demo-db/src/lib.rs",
        "crates/demo/AGENTS.md",
        "crates/demo-test-support/AGENTS.md",
        "crates/demo-test-support/src/app.rs",
        "crates/demo-test-support/src/db.rs",
        "crates/demo-test-support/tests/http.rs",
        "migrations/.gitkeep",
        "package.json",
        ".node-version",
        ".github/workflows/e2e.yml",
        "web/package.json",
        "web/.gitignore",
        "web/playwright.config.ts",
        "web/e2e/app.spec.ts",
        "web/components.json",
        "web/src/App.tsx",
        "web/src/api.ts",
        "web/src/app/router.ts",
        "web/src/routeTree.gen.ts",
        "web/src/routes/index.tsx",
        "web/src/components/ui/button.tsx",
        "web/src/lib/utils.ts",
        "landing/package.json",
        "landing/src/pages/index.astro",
        "admin-panel/package.json",
        "admin-panel/components.json",
        "admin-panel/src/app/router.ts",
        "admin-panel/src/routeTree.gen.ts",
        "admin-panel/src/routes/index.tsx",
        "admin-panel/src/routes/settings.tsx",
        "admin-panel/src/components/ui/sidebar.tsx",
        "admin-panel/src/features/overview/overview-page.tsx",
    ] {
        assert!(
            paths.iter().any(|path| path == Path::new(expected)),
            "missing output path {expected}"
        );
    }
}

#[test]
fn scaffold_rejects_unsupported_package_manager_before_scripts_render() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            web_package_manager: Some("cargo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported web_package_manager 'cargo'"));
}
