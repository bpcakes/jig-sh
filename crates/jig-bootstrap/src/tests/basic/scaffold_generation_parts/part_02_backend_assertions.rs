fn assert_admin_theme_contract(destination: &Path) {
    let admin_index = fs::read_to_string(destination.join("apps/admin-panel/index.html")).unwrap();
    let theme_storage_key = "admin-panel-theme";
    let theme_bootstrap = admin_index
        .find(&format!("const themeStorageKey = \"{theme_storage_key}\""))
        .unwrap();
    let react_entry = admin_index.find("/src/main.tsx").unwrap();
    assert!(theme_bootstrap < react_entry);
    assert_eq!(admin_index.matches(theme_storage_key).count(), 1);
    assert_contains_all(
        &admin_index,
        &[
            "localStorage.getItem(themeStorageKey)",
            "<!-- prettier-ignore -->\n    <title>Admin Panel</title>",
            "prefers-color-scheme: dark",
            "root.style.colorScheme = resolved",
        ],
    );
    let theme_provider =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/theme-provider.tsx"))
            .unwrap();
    assert_contains_all(
        &theme_provider,
        &[
            "storage = window.localStorage",
            "if (event.storageArea !== storage)",
        ],
    );
    let providers =
        fs::read_to_string(destination.join("apps/admin-panel/src/app/providers.tsx")).unwrap();
    assert!(providers.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(providers.matches(theme_storage_key).count(), 1);
    assert_contains_all(
        &providers,
        &[
            "storageKey={themeStorageKey}",
            "<QueryClientProvider client={client}>",
        ],
    );
    let admin_router =
        fs::read_to_string(destination.join("apps/admin-panel/src/app/router.ts")).unwrap();
    assert_contains_all(
        &admin_router,
        &[
            "import { routeTree } from \"@/routeTree.gen\"",
            "export function createAppRouter(",
            "context: { queryClient }",
            "defaultPreloadStaleTime: 0",
            r#"declare module "@tanstack/react-router""#,
        ],
    );
    let admin_shell =
        fs::read_to_string(destination.join("apps/admin-panel/src/app/shell.tsx")).unwrap();
    assert_contains_all(
        &admin_shell,
        &[
            r#"from "@tanstack/react-router""#,
            "const appTitle = \"Admin Panel\"",
            ">{appTitle}</p>",
        ],
    );
    let admin_sidebar =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/app-sidebar.tsx")).unwrap();
    assert_contains_all(
        &admin_sidebar,
        &[
            "const appName = \"my-app\"",
            ">{appName}</span>",
            r#"from "@tanstack/react-router""#,
            "useRouterState({",
        ],
    );
    assert_eq!(admin_sidebar.matches("\"my-app\"").count(), 1);
    let admin_overview_test = fs::read_to_string(
        destination.join("apps/admin-panel/src/features/overview/overview-page.test.tsx"),
    )
    .unwrap();
    assert_contains_all(
        &admin_overview_test,
        &[
            "const expectedAppName = \"my-app\"",
            "name: expectedAppName",
            "screen.findAllByText(expectedAppName)",
        ],
    );
    assert_eq!(admin_overview_test.matches("\"my-app\"").count(), 1);
}

fn assert_admin_component_sources(destination: &Path) {
    let admin_prettierignore =
        fs::read_to_string(destination.join("apps/admin-panel/.prettierignore")).unwrap();
    assert_eq!(admin_prettierignore.matches("dist/\n").count(), 1);
    assert_eq!(admin_prettierignore.matches("pnpm-lock.yaml").count(), 1);
    assert_eq!(
        admin_prettierignore.matches("npm-shrinkwrap.json").count(),
        1
    );
    assert!(admin_prettierignore.contains("bun.lock\nbun.lockb\n"));
    assert!(admin_prettierignore.contains("src/routeTree.gen.ts"));
    let admin_empty =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/ui/empty.tsx")).unwrap();
    assert!(admin_empty.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_empty.contains("React.ComponentProps"));
    let admin_skeleton =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/ui/skeleton.tsx")).unwrap();
    assert!(admin_skeleton.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_skeleton.contains("React.ComponentProps"));
    let admin_sonner =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/ui/sonner.tsx")).unwrap();
    assert!(admin_sonner.contains(r#"import type { CSSProperties } from "react""#));
    assert!(!admin_sonner.contains("React.CSSProperties"));
    let components = fs::read_to_string(destination.join("apps/admin-panel/components.json")).unwrap();
    assert!(components.contains(r#""style": "radix-nova""#));
    assert!(
        destination
            .join("apps/admin-panel/src/components/ui/sidebar.tsx")
            .exists()
    );
    assert!(
        destination
            .join("apps/admin-panel/src/features/overview/overview-page.tsx")
            .exists()
    );
    assert!(destination.join("apps/admin-panel/src/lib/api.ts").exists());
}

fn assert_admin_theme_and_components(destination: &Path) {
    assert_admin_theme_contract(destination);
    assert_admin_component_sources(destination);
}

fn assert_admin_data_and_routes(destination: &Path) {
    let admin_api = fs::read_to_string(destination.join("apps/admin-panel/src/lib/api.ts")).unwrap();
    assert!(admin_api.contains("getAdminStatusOptions"));
    assert!(admin_api.contains("adminStatusQueryOptions"));
    assert!(
        destination
            .join("apps/admin-panel/src/lib/query-client.ts")
            .exists()
    );
    assert!(
        destination
            .join("apps/admin-panel/src/app/router-context.ts")
            .exists()
    );
    assert!(
        destination
            .join("apps/admin-panel/src/routes/__root.tsx")
            .exists()
    );
    assert!(
        destination
            .join("apps/admin-panel/src/routes/index.tsx")
            .exists()
    );
    assert!(
        destination
            .join("apps/admin-panel/src/routes/settings.tsx")
            .exists()
    );
    assert!(
        destination
            .join("apps/admin-panel/src/routeTree.gen.ts")
            .exists()
    );
    let admin_index_route =
        fs::read_to_string(destination.join("apps/admin-panel/src/routes/index.tsx")).unwrap();
    assert!(admin_index_route.contains(r#"createFileRoute("/")"#));
    assert!(admin_index_route.contains("context.queryClient.ensureQueryData"));
    let admin_query_client =
        fs::read_to_string(destination.join("apps/admin-panel/src/lib/query-client.ts")).unwrap();
    assert!(admin_query_client.contains("retry: 1"));
    let admin_overview =
        fs::read_to_string(destination.join("apps/admin-panel/src/features/overview/overview-page.tsx"))
            .unwrap();
    assert!(admin_overview.contains("useSuspenseQuery(appStatusQueryOptions)"));
    assert!(admin_overview.contains("useQueryErrorResetBoundary()"));
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

fn assert_workspace_and_binary_manifests(destination: &Path) {
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

fn assert_application_and_public_http_crates(destination: &Path) {
    let runtime = fs::read_to_string(destination.join("crates/my-app-runtime/src/lib.rs")).unwrap();
    assert_contains_all(
        &runtime,
        &[
            "pub async fn bootstrap_database(config: app_crate::AppConfig)",
            "use batter::command::{Command, check_command}",
            "let command = Command::new(",
            "scope.reserve_cleanup(\"database.close\")?",
            "scope.stage(\"database.migrate\")?",
            "command.cancel(); command.wait().await",
            "let report = check_command(outcome)?",
        ],
    );
    let app_config = fs::read_to_string(destination.join("crates/my-app/src/config.rs")).unwrap();
    assert_contains_all(
        &app_config,
        &[
            "pub struct AppConfig",
            "pub fn from_env() -> Result<Self, SettingsError>",
            "Self::from_source(&SettingsSource::from_pairs(std::env::vars_os())?)",
            "present(source.text(\"HOST\")?)",
            "present(source.text(\"PORT\")?)",
            "present(source.text(\"BIND_ADDR\")?)",
            "database_url: Some(SecretString::new(source.required(\"DATABASE_URL\")?)),",
            "pub fn database_url(&self) -> Option<&SecretString>",
            "fn resolve_bind_addr(",
            "injected_host_and_port_override_the_dotenv_bind_address",
            "partial_jig_bind_values_fall_back_to_bind_addr",
            "invalid_settings_name_the_field_without_echoing_the_value",
            "database_url_is_required_and_redacted",
        ],
    );
    assert_contains_none(
        &app_config,
        &["std::env::var(", "database_url: Option<String>", "METRICS_OTLP_ENDPOINT"],
    );
    let app_lib = fs::read_to_string(destination.join("crates/my-app/src/lib.rs")).unwrap();
    assert_contains_all(
        &app_lib,
        &[
            "mod config;",
            "pub use config::AppConfig;",
            "pub fn from_config(config: AppConfig) -> Self",
            "pub fn from_database(config: AppConfig, database: db::Db) -> Self",
            "pub fn new_with_version(version: impl Into<String>)",
            "pub fn version(&self) -> &AppVersion",
            "pub fn is_ready(&self) -> bool",
        ],
    );
    assert_contains_none(
        &app_lib,
        &[
            "return Ok(Self",
            "return self.db.is_some()",
            "use axum::",
            "pub fn router",
            "bootstrap_database",
            "batter::",
            "Db::connect(",
        ],
    );
    let http_lib = fs::read_to_string(destination.join("crates/my-app-http/src/lib.rs")).unwrap();
    assert_contains_all(
        &http_lib,
        &[
            "pub async fn assemble<E>(",
            "ReadinessCondition::new(\"application-state\")?",
            "let (router, document) = public::application_routes().split_for_parts();",
            "GuardedRouter::from_router(router, route_inventory(&document)?)",
            ".fallback(not_found);",
            "HttpBoundary::new(requests::policy(admission))",
            ".with_rendered_liveness(ProbePath::new(public::HEALTH_PATH)?, probes::liveness)?",
            "probes::mount(boundary, readiness)?",
            "pub async fn in_process(state: AppState) -> Result<InProcessClient, BoxError>",
            "documented_paths_are_served_routes_or_boundary_probes",
        ],
    );
    assert_contains_none(
        &http_lib,
        &[
            "admin",
            "SetRequestIdLayer",
            "PropagateRequestIdLayer",
            "router_with_shutdown",
            "router_with_lifecycle",
            "low_level",
            "operational_http",
            "into_router",
        ],
    );
}

fn assert_workspace_and_backend_crates(destination: &Path) {
    assert_workspace_and_binary_manifests(destination);
    assert_application_and_public_http_crates(destination);
    assert_admin_http_crate(destination);
}

fn assert_http_test_support(destination: &Path) {
    let test_support_cargo =
        fs::read_to_string(destination.join("crates/my-app-test-support/Cargo.toml")).unwrap();
    assert!(test_support_cargo.contains(r#"my-app = { path = "../my-app""#));
    assert!(test_support_cargo.contains(r#"my-app-http = { path = "../my-app-http""#));
    assert!(
        test_support_cargo
            .contains(r#"batter = { workspace = true, features = ["sqlx-test-support"] }"#)
    );
    assert!(test_support_cargo.contains("postgres-test-harness.workspace = true"));
    assert!(!test_support_cargo.contains("tower"));
    let test_support_app =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/app.rs")).unwrap();
    assert_contains_all(
        &test_support_app,
        &[
            "pub struct TestApp",
            "client: InProcessClient,",
            "pub async fn new() -> Self",
            "app_http_crate::in_process(state)",
            "let response = self.client.request(request).await;",
        ],
    );
    assert!(!test_support_app.contains(".oneshot("));
    let test_support_response =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/responses.rs"))
            .unwrap();
    assert!(test_support_response.contains("pub struct TestResponse"));
    assert!(test_support_response.contains("failed to decode response JSON"));
    assert!(test_support_response.contains("pub fn assert_error"));
    let test_support_http_test =
        fs::read_to_string(destination.join("crates/my-app-test-support/tests/http.rs")).unwrap();
    assert_contains_all(
        &test_support_http_test,
        &[
            "use ::my_app_test_support::TestApp;",
            "let app = TestApp::new().await;",
            "async fn health_returns_ok()",
            "for path in [\"/health\", \"/health/live\"]",
            "async fn readiness_reflects_state()",
            "StatusCode::SERVICE_UNAVAILABLE",
            "async fn responses_include_request_id()",
            "async fn unknown_routes_return_a_standard_error_with_the_request_id()",
            "async fn version_returns_json()",
            "async fn status_returns_application_identity_and_readiness()",
        ],
    );
}

fn assert_http_contract_and_test_support(destination: &Path) {
    assert_public_http_contract(destination);
    assert_http_test_support(destination);
}

fn assert_database_crate_and_test_support(destination: &Path) {
    let db_lib = fs::read_to_string(destination.join("crates/my-app-db/src/lib.rs")).unwrap();
    assert_database_crate_source(&db_lib);
    let db_tests = fs::read_to_string(destination.join("crates/my-app-db/src/tests.rs")).unwrap();
    assert_contains_all(
        &db_tests,
        &[
            "interrupted_established_probe_retires_lease_and_closes_pool",
            "health_policy_admits_a_probe_before_observations_expire",
            "let owner = OperationOwner::new(Duration::from_secs(5)).unwrap();",
            "owner.cancel();",
        ],
    );
    let test_support_db =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/db.rs")).unwrap();
    assert_contains_all(
        &test_support_db,
        &[
            "use ::my_app_db as app_db_crate;",
            "pub type TestDbPool = app_db_crate::DbPool;",
            "const FIXTURE_PROJECT: &str = \"my_app\";",
            "HarnessConfig::new(FIXTURE_PROJECT)?",
            "pub async fn with_migrated_database<F, Fut, T>(body: F) -> anyhow::Result<T>",
            "FixtureSuite::new(harness.clone()).start(",
            ".template(migration_template(), initialize_template)",
            "template_spec(&bundles, TEMPLATE_REVISION)",
            "vec![(\"application\", &app_db_crate::MIGRATOR)]",
            "db.migrate(&context).await",
            "std::panic::resume_unwind(error.into_panic())",
        ],
    );
    assert_contains_none(
        &test_support_db,
        &["TEST_DATABASE_URL", "DATABASE_URL\")", "test_db_", "runledger"],
    );
    let postgres_test =
        fs::read_to_string(destination.join("crates/my-app-test-support/tests/postgres.rs"))
            .unwrap();
    assert_contains_all(
        &postgres_test,
        &[
            "#[ignore = \"run with the root test:postgres package script\"]",
            "with_migrated_database(|db| async move {",
            "migrated_database_is_ready_through_the_public_boundary",
            "health_monitor_observes_the_database_under_supervision",
            ".register_health(scope.registration())",
            "running.shutdown_checked().await",
        ],
    );
    assert!(!postgres_test.contains("example_job_runs_under_the_supervised_worker"));
}

fn assert_database_crate_source(db_lib: &str) {
    assert_contains_all(
        db_lib,
        &[
            "pub type DbPool = sqlx::PgPool;",
            "sqlx::Postgres::database_exists",
            "sqlx::Postgres::create_database",
            "Could not confirm database existence after creation failed",
            "create_if_missing",
            "DEFAULT_DB_TIMEOUT",
            "pub static MIGRATOR: Migrator = sqlx::migrate!(",
            "\"../../migrations\"",
            "pub async fn connect_in(",
            "database_url: &SecretString,",
            "batter::sqlx::pool_in(cleanup, PgPoolOptions::new(), options)",
            "let probe = context.child(DEFAULT_DB_TIMEOUT)?.into_context();",
            "batter::sqlx::probe(database.pool(), &probe)",
            "PgLease::acquire(self.pool(), &operation)",
            ".run(\"database.migrate\", |_| lease.migrate(&MIGRATOR))",
            "pub fn register_health(",
            "HealthMonitor::new(health_policy()?",
            "monitor.register_in(&mut registration, \"database.health\")",
            "pub type DbHealthError = OperationError<SqlxFailure>;",
        ],
    );
    assert_contains_none(
        db_lib,
        &[
            "sqlx::query(\"SELECT 1\")",
            "connect_with_timeout",
            "migrate_with_timeout",
            "RunledgerDatabase",
            "OperationContext::new",
        ],
    );
}

fn assert_postgres_test_script(destination: &Path) {
    let postgres_script = fs::read_to_string(destination.join("scripts/test-postgres.sh")).unwrap();
    assert_contains_all(
        &postgres_script,
        &[
            "if [ -n \"${POSTGRES_TEST_ADMIN_URL:-}\" ]; then",
            "--publish 127.0.0.1::5432",
            "postgres:18",
            "docker rm --force",
            "--dbname postgres",
            "--command 'SELECT 1'",
            "attempt=$((attempt + 1))",
            "export POSTGRES_TEST_ADMIN_URL=\"postgres://postgres:postgres@127.0.0.1:${host_port}/postgres?sslmode=disable\"",
            "cargo test --locked -p my-app-test-support --test postgres -- --ignored --nocapture",
        ],
    );
    assert_contains_none(
        &postgres_script,
        &["pg_isready", "seq 1 60", "TEST_DATABASE_URL", "test_db_", "POSTGRES_DB="],
    );
}

fn assert_generated_backend_docs(destination: &Path) {
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

fn assert_database_support_and_docs(destination: &Path) {
    assert_database_crate_and_test_support(destination);
    assert_postgres_test_script(destination);
    assert_generated_backend_docs(destination);
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
    let bootstrap = config["commands"]["repo_bootstrap_command"].as_str().unwrap();
    assert!(bootstrap.contains("scripts/check-webapps.sh bootstrap"));
    assert_contains_none(bootstrap, &["DATABASE_URL", "--bootstrap-database"]);
    let database_setup = fs::read_to_string(destination.join("scripts/setup-database.sh")).unwrap();
    assert_contains_all(&database_setup, &["Missing DATABASE_URL", "before database setup", "cargo run -p my-app-api -- --bootstrap-database"]);
    assert_contains_none(&answers, &["(cd web && bun install)"]);
}
