use super::*;

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
            metrics: None,
            jobs: None,
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
        rendered_contents(&rendered, "apps/web/playwright.config.ts"),
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
            metrics: None,
            jobs: None,
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

pub(super) fn assert_go_repository_contract(destination: &Path) {
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
    assert!(config_value.get("work").is_none());

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

pub(super) fn assert_go_generated_runtime_files(destination: &Path) {
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
    let context = jig_context::RepoContext::load_from(destination).unwrap();
    assert_go_module_authority_declares(&context, "1.26.0");
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

/// The repository's resolved Go module authority declares exactly `version`.
pub(super) fn assert_go_module_authority_declares(
    context: &jig_context::RepoContext,
    version: &str,
) {
    let authority = context.go_module_authority_paths().unwrap();
    assert_eq!(authority.len(), 1, "{authority:?}");
    let go_mod = fs::read_to_string(&authority[0]).unwrap();
    let directive = format!("go {version}");
    assert!(
        go_mod.lines().any(|line| line.trim() == directive),
        "{go_mod}"
    );
}

fn assert_nested_go_defaults(plan: &scaffold::InitScaffoldPlan) {
    let mut defaults = AnswerOpts::default();
    plan.apply_answer_defaults(&mut defaults);
    assert_eq!(
        defaults.migration_dir.as_deref(),
        Some("services/api/internal/database/migrations")
    );
    assert_eq!(defaults.dev_apps[0].dir.as_deref(), Some("services/api"));
    let bootstrap = defaults.bootstrap_command.unwrap();
    assert_contains_none(&bootstrap, &["DATABASE_URL", "--bootstrap-database"]);
    assert_contains_all(
        &bootstrap,
        &[
            "(cd services/api && go mod tidy)",
            "(cd services/api && go tool sqlc generate)",
        ],
    );
}

#[test]
fn go_browser_scaffold_honors_the_authored_backend_root() {
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
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            go_module: Some("example.com/demo".into()),
            scaffold_go_component_roots: vec!["services/api".into()],
            migration_dir: Some("services/api/internal/database/migrations".into()),
            ..AnswerOpts::default()
        },
        planning_root.path(),
    )
    .unwrap()
    .unwrap();

    let rendered = plan.render_files().unwrap();
    let contents = |path: &str| {
        rendered
            .iter()
            .find(|file| file.relative == path)
            .unwrap_or_else(|| panic!("missing rendered {path}"))
            .contents
            .as_str()
    };
    assert_rendered_paths(
        &rendered,
        &[
            "services/api/.env.example",
            "services/api/go.mod",
            "services/api/cmd/api/main.go",
            "services/api/cmd/openapi/main.go",
            "services/api/sqlc.yaml",
            "services/api/internal/database/database.go",
        ],
    );
    assert_rendered_paths_absent(
        &rendered,
        &[".env.example", "go.mod", "cmd/api/main.go", "sqlc.yaml"],
    );
    let output_paths = plan.output_paths();
    assert!(
        output_paths
            .iter()
            .any(|path| path == Path::new("services/api/go.mod"))
    );
    assert!(output_paths.iter().all(|path| path != Path::new("go.mod")));
    assert!(
        rendered
            .iter()
            .any(|file| file.relative == "openapi/public.json")
    );
    let postgres_script = contents("scripts/test-postgres.sh");
    assert_contains_all(postgres_script, &[r#"go -C "services/api" test -count=1"#]);
    let httpapi_test = contents("services/api/internal/httpapi/httpapi_test.go");
    assert!(httpapi_test.contains(r#"filepath.FromSlash("../../../../openapi/public.json")"#));
    let workflow = contents(".github/workflows/e2e.yml");
    assert_eq!(workflow.matches(r#"- "services/api/**""#).count(), 2);
    assert_eq!(
        workflow
            .matches(r#"- "services/api/internal/database/migrations/**""#)
            .count(),
        2
    );
    assert_contains_none(
        workflow,
        &[r#"- "cmd/**""#, r#"- "internal/**""#, r#"- "**""#],
    );

    let playwright = contents("apps/web/playwright.config.ts");
    assert_contains_all(
        playwright,
        &[
            r#"path.resolve(repoRoot, "services/api")"#,
            "cwd: backendRoot",
        ],
    );
    let contracts = contents("scripts/contracts.mjs");
    assert_contains_all(
        contracts,
        &[
            r#"resolve(repoRoot, "services/api")"#,
            r#"join(backendRoot, "go.mod")"#,
            r#"run("go", ["run", "./cmd/openapi", "--output", document], backendRoot)"#,
        ],
    );
    assert_nested_go_defaults(&plan);
}

#[test]
fn go_react_rejects_missing_module_and_admin() {
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

    let admin = ScaffoldOpts {
        preset: Some(ScaffoldPreset::GoReact),
        db: Some(ScaffoldDb::None),
        frontends: vec![ScaffoldFrontend {
            name: "admin".into(),
            kind: ScaffoldFrontendKind::Admin,
            custom_default_name: false,
        }],
        frontend_list: Vec::new(),
        metrics: None,
        jobs: None,
    }
    .validate_init_invariants(&AnswerOpts {
        go_module: Some("example.com/demo".into()),
        ..AnswerOpts::default()
    })
    .unwrap_err()
    .to_string();
    assert!(admin.contains("separate privileged API and client boundary"));
}
