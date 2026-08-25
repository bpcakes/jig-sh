
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

#[cfg(unix)]
#[test]
fn scaffold_rendered_go_is_formatted_and_config_handles_ip_hosts() {
    let planning_root = tempdir().unwrap();
    for db in [ScaffoldDb::None, ScaffoldDb::Postgres] {
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::GoReact),
                db: Some(db),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                go_module: Some("example.com/ExampleProject".into()),
                ..AnswerOpts::default()
            },
            planning_root.path(),
        )
        .unwrap()
        .unwrap();

        assert_rendered_scaffold_go_is_formatted_and_config_is_runnable(
            &plan,
            match db {
                ScaffoldDb::None => "none",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => unreachable!(),
            },
        );
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
    assert!(paths.contains("internal/config/config_test.go"));
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
    let config = rendered
        .iter()
        .find(|file| file.relative == "internal/config/config.go")
        .unwrap();
    assert!(api_main.contents.contains("godotenv.Load()"));
    assert!(
        api_main
            .contents
            .find("parseCommand(os.Args[1:])")
            .unwrap()
            < api_main.contents.find("config.Load()").unwrap()
    );
    assert!(config.contents.contains("DatabaseURL"));
    assert!(config.contents.contains("DATABASE_URL"));
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
    assert!(
        api_main_test
            .contents
            .contains("func TestRunRejectsInvalidCommandBeforeLoadingConfig")
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
    assert!(!contracts.contents.contains("execFile"));
    assert!(!contracts.contents.contains("promisify"));
    assert!(contracts.contents.contains("async function withStagedClients()"));
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
fn go_react_without_database_keeps_runtime_dependencies_and_omits_database_boundaries() {
    let planning_root = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            go_module: Some("example.com/ExampleProject".into()),
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
    let go_mod = rendered
        .iter()
        .find(|file| file.relative == "go.mod")
        .unwrap();
    let api_main = rendered
        .iter()
        .find(|file| file.relative == "cmd/api/main.go")
        .unwrap();
    let config = rendered
        .iter()
        .find(|file| file.relative == "internal/config/config.go")
        .unwrap();

    assert!(go_mod.contents.contains("github.com/joho/godotenv v1.5.1"));
    assert!(!go_mod.contents.contains("github.com/jackc/pgx"));
    assert!(!go_mod.contents.contains("github.com/pressly/goose"));
    assert!(!go_mod.contents.contains("github.com/sqlc-dev/sqlc"));
    assert!(!go_mod.contents.contains("tool ("));
    assert!(api_main.contents.contains("godotenv.Load()"));
    assert!(!config.contents.contains("DatabaseURL"));
    assert!(!config.contents.contains("DATABASE_URL"));
    assert!(!paths.contains("cmd/api/database_command.go"));
    assert!(!paths.contains("internal/database/database.go"));
    assert!(!paths.contains("sqlc.yaml"));
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
            ci_github_runner: Some("macos-14".into()),
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
        assert_eq!(
            workflow.matches(path).count(),
            2,
            "missing {path} in:\n{workflow}"
        );
    }
    assert!(workflow.contains("node scripts/contracts.mjs client-check"));
    assert!(!workflow.contains("if [ -f scripts/contracts.mjs ]"));
    let build_step = workflow.find("Run build").unwrap();
    let client_check_step = workflow
        .find("Check generated API clients and public boundary")
        .unwrap();
    assert!(build_step < client_check_step);

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
        assert!(
            workflow.contains("go-version: ${{ steps.go-version.outputs.version }}"),
            "{workflow_name} must pass the resolved component authority to setup-go"
        );
        assert!(workflow.contains("version=\"$(scripts/jig info go-version)\""));
        assert!(
            !workflow.contains("go-version-file: .go-version")
                && !workflow.contains("go-version-file: \".go-version\""),
            "{workflow_name} must not refer to the retired Go version file"
        );
        assert!(workflow.contains(
            "cache-dependency-path: |\n            go.mod\n            go.sum\n            go.work\n            go.work.sum\n            **/go.mod"
        ));
        for path in [
            "go.mod",
            "go.sum",
            "go.work",
            "go.work.sum",
            "**/go.mod",
            "**/go.sum",
            "**/go.work",
            "**/go.work.sum",
            "vendor/modules.txt",
            "**/vendor/modules.txt",
        ] {
            assert_eq!(
                workflow.matches(&format!(r#"- "{path}""#)).count(),
                2,
                "{workflow_name} must track adapter input {path} on pull requests and pushes"
            );
        }
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
        assert_eq!(workflow.matches(r#"- "**/*.sql""#).count(), 2);
    }

    let go_tests =
        fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    assert_eq!(go_tests.matches(r#"- "openapi/**""#).count(), 2);
    for path in [
        "go.mod",
        "go.sum",
        "go.work",
        "go.work.sum",
        "**/go.mod",
        "**/go.sum",
        "**/go.work",
        "**/go.work.sum",
        "vendor/modules.txt",
        "**/vendor/modules.txt",
    ] {
        assert_eq!(
            go_tests.matches(&format!(r#"- "{path}""#)).count(),
            2,
            "Go CI must track adapter input {path} on pull requests and pushes"
        );
    }
    assert_eq!(
        go_tests
            .matches(r#"- "scripts/test-postgres.sh""#)
            .count(),
        2
    );
    assert_eq!(go_tests.matches("runs-on: \"macos-14\"").count(), 1);
    assert_eq!(
        go_tests.matches("runs-on: \"ubuntu-latest\"").count(),
        1
    );
    assert!(go_tests.contains("run: bash scripts/test-postgres.sh"));
    assert!(!destination.join(".go-version").exists());

    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains(".contract-stage-*/"));
    assert!(gitignore.contains(".contract-client-stage-*/"));

    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert!(browser_e2e.contains("version=\"$(scripts/jig info go-version)\""));
    assert!(browser_e2e.contains("go-version: ${{ steps.go-version.outputs.version }}"));
    assert!(browser_e2e.contains("cache-dependency-path: |"));

    let config = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(config.contains(r#"migration_dir = "internal/database/migrations""#));
    assert!(!config.contains("rust_migration_dir ="));
    assert!(!config.contains("backend_language ="));
    assert!(!config.contains("go_database ="));
    assert!(config.contains("[repository]"));
    assert!(config.contains(
        r#"affected_ignore = [".env", ".env.*", "**/.env", "**/.env.*", "README.md", "**/README.md", "AGENTS.md", "**/AGENTS.md", "agent-map.md", "CHANGELOG.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md", "docs/**", "LICENSE", "LICENSE.*", ".github/**"]"#
    ));
    assert!(config.contains("api_test_command = \"go test ./...\""));
    assert!(config.contains("web_test_command = \"scripts/check-webapps.sh check-one"));
    assert!(config.contains("action = \"frontend-contract-drift\""));
    assert!(config.contains("action = \"frontend-public-boundary\""));
    assert!(config.contains("contracts-drift-check"));
    assert!(config.contains("contracts-boundary-check"));
    let config_value = toml::from_str::<toml::Value>(&config).unwrap();
    assert_eq!(
        config_value["work"]["gates"][0]["profile"].as_str(),
        Some("verify")
    );
    let contract = fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""name": "jig.migration_add""#));
    let contract_value = serde_json::from_str::<serde_json::Value>(&contract).unwrap();
    assert_eq!(contract_value["default_check_profile"], "verify");
    assert_eq!(
        contract_value["affected_ignore"],
        serde_json::json!([
            ".env",
            ".env.*",
            "**/.env",
            "**/.env.*",
            "README.md",
            "**/README.md",
            "AGENTS.md",
            "**/AGENTS.md",
            "agent-map.md",
            "CHANGELOG.md",
            "CONTRIBUTING.md",
            "CODE_OF_CONDUCT.md",
            "SECURITY.md",
            "docs/**",
            "LICENSE",
            "LICENSE.*",
            ".github/**"
        ])
    );
    for component in ["api", "web"] {
        assert!(contract_value["components"].as_array().unwrap().iter().any(
            |candidate| candidate["id"] == component
        ));
        assert!(contract_value["actions"].as_array().unwrap().iter().any(
            |action| action["target"]["component"] == component
                && action["target"]["action"] == "test"
        ));
    }
    for action in ["frontend-contract-drift", "frontend-public-boundary"] {
        assert!(contract_value["actions"].as_array().unwrap().iter().any(
            |candidate| candidate["target"]["component"] == "repo"
                && candidate["target"]["action"] == action
        ));
    }
    let root_guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("business logic in the owning package"));
    assert!(!root_guide.contains("business logic in the owning crate"));
    assert!(root_guide.contains("## Backend Guide Conventions"));
    assert!(root_guide.contains("scripts/jig migration add NAME"));
    let go_mod = fs::read_to_string(destination.join("go.mod")).unwrap();
    assert!(!go_mod.contains("github.com/pressly/goose/v3/cmd/goose"));
    let context = crate::context::RepoContext::load_from(&destination).unwrap();
    assert_eq!(crate::doctor::go_version_selector(&context).unwrap(), "1.26.0");
    let http_api = fs::read_to_string(destination.join("internal/httpapi/httpapi.go")).unwrap();
    assert!(http_api.contains("config.CreateHooks = nil"));
    let http_api_test =
        fs::read_to_string(destination.join("internal/httpapi/httpapi_test.go")).unwrap();
    assert!(http_api_test.contains("want field omitted when schema routes are disabled"));
    let openapi = fs::read_to_string(destination.join("openapi/public.json")).unwrap();
    assert!(!openapi.contains("\"$schema\""));
    let client_types = fs::read_to_string(
        destination.join("packages/public-api-client/src/generated/types.gen.ts"),
    )
    .unwrap();
    assert!(!client_types.contains("$schema"));
    let postgres_script = fs::read_to_string(destination.join("scripts/test-postgres.sh")).unwrap();
    assert!(!postgres_script.contains("seq 1 60"));
    assert!(postgres_script.contains("attempt=$((attempt + 1))"));
    assert!(postgres_script.contains("PostgreSQL container did not become queryable"));
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

    #[cfg(unix)]
    if Command::new("node")
        .arg("--version")
        .status()
        .is_ok_and(|status| status.success())
    {
        let fake_module = destination.join("node_modules/@hey-api/openapi-ts");
        fs::create_dir_all(&fake_module).unwrap();
        fs::write(
            fake_module.join("package.json"),
            r#"{"name":"@hey-api/openapi-ts","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        fs::write(
            fake_module.join("index.js"),
            r#"import { cp } from "node:fs/promises";

export async function createClient({ output }) {
  await cp("packages/public-api-client/src/generated", output, { recursive: true });
}
"#,
        )
        .unwrap();

        let fake_bin = destination.join(".fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        write_executable_test_script(
            &fake_bin.join("go"),
            "#!/bin/sh\n: > \"$JIG_TEST_GO_MARKER\"\nexit 91\n",
        );
        let backend_marker = destination.join(".backend-exporter-ran");
        let path = std::env::join_paths(
            std::iter::once(fake_bin).chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
        )
        .unwrap();
        let before = regular_file_tree_snapshot(&destination);

        let output = Command::new("node")
            .args(["scripts/contracts.mjs", "client-check"])
            .current_dir(&destination)
            .env("PATH", path)
            .env("JIG_TEST_GO_MARKER", &backend_marker)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "client-check failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!backend_marker.exists(), "client-check invoked Go");
        assert_eq!(regular_file_tree_snapshot(&destination), before);
    }

    let nested_module_dir = destination.join("services/api");
    fs::create_dir_all(&nested_module_dir).unwrap();
    fs::rename(destination.join("go.mod"), nested_module_dir.join("go.mod")).unwrap();
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let api_component = config["repository"]["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|component| component["id"].as_str() == Some("api"))
        .unwrap();
    api_component["root"] = toml::Value::String("services/api".into());
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow = fs::read_to_string(
            destination
                .join(".github/workflows")
                .join(workflow_name),
        )
        .unwrap();
        assert!(workflow.contains("version=\"$(scripts/jig info go-version)\""));
        assert!(!workflow.contains("go-version-file: go.mod"));
    }
    let context = crate::context::RepoContext::load_from(&destination).unwrap();
    assert_eq!(crate::doctor::go_version_selector(&context).unwrap(), "1.26.0");
    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert!(browser_e2e.contains("version=\"$(scripts/jig info go-version)\""));
    assert!(!browser_e2e.contains("go-version-file: go.mod"));

    fs::remove_file(destination.join("scripts/test-postgres.sh")).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();
    let go_tests =
        fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    assert!(go_tests.contains("scripts/jig check sqlc"));
    assert!(!go_tests.contains("postgres-integration:"));
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
