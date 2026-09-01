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
fn go_scaffold_keeps_names_that_are_only_rust_keywords() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts {
            repo_name: Some("loop".into()),
            go_module: Some("example.com/loop".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(plan.summary().contains("Go backend for loop"));
    assert!(plan.sanitized_repo_name_note().is_none());
    plan.write(temp.path(), false).unwrap();
    let workspace = fs::read_to_string(temp.path().join("package.json")).unwrap();
    assert!(workspace.contains(r#""name": "loop-workspace""#));
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

fn assert_sqlite_scaffold_manifests(root: &Path) {
    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_text_contains_all(
        &cargo_toml,
        &[
            "\"sqlite\"",
            "\"signal\", \"sync\", \"time\"",
            "fs4 = \"0.13.1\"",
            "url = \"2\"",
        ],
    );
    assert!(cargo_toml.ends_with('\n'));
    assert_eq!(
        fs::read_to_string(root.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=demo=info,demo_api=info,tower_http=info\nDATABASE_URL=sqlite:demo.db\n"
    );
    let db_cargo = fs::read_to_string(root.join("crates/demo-db/Cargo.toml")).unwrap();
    assert_text_contains_all(
        &db_cargo,
        &[
            "anyhow.workspace = true",
            "fs4.workspace = true",
            "url.workspace = true",
            "tokio.workspace = true",
        ],
    );
}

fn assert_sqlite_database_helper(root: &Path) {
    let db_lib = fs::read_to_string(root.join("crates/demo-db/src/lib.rs")).unwrap();
    assert_text_contains_all(
        &db_lib,
        &[
            "SqlitePool",
            "sqlx::Sqlite::database_exists",
            "OpenOptions::new()",
            ".create_new(true)",
            "options.get_filename()",
            "fs::create_dir_all(parent)",
            "create_if_missing",
            "concurrent_create_if_missing_calls_are_idempotent",
            "sqlx::migrate!(\n",
            "\"../../db/migrations\"\n        )",
            "DEFAULT_DB_TIMEOUT",
            "connect_with_timeout",
            "fs::canonicalize(&database_filename)",
            "sqlite_database_url_is_in_memory",
            "sqlite_database_url_semantics",
            "requires_single_connection_pool",
            "SqlitePoolOptions::new()",
            ".max_connections(1)",
            ".min_connections(1)",
            ".idle_timeout(None)",
            ".max_lifetime(None)",
            ".test_before_acquire(false)",
            "mirrors_sqlx_ordered_in_memory_cache_semantics",
            "in_memory_mode_ignores_an_existing_filename_for_locking",
            "create_if_missing_does_not_materialize_an_in_memory_filename",
            "symlink_aliases_share_the_canonical_migration_lock",
            "migrate_with_timeout",
            "static SQLITE_MIGRATION_LOCK",
            "fs4::fs_std::FileExt::try_lock_exclusive(&file)",
            "Ok(true) => return Ok(Some(file))",
            "Ok(false) =>",
            "in_memory_database_connects_and_migrates_without_a_file_lock",
            "private_cache_in_memory_pool_waits_for_the_active_checkout",
            "shared_in_memory_urls_keep_multiple_schema_aware_connections",
            "ordinary_file_pool_keeps_multiple_schema_aware_connections",
            "migration_mutex_is_shared_by_separate_in_memory_connections",
        ],
    );
    assert_text_contains_none(
        &db_lib,
        &[
            "sqlx::Sqlite::create_database",
            "num_idle()",
            "fs4::lock_contended_error",
        ],
    );
    assert!(root.join("db/migrations/.gitkeep").exists());
}

fn assert_sqlite_playwright(root: &Path) {
    let playwright = fs::read_to_string(root.join("web/playwright.config.ts")).unwrap();
    assert_text_contains_all(
        &playwright,
        &[
            "E2E_DATABASE_URL",
            "sqlite:${defaultDatabasePath}",
            "demo_web_e2e.sqlite",
            "-- --bootstrap-database",
            "['','-shm','-wal','-journal']",
        ],
    );
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
        let database = root.join(".agent/tmp/demo_web_e2e.sqlite");
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        for suffix in ["", "-shm", "-wal", "-journal"] {
            fs::write(format!("{}{}", database.display(), suffix), "stale\n").unwrap();
        }
        assert!(
            std::process::Command::new("bash")
                .args(["-c", &reset_command])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        for suffix in ["", "-shm", "-wal", "-journal"] {
            assert!(!Path::new(&format!("{}{}", database.display(), suffix)).exists());
        }
    }
}

fn assert_sqlite_e2e_workflow(root: &Path) {
    let workflow = fs::read_to_string(root.join(".github/workflows/e2e.yml")).unwrap();
    let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], "macos-14");
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert_text_contains_all(
        &workflow,
        &[
            r#"- "db/migrations/**""#,
            r#"- ".sqlx/**""#,
            r#"SQLX_OFFLINE: "true""#,
        ],
    );
    assert_text_contains_none(&workflow, &["image: postgres", "E2E_DATABASE_URL"]);
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
    assert_sqlite_scaffold_manifests(temp.path());
    assert_sqlite_database_helper(temp.path());
    assert_sqlite_playwright(temp.path());
    assert_sqlite_e2e_workflow(temp.path());
}

mod output_paths;
