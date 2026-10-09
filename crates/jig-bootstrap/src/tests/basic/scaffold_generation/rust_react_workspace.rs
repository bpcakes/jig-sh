use super::*;

fn assert_rust_react_guidance_and_policy(destination: &Path, output: &serde_json::Value) {
    let next_steps = output["next_steps"].as_array().unwrap();
    let database_config = next_steps
        .iter()
        .position(|step| {
            step.as_str()
                .is_some_and(|step| step.contains("Export DATABASE_URL"))
        })
        .unwrap();
    let setup = next_steps
        .iter()
        .position(|step| step.as_str() == Some("scripts/jig setup"))
        .unwrap();
    let database_setup = next_steps
        .iter()
        .position(|step| step.as_str() == Some("bash scripts/setup-database.sh"))
        .unwrap();
    assert!(setup < database_config);
    assert!(database_config < database_setup);
    let context = jig_context::RepoContext::load_from(destination).unwrap();
    let agent_map_check = jig_policy::run_check(
        &context,
        jig_policy::PolicyCheckCommand::AgentMap(jig_policy::AgentMapInput {
            map_path: PathBuf::from("agent-map.md"),
        }),
    )
    .unwrap();
    assert_eq!(agent_map_check["ok"], true);
    assert_eq!(agent_map_check["agents"], 8);
    assert!(
        agent_map_check["missing_agents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        agent_map_check["broken_links"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let agent_guides_check =
        jig_policy::run_check(&context, jig_policy::PolicyCheckCommand::AgentGuides).unwrap();
    assert_eq!(agent_guides_check["ok"], true);
    assert_eq!(agent_guides_check["guide_count"], 8);
    assert!(
        agent_guides_check["missing_entry_ref"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

fn assert_rust_react_report_and_paths(destination: &Path, output: &serde_json::Value) {
    assert_eq!(output["scaffold"]["frontends"][0]["dir"], "apps/web");
    assert!(!destination.join("web").exists());
    assert_eq!(output["scaffold"]["preset"], "rust-react");
    assert_eq!(output["scaffold"]["db"], "postgres");
    assert_eq!(output["scaffold"]["frontends"][0]["role"], "spa");
    assert_eq!(
        output["scaffold"]["frontends"][0]["ui"]["style"],
        "radix-nova"
    );
    assert_eq!(output["scaffold"]["frontends"][2]["role"], "admin");
    assert_eq!(
        output["scaffold"]["frontends"][2]["ui"]["cli_version"],
        "4.18.0"
    );
    assert_generated_rust_clippy_defaults(destination);
    assert_paths_exist(
        destination,
        &[
            ".env.example",
            "apps/my-app-api/src/main.rs",
            "crates/my-app-core/src/lib.rs",
            "crates/my-app/src/lib.rs",
            "crates/my-app/AGENTS.md",
            "crates/my-app-http/src/lib.rs",
            "crates/my-app-http/src/public.rs",
            "crates/my-app-http-common/src/lib.rs",
            "crates/my-app-admin-http/src/lib.rs",
            "apps/my-app-admin-api/src/main.rs",
            "crates/my-app-http/AGENTS.md",
            "apps/my-app-api/src/bin/export-openapi.rs",
            "openapi/public.json",
            "openapi/admin.json",
            "README.md",
            "scripts/test-postgres.sh",
            "crates/my-app-test-support/tests/postgres.rs",
            "crates/my-app-db/src/lib.rs",
            "crates/my-app-db/AGENTS.md",
            "crates/my-app-test-support/src/lib.rs",
            "crates/my-app-test-support/AGENTS.md",
            "crates/my-app-test-support/src/app.rs",
            "crates/my-app-test-support/src/http.rs",
            "crates/my-app-test-support/src/responses.rs",
            "crates/my-app-test-support/src/db.rs",
            "crates/my-app-test-support/tests/http.rs",
            "apps/web/package.json",
        ],
    );
}

fn assert_workspace_and_contract_tooling(destination: &Path) {
    let web_gitignore = fs::read_to_string(destination.join("apps/web/.gitignore")).unwrap();
    assert_contains_all(
        &web_gitignore,
        &[
            "playwright-report/",
            "test-results/",
            "blob-report/",
            "*.tsbuildinfo",
        ],
    );
    assert_paths_exist(
        destination,
        &[
            "apps/landing/astro.config.mjs",
            "apps/admin-panel/package.json",
        ],
    );
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    let workspace_package_json: serde_json::Value =
        serde_json::from_str(&workspace_package).unwrap();
    assert_eq!(
        workspace_package_json["workspaces"],
        serde_json::json!([
            "apps/web",
            "apps/landing",
            "apps/admin-panel",
            "packages/public-api-client",
            "packages/admin-api-client"
        ])
    );
    let expected_node_engine = format!(">={GENERATED_NODE_VERSION}");
    assert_contains_all(
        &workspace_package,
        &[
            r#""packageManager": "bun@1.3.14""#,
            r#""apps/admin-panel""#,
            r#""api:generate""#,
            r#""api:check""#,
            r#""contract:generate""#,
            r#""contract:check""#,
            r#""contract:client-check""#,
            r#""public:artifacts:check""#,
            r#""packages/public-api-client""#,
            r#""packages/admin-api-client""#,
        ],
    );
    assert_eq!(
        workspace_package_json["engines"]["node"].as_str(),
        Some(expected_node_engine.as_str())
    );
    assert_eq!(
        workspace_package_json["scripts"]["bootstrap"],
        "bash scripts/jig bootstrap"
    );
    assert_eq!(
        workspace_package_json["scripts"]["test:postgres"],
        "bash scripts/test-postgres.sh"
    );
    assert_eq!(workspace_package_json["overrides"]["js-yaml"], "4.3.1");
    let shared_eslint = fs::read_to_string(destination.join("eslint.config.shared.mjs")).unwrap();
    assert_contains_all(
        &shared_eslint,
        &[
            "tseslint.configs.recommendedTypeChecked",
            "reactHooks.configs.flat.recommended",
            "testingLibrary.configs[\"flat/react\"]",
            "vitest.configs.recommended",
            "reportUnusedDisableDirectives: \"error\"",
            "reportUnusedInlineConfigs: \"error\"",
            "src/components/**/*.{ts,tsx}",
            "src/domain/**/*.{ts,tsx}",
        ],
    );
    let contracts_script = fs::read_to_string(destination.join("scripts/contracts.mjs")).unwrap();
    assert_contains_all(
        &contracts_script,
        &[
            "await withStagedContracts(mode)",
            "await withStagedClients()",
            "generateClient(resolve(contract.document), generated)",
            "async function publishAtomically(",
            "async function assertPublicBoundary(",
            r#"["tree", "--quiet", "-p", "my-app-api""#,
            r#"cargoPackage: "my-app-api""#,
            r#"cargoPackage: "my-app-admin-api""#,
            "Contract recovery data was preserved",
        ],
    );
    assert_eq!(
        fs::read_to_string(destination.join(".node-version")).unwrap(),
        format!("{GENERATED_NODE_VERSION}\n")
    );
}

fn assert_generated_ci_workflows(destination: &Path) {
    let rust_workflow =
        fs::read_to_string(destination.join(".github/workflows/rust-tests.yml")).unwrap();
    let rust_workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&rust_workflow).unwrap();
    for job in ["fmt", "clippy", "test"] {
        assert_eq!(rust_workflow_yaml["jobs"][job]["runs-on"], "macos-14");
    }
    for event in ["pull_request", "push"] {
        let paths = rust_workflow_yaml["on"][event]["paths"].as_array().unwrap();
        assert!(
            paths.iter().any(|path| path == "**"),
            "Rust CI must derive its root component input from repository authority"
        );
        assert!(paths.iter().any(|path| path == "migrations/**"));
        assert!(paths.iter().any(|path| path == ".sqlx/**"));
    }
    assert_eq!(
        rust_workflow_yaml["jobs"]["clippy"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert_eq!(
        rust_workflow_yaml["jobs"]["test"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(rust_workflow_yaml["jobs"]["fmt"]["env"].is_null());
    for (workflow_name, jobs) in [
        ("agent-map-check.yml", &["agent-map-check"][..]),
        (
            "repo-policy.yml",
            &[
                "file-budget",
                "sqlx-unchecked-queries",
                "migration-immutability",
            ][..],
        ),
    ] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
        for event in ["pull_request", "push"] {
            if workflow_name == "repo-policy.yml" {
                assert!(
                    workflow["on"][event]["paths"].is_null(),
                    "repository policy must not hide source or policy changes behind path filters"
                );
            } else {
                assert!(
                    workflow["on"][event]["paths"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|path| path == "**"),
                    "{workflow_name} must derive Rust component paths from repository authority"
                );
            }
        }
        for job in jobs {
            assert_eq!(workflow["jobs"][job]["runs-on"], "macos-14");
        }
    }
}

fn assert_agent_map(destination: &Path) {
    let agent_map = fs::read_to_string(destination.join("agent-map.md")).unwrap();
    for guide in [
        "crates/my-app/AGENTS.md",
        "crates/my-app-db/AGENTS.md",
        "crates/my-app-http/AGENTS.md",
        "crates/my-app-test-support/AGENTS.md",
    ] {
        assert!(agent_map.contains(guide), "agent map is missing {guide}");
    }
}

fn assert_api_entrypoint(destination: &Path) {
    let api_main = fs::read_to_string(destination.join("apps/my-app-api/src/main.rs")).unwrap();
    assert_contains_all(
        &api_main,
        &[
            "use anyhow::Context;",
            "use ::my_app as app_crate;",
            "use ::my_app_http as app_http_crate;",
            "load_dotenv();",
            "warning: failed to load .env",
            "runtime::serve(config, app_http_crate::assemble).await",
            "app_crate::AppConfig::from_env()",
            "--bootstrap-database",
            "    let command = parse_command()?;\n    let config = app_crate::AppConfig::from_env()",
            "match (arguments.next(), arguments.next())",
            "unexpected API argument",
            "runtime::bootstrap_database(config)",
            "install_panic_hook",
            "tracing::error!(error = ?error, \"API server failed\")",
            "#[allow(clippy::useless_concat)]\n    let default_filter",
            "let default_filter = concat!(",
            "\"my_app=info,\",",
            "\"my_app_api=info,\",",
            "\"batter=info,batter_axum=info\",",
        ],
    );
    assert_contains_none(
        &api_main,
        &[
            "args_os().any",
            "unsafe {",
            "std::env::set_var",
            "std::env::remove_var",
            "serve_with_jobs",
            "runledger",
        ],
    );
    let runtime = fs::read_to_string(destination.join("crates/my-app-runtime/src/lib.rs")).unwrap();
    assert_contains_all(
        &runtime,
        &[
            "Startup::scoped",
            ".with_unix_signals(\"signals\")",
            "let completion = service::start(startup, service::NoDiagnostics).wait().await;",
            "ServiceOutcome::StartupFailed(error) => Err(anyhow::Error::new(error.clone()))",
            "ServiceOutcome::Shutdown(Err(error)) => Err(anyhow::Error::new(error.clone()))",
            "let supervisor = Supervisor::new(shutdown_budget()?);",
            "let lifecycle = supervisor.status();",
            "let admission = supervisor.operation_admission();",
            "OperationOwner::new(STARTUP_BUDGET)?.into_context()",
            "let initialization = context.clone();",
            "ProtectedStartupScope",
            "application.register_in(scope, \"http\", listener)?",
            "reserve_cleanup(\"database.close\")",
            "db.migrate(context).await?",
            "let health = db.register_health(scope.registration())?;",
            "ReadinessPolicy::new(self.lifecycle, health)",
            "ReadinessPolicy::lifecycle_only(self.lifecycle)",
            "pub type Readiness = ReadinessPolicy<DependencyError>;",
            "let database_context = scope.context().clone();",
            "&database_context",
        ],
    );
    assert_contains_none(
        &runtime,
        &[
            "Startup::new",
            "scope.supervisor()",
            "install_signals",
            "signals.received()",
            "low_level",
            "register_http_in",
            "check_shutdown",
            "OperationContext::new",
            "axum::Router",
            "jobs_crate",
            "metrics::",
        ],
    );
}

fn assert_generated_dev_config(destination: &Path) {
    let jig_toml = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(jig_toml.contains("[[dev.apps]]\nname = \"api\""));
    assert!(jig_toml.contains("kind = \"env-port\""));
    assert!(!jig_toml.contains("proxy = false"));
    assert!(jig_toml.contains("argv = [\"cargo\", \"run\", \"-p\", \"my-app-api\"]"));
    assert!(jig_toml.contains("[[dev.apps]]\nname = \"admin-api\""));
    assert!(jig_toml.contains("argv = [\"cargo\", \"run\", \"-p\", \"my-app-admin-api\"]"));
    assert!(!jig_toml.contains("BIND_ADDR=\"${HOST}:${PORT}\""));
    assert!(!jig_toml.contains("port = 3000"));
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=my_app=info,my_app_api=info,my_app_admin_api=info,batter=info,batter_axum=info\nDATABASE_URL=postgres://postgres:postgres@localhost:5432/my_app_dev\n"
    );
}

fn assert_api_entrypoint_and_dev_config(destination: &Path) {
    assert_api_entrypoint(destination);
    assert_generated_dev_config(destination);
}

pub(super) fn assert_workspace_and_binary_manifests(destination: &Path) {
    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("rust-version = \"1.94\""));
    assert!(workspace_cargo.contains("sqlx = { version = \"0.9\""));
    assert!(!workspace_cargo.contains("sqlx = { version = \"0.8\""));
    assert!(workspace_cargo.contains("dotenvy = \"0.15\""));
    assert!(workspace_cargo.contains(
        "batter = { git = \"https://github.com/bpcakes/batter\", rev = \"18cdf97ac544c665e0189efd28388d1e10456232\", features = [\"axum\", \"sqlx\"] }"
    ));
    assert!(workspace_cargo.contains(
        "postgres-test-harness = { git = \"https://github.com/bpcakes/postgres-test-harness.git\", rev = \"3d525e6fc5745ce2e2437c7997de5cccdecff4ac\", default-features = false }"
    ));
    assert!(!workspace_cargo.contains("batter-axum ="));
    assert!(!workspace_cargo.contains("batter-sqlx ="));
    assert!(!workspace_cargo.contains("\"otlp\""));
    assert!(!workspace_cargo.contains("\"runledger\""));
    assert!(!workspace_cargo.contains("my-app-jobs"));
    assert!(workspace_cargo.contains(r#""apps/my-app-admin-api""#));
    assert!(workspace_cargo.contains(r#""crates/my-app-admin-http""#));
    assert!(workspace_cargo.contains(r#""crates/my-app-http-common""#));
    let api_cargo = fs::read_to_string(destination.join("apps/my-app-api/Cargo.toml")).unwrap();
    assert!(api_cargo.contains("dotenvy.workspace = true"));
    assert!(!api_cargo.contains("my-app-admin-http"));
    let admin_api_cargo =
        fs::read_to_string(destination.join("apps/my-app-admin-api/Cargo.toml")).unwrap();
    assert!(admin_api_cargo.contains("my-app-admin-http"));
    assert!(!admin_api_cargo.contains("my-app-http ="));
}

pub(super) fn assert_generated_backend_docs(destination: &Path) {
    let root_readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert_contains_all(
        &root_readme,
        &[
            "Prerequisites: Rust 1.94 or newer",
            "bun run bootstrap",
            "do not start with `bun install --frozen-lockfile`",
            "Commit the generated `bun.lock`",
            "DenyAllAdminAuthorizer",
            "`assemble` from `crates/my-app-admin-http`",
            "x-admin-request: 1",
            "bun run test:postgres",
            "POSTGRES_TEST_ADMIN_URL",
            "`batter` facade",
            "`axum`, `sqlx`.",
            "upgrade both pins together",
            "upgrade the Batter revision",
            "batter::service::start",
            "Startup::scoped",
            "sealed `HttpBoundary`",
            "`AdmittedRequest`",
            "supervised PostgreSQL health\nmonitor",
            "local closure does not",
        ],
    );
    assert_contains_none(
        &root_readme,
        &[
            "router_with_lifecycle",
            "test_db_",
            "not a continuous database connectivity check",
            "### Metrics export",
            "### Background jobs",
        ],
    );
    let http_agents = fs::read_to_string(destination.join("crates/my-app-http/AGENTS.md")).unwrap();
    assert!(http_agents.contains("`src/public.rs`: owns public routes"));
    assert!(http_agents.contains("Never depend on `my-app-admin-http`"));
    assert!(http_agents.contains("`HttpBoundary` owns the layer order"));
    let app_agents = fs::read_to_string(destination.join("crates/my-app/AGENTS.md")).unwrap();
    assert!(app_agents.contains("Capture the environment once at startup"));
    let runtime_agents =
        fs::read_to_string(destination.join("crates/my-app-runtime/AGENTS.md")).unwrap();
    assert!(runtime_agents.contains("batter::service::start"));
    assert!(!runtime_agents.contains("register_http_in"));
}

fn assert_rendered_jig_answers(destination: &Path) {
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert_contains_all(
        &answers,
        &[
            "repo_name = \"my-app\"",
            "sqlx_enabled = true",
            "rust_migration_dir = \"migrations\"",
            "rust_sqlx_metadata_dir = \".sqlx\"",
            "schema_dump_enabled = false",
            "rust_crate_roots = [\"apps\", \"crates\"]",
            "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -D clippy::mod_module_files",
            "web_package_manager = \"bun\"",
            "if [ -f Cargo.toml ]; then cargo fetch;",
            "name = \"web\"",
            "dir = \"apps/landing\"",
            "kind = \"env-port\"",
            "name = \"admin-panel\"",
            "role = \"spa\"",
            "role = \"astro\"",
            "role = \"admin\"",
        ],
    );
    let config: toml::Value = toml::from_str(&answers).unwrap();
    let bootstrap = config["commands"]["repo_bootstrap_command"]
        .as_str()
        .unwrap();
    assert!(bootstrap.contains("scripts/check-webapps.sh bootstrap"));
    assert_contains_none(bootstrap, &["DATABASE_URL", "--bootstrap-database"]);
    let database_setup = fs::read_to_string(destination.join("scripts/setup-database.sh")).unwrap();
    assert_contains_all(
        &database_setup,
        &[
            "Missing DATABASE_URL",
            "before database setup",
            "cargo run -p my-app-api -- --bootstrap-database",
        ],
    );
    assert_contains_none(&answers, &["(cd web && bun install)"]);
}

#[test]
fn run_init_rust_react_scaffold_generates_backend_and_frontends() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("my-app");
    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
            metrics: None,
            jobs: None,
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            ci_github_runner: Some("macos-14".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    assert_rust_react_guidance_and_policy(&destination, &output);
    assert_rust_react_report_and_paths(&destination, &output);
    assert_workspace_and_contract_tooling(&destination);
    assert_public_spa_package_and_clients(&destination);
    assert_public_spa_source_and_vite(&destination);
    assert_public_spa_e2e(&destination);
    assert_generated_ci_workflows(&destination);
    assert_landing_and_admin_tooling(&destination);
    assert_admin_theme_and_components(&destination);
    assert_admin_data_and_routes(&destination);
    assert_agent_map(&destination);
    assert_api_entrypoint_and_dev_config(&destination);
    assert_workspace_and_backend_crates(&destination);
    assert_http_contract_and_test_support(&destination);
    assert_database_support_and_docs(&destination);
    assert_rendered_jig_answers(&destination);
}

#[cfg(unix)]
#[test]
fn generated_dependency_failure_names_the_exact_bootstrap_recovery_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("dependency-recovery");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
            metrics: None,
            jobs: None,
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let package_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(destination.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_json["packageManager"], "npm@12.0.2");
    assert_eq!(package_json["allowScripts"]["esbuild@0.28.2"], true);

    let output = Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-install",
            "apps/web",
        ])
        .current_dir(&destination)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing package-lock.json"), "{stderr}");
    assert!(
        stderr.contains("Run 'scripts/check-webapps.sh bootstrap' from the repository root"),
        "{stderr}"
    );
}

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
            metrics: None,
            jobs: None,
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
    let vite = fs::read_to_string(destination.join("apps/web/vite.config.ts")).unwrap();
    assert!(vite.contains("http://api.exampleproject.example.test:2455"));
    assert!(!vite.contains("localhost:1355"));
}
