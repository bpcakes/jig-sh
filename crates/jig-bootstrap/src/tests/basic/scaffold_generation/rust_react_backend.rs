use super::*;

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
        &[
            "std::env::var(",
            "database_url: Option<String>",
            "METRICS_OTLP_ENDPOINT",
        ],
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

pub(super) fn assert_workspace_and_backend_crates(destination: &Path) {
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

pub(super) fn assert_http_contract_and_test_support(destination: &Path) {
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
        &[
            "TEST_DATABASE_URL",
            "DATABASE_URL\")",
            "test_db_",
            "runledger",
        ],
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
        &[
            "pg_isready",
            "seq 1 60",
            "TEST_DATABASE_URL",
            "test_db_",
            "POSTGRES_DB=",
        ],
    );
}

pub(super) fn assert_database_support_and_docs(destination: &Path) {
    assert_database_crate_and_test_support(destination);
    assert_postgres_test_script(destination);
    assert_generated_backend_docs(destination);
}

fn assert_public_http_contract(destination: &Path) {
    let http_common_lib =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/lib.rs")).unwrap();
    assert!(http_common_lib.contains("pub struct ApiErrorResponse"));
    assert!(http_common_lib.contains("pub request_id: String"));
    assert_contains_all(
        &http_common_lib,
        &[
            "use batter::axum::{AdmittedRequest, CorrelationId, RouteInventory, RouteInventoryError};",
            "correlation_id: Option<&CorrelationId>",
            "request_id: request_id(correlation_id)",
            "pub async fn not_found(admitted: AdmittedRequest) -> ApiError",
            "ApiError::not_found(Some(admitted.correlation_id()))",
            "pub fn route_inventory(document: &OpenApi) -> Result<RouteInventory, RouteInventoryError>",
            "fn request_id(correlation_id: Option<&CorrelationId>) -> String",
            ".map(CorrelationId::as_str)",
        ],
    );
    assert_contains_none(
        &http_common_lib,
        &[
            "REQUEST_ID_HEADER",
            "HeaderMap",
            "request_id(headers",
            "request.headers()",
            "Extension(",
        ],
    );
    let requests =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/requests.rs")).unwrap();
    assert_contains_all(
        &requests,
        &[
            "pub const REQUEST_BUDGET: Duration = Duration::from_secs(10);",
            "ResponseConstructionBudget::new(REQUEST_BUDGET)",
            "RequestPolicy::new(admission, request_budget).with_failure_renderer(|failure, parts| {",
            concat!(
                "crate::ApiError::new(\n",
                "            failure.status(),\n",
                "            failure.code(),\n",
                "            \"The request could not be completed\",\n",
                "            parts.extensions.get::<CorrelationId>(),\n",
                "        )"
            ),
            "HttpBoundary::new(policy(shutdown.operation_admission()))",
            ".in_process();",
        ],
    );
    assert_contains_none(
        &requests,
        &[
            "RequestPolicy::new(shutdown",
            "request_admission",
            "operational_http",
            "middleware::from_fn",
        ],
    );
    let probes =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/probes.rs")).unwrap();
    assert_contains_all(
        &probes,
        &[
            "pub const LIVENESS_PATH: &str = \"/health/live\";",
            "pub const READINESS_PATH: &str = \"/health/ready\";",
            "pub fn liveness(_parts: &Parts) -> Response",
            "pub fn readiness(decision: ReadinessDecision, parts: &Parts) -> Response",
            "ReadinessUnreadyReason::Dependency(_) | ReadinessUnreadyReason::Condition(_)",
            "\"dependency_unavailable\"",
            "\"service_unavailable\"",
            ".with_rendered_liveness(ProbePath::new(LIVENESS_PATH)?, liveness)?",
            ".with_rendered_readiness(ProbePath::new(READINESS_PATH)?, readiness_policy, readiness)?",
        ],
    );
    let public_http =
        fs::read_to_string(destination.join("crates/my-app-http/src/public.rs")).unwrap();
    for handler in ["health", "live", "ready", "version", "status"] {
        assert!(public_http.contains(&format!(".routes(routes!({handler}))")));
    }
    assert_contains_all(
        &public_http,
        &[
            r#"path = "/health/live""#,
            r#"path = "/health/ready""#,
            r#"path = "/api/version""#,
            r#"path = "/api/status""#,
            "body = ApiErrorResponse",
            "pub(super) const HEALTH_PATH: &str = \"/health\";",
            "pub(super) fn application_routes() -> OpenApiRouter<AppState>",
            "fn probe_documentation() -> OpenApiRouter<AppState>",
        ],
    );
    assert_contains_none(
        &public_http,
        &[
            "HeaderMap",
            "ShutdownHandle",
            "&headers",
            "Extension(",
            "LifecycleStatus",
        ],
    );
}

#[test]
fn postgres_readme_matches_selected_backend_test_contract() {
    let planning_root = tempdir().unwrap();
    let go_contract = ["requires Docker", "`TEST_DATABASE_URL`"];
    let rust_contract = ["Batter's fixture harness", "`POSTGRES_TEST_ADMIN_URL`"];
    for (preset, expected, forbidden) in [
        (ScaffoldPreset::GoReact, go_contract, rust_contract),
        (ScaffoldPreset::RustReact, rust_contract, go_contract),
    ] {
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(preset),
                db: Some(ScaffoldDb::Postgres),
                frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
                ..ScaffoldOpts::default()
            },
            &AnswerOpts {
                repo_name: Some("example-project".into()),
                go_module: match preset {
                    ScaffoldPreset::GoReact => Some("example.com/ExampleProject".into()),
                    _ => None,
                },
                ..AnswerOpts::default()
            },
            planning_root.path(),
        )
        .unwrap()
        .unwrap();

        let rendered = plan.render_files().unwrap();
        let readme = rendered_contents(&rendered, "README.md");
        assert_contains_all(readme, &expected);
        assert_contains_none(readme, &forbidden);
        assert!(readme.contains("The tests never fall back to `DATABASE_URL`."));
    }
}
