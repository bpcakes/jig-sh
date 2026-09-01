#[test]
fn rust_react_dev_answer_authority_reaches_config_and_vite_fallback() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("ExampleProject");
    let answers = temp.path().join("answers.toml");
    fs::write(
        &answers,
        r#"[dev]
proxy_port = 2455
https_port = 2443
tld = "Example.TEST"
"#,
    )
    .unwrap();

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
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
            answers_file: Some(answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let config = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(config.contains("proxy_port = 2455"));
    assert!(config.contains("https_port = 2443"));
    assert!(config.contains("tld = \"example.test\""));
    let vite = fs::read_to_string(destination.join("web/vite.config.ts")).unwrap();
    assert!(vite.contains("http://api.exampleproject.example.test:2455"));
    assert!(!vite.contains("localhost:1355"));
}

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
    assert_rendered_paths(
        &rendered,
        &[
            "go.mod",
            "cmd/api/main.go",
            "cmd/api/main_test.go",
            "cmd/api/database_command.go",
            "cmd/openapi/main.go",
            "sqlc.yaml",
            "internal/config/config_test.go",
            "internal/database/migrations/00001_app_metadata.sql",
            "internal/database/database_test.go",
            "scripts/test-postgres.sh",
            "internal/database/sqlc/db.go",
        ],
    );
    assert_rendered_paths_absent(&rendered, &["Cargo.toml"]);

    let go_mod = rendered_contents(&rendered, "go.mod");
    assert_contains_all(
        go_mod,
        &[
            "module github.com/acme/demo",
            "go 1.26.0",
            "github.com/joho/godotenv",
            "tool (",
        ],
    );
    let api_main = rendered_contents(&rendered, "cmd/api/main.go");
    assert_contains_all(
        api_main,
        &[
            "godotenv.Load()",
            "net.Listen(\"tcp\", cfg.Address)",
            "func serve(ctx context.Context, server *http.Server, listener net.Listener) error",
            "server.Shutdown(shutdownCtx)",
            "serveErr := <-serverDone",
        ],
    );
    assert_text_before(api_main, "parseCommand(os.Args[1:])", "config.Load()");
    assert_contains_all(
        rendered_contents(&rendered, "internal/config/config.go"),
        &["DatabaseURL", "DATABASE_URL"],
    );
    assert_contains_all(
        rendered_contents(&rendered, "cmd/api/main_test.go"),
        &[
            "func TestServeWaitsForInflightRequestsDuringShutdown",
            "func TestRunRejectsInvalidCommandBeforeLoadingConfig",
        ],
    );
    assert_contains_all(
        rendered_contents(&rendered, "cmd/api/database_command.go"),
        &["--bootstrap-database"],
    );

    let database = rendered_contents(&rendered, "internal/database/database.go");
    assert_contains_all(database, &["func Bootstrap(", "CREATE DATABASE"]);
    let bootstrap_start = database.find("func Bootstrap(").unwrap();
    let open_start = database.find("func Open(").unwrap();
    let migrate_start = database.find("func migrate(").unwrap();
    assert_contains_all(
        &database[bootstrap_start..open_start],
        &["if err := migrate(ctx, databaseURL); err != nil"],
    );
    assert_contains_none(
        &database[open_start..migrate_start],
        &["migrate(ctx, databaseURL)"],
    );
    assert_contains_all(
        rendered_contents(&rendered, "internal/database/database_test.go"),
        &["database.Bootstrap(ctx, databaseURL)"],
    );
    assert_contains_all(
        rendered_contents(&rendered, "web/playwright.config.ts"),
        &["go run ./cmd/api --bootstrap-database"],
    );
    let contracts = rendered_contents(&rendered, "scripts/contracts.mjs");
    assert_contains_all(
        contracts,
        &[
            r#"run("go", ["run", "./cmd/openapi""#,
            r#"join(backendRoot, "go.mod")"#,
            "async function withStagedClients()",
        ],
    );
    assert_contains_none(contracts, &[r#"run("cargo""#, "execFile", "promisify"]);
    let httpapi_test = rendered_contents(&rendered, "internal/httpapi/httpapi_test.go");
    assert_contains_all(
        httpapi_test,
        &[
            "func TestOpenAPIIsCurrent",
            "public OpenAPI document is stale",
            r#"filepath.FromSlash("../../openapi/public.json")"#,
        ],
    );
    assert_contains_none(httpapi_test, &["runtime.Caller"]);
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

fn assert_quoted_workflow_path_counts(workflow: &str, paths: &[&str], expected: usize) {
    for path in paths {
        assert_eq!(
            workflow.matches(&format!(r#"- "{path}""#)).count(),
            expected,
            "workflow has the wrong filter count for {path}"
        );
    }
}

fn assert_go_adapter_workflow(destination: &Path, workflow_name: &str) {
    let workflow =
        fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
    assert_contains_all(
        &workflow,
        &[
            "actions-rust-lang/setup-rust-toolchain@v1",
            "go-version: ${{ steps.go-version.outputs.version }}",
            "version=\"$(scripts/jig info go-version)\"",
            "cache-dependency-path: |\n            go.mod\n            go.sum\n            go.work\n            go.work.sum\n            **/go.mod",
        ],
    );
    assert_contains_none(
        &workflow,
        &[
            "go-version-file: .go-version",
            "go-version-file: \".go-version\"",
        ],
    );
    let expected_root_filters = usize::from(workflow_name == "go-tests.yml") * 2;
    assert_contains_count(&workflow, &[(r#"- "**""#, expected_root_filters)]);
    if workflow_name == "repo-policy.yml" {
        assert_contains_all(&workflow, &["JIG_PUSH_BEFORE: ${{ github.event.before }}"]);
    }
    assert_quoted_workflow_path_counts(
        &workflow,
        &[
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
            "sqlc.yaml",
            "**/sqlc.yaml",
            "internal/database/migrations/**",
            "**/*.sql",
        ],
        expected_root_filters,
    );
}

fn assert_go_test_workflow(destination: &Path) {
    let go_tests = fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&go_tests).unwrap();
    assert_eq!(parsed["jobs"]["checks"]["defaults"]["run"]["shell"], "bash");
    assert_contains_count(
        &go_tests,
        &[
            (r#"- "openapi/**""#, 2),
            (r#"- "scripts/test-postgres.sh""#, 2),
            ("runs-on: \"macos-14\"", 1),
            ("runs-on: \"ubuntu-latest\"", 1),
            ("actions-rust-lang/setup-rust-toolchain@v1", 2),
        ],
    );
    for target in ["api:fmt", "api:lint", "api:test-locked", "api:sqlc"] {
        assert_contains_all(&go_tests, &[&format!("scripts/jig check {target}")]);
    }
    assert_quoted_workflow_path_counts(
        &go_tests,
        &[
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
            "scripts/jig",
            "scripts/install-jig.sh",
        ],
        2,
    );
    let postgres_job = &go_tests[go_tests.find("postgres-integration:").unwrap()..];
    assert_text_before(
        postgres_job,
        "actions-rust-lang/setup-rust-toolchain@v1",
        "name: Resolve Go toolchain version",
    );
    assert_contains_all(&go_tests, &["run: bash scripts/test-postgres.sh"]);
    assert_paths_absent(destination, &[".go-version"]);
}

fn assert_go_repository_contract(destination: &Path) {
    let config = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert_contains_all(
        &config,
        &[
            r#"migration_dir = "internal/database/migrations""#,
            "[repository]",
            r#"affected_ignore = [".env", ".env.*", "**/.env", "**/.env.*", "README.md", "**/README.md", "AGENTS.md", "**/AGENTS.md", "agent-map.md", "CHANGELOG.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md", "docs/**", "LICENSE", "LICENSE.*", ".github/**"]"#,
            "api_test_command = \"go test ./...\"",
            "web_test_command = \"scripts/check-webapps.sh check-one",
            "action = \"frontend-contract-drift\"",
            "action = \"frontend-public-boundary\"",
            "contracts-drift-check",
            "contracts-boundary-check",
        ],
    );
    assert_contains_none(
        &config,
        &[
            "rust_migration_dir =",
            "backend_language =",
            "go_database =",
        ],
    );
    let config_value = toml::from_str::<toml::Value>(&config).unwrap();
    assert_eq!(
        config_value["work"]["gates"][0]["profile"].as_str(),
        Some("verify")
    );

    let contract = fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap();
    assert_contains_all(&contract, &[r#""name": "jig.migration_add""#]);
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
        assert!(
            contract_value["components"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| candidate["id"] == component)
        );
        assert!(
            contract_value["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action["target"]["component"] == component
                    && action["target"]["action"] == "test")
        );
    }
    for action in ["frontend-contract-drift", "frontend-public-boundary"] {
        assert!(
            contract_value["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| candidate["target"]["component"] == "repo"
                    && candidate["target"]["action"] == action)
        );
    }
}

fn assert_go_generated_runtime_files(destination: &Path) {
    let root_guide = fs::read_to_string(destination.join("AGENTS.md")).unwrap();
    assert_contains_all(
        &root_guide,
        &[
            "business logic in the owning package",
            "## Backend Guide Conventions",
            "scripts/jig migration add NAME",
        ],
    );
    assert_contains_none(&root_guide, &["business logic in the owning crate"]);
    assert_contains_none(
        &fs::read_to_string(destination.join("go.mod")).unwrap(),
        &["github.com/pressly/goose/v3/cmd/goose"],
    );
    let context = crate::context::RepoContext::load_from(destination).unwrap();
    assert_eq!(
        crate::doctor::go_version_selector(&context).unwrap(),
        "1.26.0"
    );
    assert_contains_all(
        &fs::read_to_string(destination.join("internal/httpapi/httpapi.go")).unwrap(),
        &["config.CreateHooks = nil"],
    );
    assert_contains_all(
        &fs::read_to_string(destination.join("internal/httpapi/httpapi_test.go")).unwrap(),
        &["want field omitted when schema routes are disabled"],
    );
    assert_contains_none(
        &fs::read_to_string(destination.join("openapi/public.json")).unwrap(),
        &["\"$schema\""],
    );
    assert_contains_none(
        &fs::read_to_string(
            destination.join("packages/public-api-client/src/generated/types.gen.ts"),
        )
        .unwrap(),
        &["$schema"],
    );
    let postgres_script = fs::read_to_string(destination.join("scripts/test-postgres.sh")).unwrap();
    assert_contains_all(
        &postgres_script,
        &[
            "attempt=$((attempt + 1))",
            "PostgreSQL container did not become queryable",
        ],
    );
    assert_contains_none(&postgres_script, &["seq 1 60"]);
    let policy: serde_json::Value = serde_yaml_ng::from_str(
        &fs::read_to_string(destination.join(".github/workflows/repo-policy.yml")).unwrap(),
    )
    .unwrap();
    assert!(policy["jobs"]["migration-immutability"].is_object());
    assert!(policy["jobs"]["sqlx-unchecked-queries"].is_null());
}

#[cfg(unix)]
fn assert_rust_only_command_output_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
    assert_contains_count(
        &workflow,
        &[
            (r#"- "**""#, 2),
            (r#"- "openapi/**""#, 2),
            (r#"- "packages/public-api-client/**""#, 2),
        ],
    );
    assert_contains_all(&workflow, &["node scripts/contracts.mjs client-check"]);
    assert_contains_none(&workflow, &["if [ -f scripts/contracts.mjs ]"]);
    assert_text_before(
        &workflow,
        "Run build",
        "Check generated API clients and public boundary",
    );

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        assert_go_adapter_workflow(&destination, workflow_name);
    }
    assert_go_test_workflow(&destination);

    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert_contains_all(
        &gitignore,
        &[".contract-stage-*/", ".contract-client-stage-*/"],
    );

    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert_contains_all(
        &browser_e2e,
        &[
            "version=\"$(scripts/jig info go-version)\"",
            "go-version: ${{ steps.go-version.outputs.version }}",
            "cache-dependency-path: |",
        ],
    );
    assert_go_repository_contract(&destination);
    assert_go_generated_runtime_files(&destination);

    let config_path = destination.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        r#"migration_dir = "internal/database/migrations""#,
        r#"migration_dir = "database/migrations""#,
    );
    fs::write(&config_path, config).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        assert_contains_count(
            &workflow,
            &[(
                r#"- "database/migrations/**""#,
                usize::from(workflow_name == "go-tests.yml") * 2,
            )],
        );
        assert_contains_none(&workflow, &[r#"- "internal/database/migrations/**""#]);
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
        let path = std::env::join_paths(std::iter::once(fake_bin).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();
        let before = regular_file_tree_snapshot(&destination);

        let output = Command::new("node")
            .args(["scripts/contracts.mjs", "client-check"])
            .current_dir(&destination)
            .env("PATH", path)
            .env("JIG_TEST_GO_MARKER", &backend_marker)
            .output()
            .unwrap();

        assert_rust_only_command_output_success("client-check", &output);
        assert_paths_absent(&destination, &[".backend-exporter-ran"]);
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
    let locked_test = config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|action| {
            action["target"]["component"].as_str() == Some("api")
                && action["target"]["action"].as_str() == Some("test-locked")
        })
        .unwrap();
    locked_test["inputs"]
        .as_array_mut()
        .unwrap()
        .push(toml::Value::String("shared/proto/**".into()));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();

    for workflow_name in ["go-tests.yml", "repo-policy.yml"] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        assert_contains_all(&workflow, &["version=\"$(scripts/jig info go-version)\""]);
        assert_contains_none(&workflow, &["go-version-file: go.mod", r#"- "**""#]);
        let expected_path_filters = usize::from(workflow_name == "go-tests.yml") * 2;
        assert_contains_count(
            &workflow,
            &[
                (r#"- "services/api/**""#, expected_path_filters),
                (r#"- "shared/proto/**""#, expected_path_filters),
            ],
        );
    }
    let context = crate::context::RepoContext::load_from(&destination).unwrap();
    assert_eq!(
        crate::doctor::go_version_selector(&context).unwrap(),
        "1.26.0"
    );
    let browser_e2e = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert_contains_all(
        &browser_e2e,
        &["version=\"$(scripts/jig info go-version)\""],
    );
    assert_contains_none(&browser_e2e, &["go-version-file: go.mod"]);

    fs::remove_file(destination.join("scripts/test-postgres.sh")).unwrap();
    run_update(update_opts(&destination, template.path(), true)).unwrap();
    let go_tests = fs::read_to_string(destination.join(".github/workflows/go-tests.yml")).unwrap();
    assert_contains_all(&go_tests, &["scripts/jig check api:sqlc"]);
    assert_contains_none(&go_tests, &["postgres-integration:"]);
}
