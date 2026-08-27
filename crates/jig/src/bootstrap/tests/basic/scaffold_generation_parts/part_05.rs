fn assert_generated_backend(destination: &std::path::Path) {
    let api_main = fs::read_to_string(destination.join("apps/my-app-api/src/main.rs")).unwrap();
    assert!(api_main.contains("use anyhow::Context;"));
    assert!(api_main.contains("use ::my_app as app_crate;"));
    assert!(api_main.contains("use ::my_app_http as app_http_crate;"));
    assert!(api_main.contains("load_dotenv();"));
    assert!(api_main.contains("warning: failed to load .env"));
    assert!(api_main.contains("let bound_addr = listener"));
    assert!(api_main.contains("Failed to read API listener address after bind"));
    assert!(api_main.contains("tracing::info!(%bound_addr, \"listening\")"));
    assert!(api_main.contains("app_http_crate::router"));
    assert!(api_main.contains("app_crate::AppConfig::from_env()"));
    assert!(api_main.contains("app_crate::AppState::from_config(config)"));
    assert!(api_main.contains("--bootstrap-database"));
    assert!(api_main.contains(
        "    let command = parse_command()?;\n    let config = app_crate::AppConfig::from_env()"
    ));
    assert!(api_main.contains("match (arguments.next(), arguments.next())"));
    assert!(api_main.contains("unexpected API argument"));
    assert!(!api_main.contains("args_os().any"));
    assert!(api_main.contains("app_crate::AppState::bootstrap_database(&config)"));
    assert!(api_main.contains("install_panic_hook"));
    assert!(api_main.contains("tracing::error!(error = ?error, \"API server failed\")"));
    assert!(api_main.contains("#[allow(clippy::useless_concat)]\n    let default_filter"));
    assert!(api_main.contains("let default_filter = concat!("));
    assert!(api_main.contains("\"my_app=info,\","));
    assert!(api_main.contains("\"my_app_api=info,\","));
    assert!(api_main.contains("\"tower_http=info\","));
    assert!(api_main.contains("Failed to bind API listener"));
    assert!(api_main.contains("API server exited with an error"));
    assert!(api_main.contains("SignalKind::terminate"));
    assert!(api_main.contains("failed to listen for Ctrl-C"));
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
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=my_app=info,my_app_api=info,my_app_admin_api=info,tower_http=info\nDATABASE_URL=postgres://postgres:postgres@localhost:5432/my_app_dev\n"
    );
    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("rust-version = \"1.94\""));
    assert!(workspace_cargo.contains("sqlx = { version = \"0.9\""));
    assert!(!workspace_cargo.contains("sqlx = { version = \"0.8\""));
    assert!(workspace_cargo.contains("dotenvy = \"0.15\""));
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
    let app_lib = fs::read_to_string(destination.join("crates/my-app/src/lib.rs")).unwrap();
    assert!(app_lib.contains("pub struct AppConfig"));
    assert!(app_lib.contains("pub fn from_env() -> Result<Self>"));
    assert!(app_lib.contains("std::env::var(\"HOST\")"));
    assert!(app_lib.contains("std::env::var(\"PORT\")"));
    assert!(app_lib.contains("fn resolve_bind_addr("));
    assert!(app_lib.contains("injected_host_and_port_override_the_dotenv_bind_address"));
    assert!(app_lib.contains("partial_jig_bind_values_fall_back_to_bind_addr"));
    assert!(app_lib.contains("DATABASE_URL is required when the db feature is enabled"));
    assert!(app_lib.contains("pub async fn from_config(config: AppConfig) -> Result<Self>"));
    assert!(app_lib.contains("pub async fn bootstrap_database(config: &AppConfig)"));
    assert!(app_lib.contains("pub fn new_with_version(version: impl Into<String>)"));
    assert!(app_lib.contains("pub fn version(&self) -> &AppVersion"));
    assert!(app_lib.contains("pub fn is_ready(&self) -> bool"));
    assert!(!app_lib.contains("return Ok(Self"));
    assert!(!app_lib.contains("return self.db.is_some()"));
    assert!(!app_lib.contains("use axum::"));
    assert!(!app_lib.contains("pub fn router"));
    let http_lib = fs::read_to_string(destination.join("crates/my-app-http/src/lib.rs")).unwrap();
    assert!(http_lib.contains("pub fn router(state: AppState) -> Router"));
    assert!(http_lib.contains("TraceLayer::new_for_http()"));
    assert!(http_lib.contains("SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid)"));
    assert!(http_lib.contains("Router::from(public::routes()).fallback(not_found)"));
    assert!(!http_lib.contains("admin"));
    let admin_http_lib =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/lib.rs")).unwrap();
    assert!(admin_http_lib.contains("pub trait AdminAuthorizer"));
    assert!(admin_http_lib.contains("pub struct DenyAllAdminAuthorizer"));
    assert!(admin_http_lib.contains("pub fn router<A: AdminAuthorizer>"));
    assert!(admin_http_lib.contains("require_admin_authorization::<A>"));
    assert!(admin_http_lib.contains("pub fn openapi() -> OpenApiDocument"));
    assert!(admin_http_lib.contains("components(schemas(ApiErrorResponse))"));
    assert!(admin_http_lib.contains(r#"path = "/admin-api/status""#));
    assert!(admin_http_lib.contains("operation_id = \"getAdminStatus\""));
    assert!(
        admin_http_lib
            .contains("admin_status_is_protected_and_reflects_readiness_after_authorization")
    );
    assert!(admin_http_lib.contains("let expected_ready = state.is_ready();"));
    assert!(admin_http_lib.contains("assert_eq!(body[\"ready\"], expected_ready);"));
    let http_common_lib =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/lib.rs")).unwrap();
    assert!(http_common_lib.contains("pub struct ApiErrorResponse"));
    assert!(http_common_lib.contains("pub request_id: String"));
    let public_http =
        fs::read_to_string(destination.join("crates/my-app-http/src/public.rs")).unwrap();
    for handler in ["health", "live", "ready", "version", "status"] {
        assert!(public_http.contains(&format!(".routes(routes!({handler}))")));
    }
    assert!(public_http.contains(r#"path = "/health/live""#));
    assert!(public_http.contains(r#"path = "/health/ready""#));
    assert!(public_http.contains(r#"path = "/api/version""#));
    assert!(public_http.contains(r#"path = "/api/status""#));
    assert!(public_http.contains("body = ApiErrorResponse"));
    assert!(public_http.contains(r#""dependency_unavailable""#));
    let test_support_cargo =
        fs::read_to_string(destination.join("crates/my-app-test-support/Cargo.toml")).unwrap();
    assert!(test_support_cargo.contains(r#"my-app = { path = "../my-app""#));
    assert!(test_support_cargo.contains(r#"my-app-http = { path = "../my-app-http""#));
    assert!(test_support_cargo.contains(r#"tower = { workspace = true, features = ["util"] }"#));
    let test_support_app =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/app.rs")).unwrap();
    assert!(test_support_app.contains("pub struct TestApp"));
    assert!(test_support_app.contains(".oneshot(request)"));
    let test_support_response =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/responses.rs"))
            .unwrap();
    assert!(test_support_response.contains("pub struct TestResponse"));
    assert!(test_support_response.contains("failed to decode response JSON"));
    assert!(test_support_response.contains("pub fn assert_error"));
    let test_support_http_test =
        fs::read_to_string(destination.join("crates/my-app-test-support/tests/http.rs")).unwrap();
    assert!(test_support_http_test.contains("use ::my_app_test_support::TestApp;"));
    assert!(test_support_http_test.contains("async fn health_returns_ok()"));
    assert!(test_support_http_test.contains("async fn readiness_reflects_state()"));
    assert!(test_support_http_test.contains("StatusCode::SERVICE_UNAVAILABLE"));
    assert!(test_support_http_test.contains("async fn responses_include_request_id()"));
    assert!(
        test_support_http_test
            .contains("async fn unknown_routes_return_a_standard_error_with_the_request_id()")
    );
    assert!(test_support_http_test.contains("async fn version_returns_json()"));
    assert!(
        test_support_http_test
            .contains("async fn status_returns_application_identity_and_readiness()")
    );
    let db_lib = fs::read_to_string(destination.join("crates/my-app-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("PgPool"));
    assert!(db_lib.contains("sqlx::Postgres::database_exists"));
    assert!(db_lib.contains("sqlx::Postgres::create_database"));
    assert!(db_lib.contains("Could not confirm database existence after creation failed"));
    assert!(db_lib.contains("create_if_missing"));
    assert!(db_lib.contains("DEFAULT_DB_TIMEOUT"));
    assert!(db_lib.contains("connect_with_timeout"));
    assert!(db_lib.contains("migrate_with_timeout"));
    let test_support_db =
        fs::read_to_string(destination.join("crates/my-app-test-support/src/db.rs")).unwrap();
    assert!(test_support_db.contains("pub struct DatabaseTestConfig"));
    assert!(test_support_db.contains("validate_test_database_name"));
    assert!(test_support_db.contains("pub fn from_test_env()"));
    assert!(test_support_db.contains("pub async fn migrate(&self)"));
    let postgres_test =
        fs::read_to_string(destination.join("crates/my-app-test-support/tests/postgres.rs"))
            .unwrap();
    assert!(postgres_test.contains("SELECT current_database()"));
    assert!(postgres_test.contains("validate_test_database_name(&database_name)?"));
    assert!(
        postgres_test.contains("#[ignore = \"run with the root test:postgres package script\"]")
    );
    let postgres_script = fs::read_to_string(destination.join("scripts/test-postgres.sh")).unwrap();
    assert!(postgres_script.contains("--publish 127.0.0.1::5432"));
    assert!(postgres_script.contains("docker rm --force"));
    assert!(postgres_script.contains("TEST_DATABASE_URL="));
    assert!(postgres_script.contains("test_db_my_app"));
    assert!(postgres_script.contains("--command 'SELECT 1'"));
    assert!(!postgres_script.contains("pg_isready"));
    assert!(postgres_script.contains("-- --ignored --nocapture"));
    let root_readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert!(root_readme.contains("Prerequisites: Rust 1.94 or newer"));
    assert!(root_readme.contains("bun run bootstrap"));
    assert!(root_readme.contains("do not start with `bun install --frozen-lockfile`"));
    assert!(root_readme.contains("Commit the generated `bun.lock`"));
    assert!(root_readme.contains("DenyAllAdminAuthorizer"));
    assert!(root_readme.contains("bun run test:postgres"));
    let http_agents = fs::read_to_string(destination.join("crates/my-app-http/AGENTS.md")).unwrap();
    assert!(http_agents.contains("`src/public.rs`: owns public routes"));
    assert!(http_agents.contains("Never depend on `my-app-admin-http`"));
    let app_agents = fs::read_to_string(destination.join("crates/my-app/AGENTS.md")).unwrap();
    assert!(app_agents.contains("Parse environment configuration once at startup"));

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"my-app\""));
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("rust_sqlx_metadata_dir = \".sqlx\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("rust_crate_roots = [\"apps\", \"crates\"]"));
    assert!(answers.contains("web_package_manager = \"bun\""));
    assert!(answers.contains("if [ -f Cargo.toml ]; then cargo fetch;"));
    assert!(answers.contains("cargo run -p my-app-api -- --bootstrap-database"));
    assert!(answers.contains("export it or copy .env.example to .env before bootstrap"));
    assert!(answers.contains("${DATABASE_URL:-}"));
    let web_bootstrap = answers.find("scripts/check-webapps.sh bootstrap").unwrap();
    let database_guard = answers.find("Missing DATABASE_URL").unwrap();
    let database_bootstrap = answers
        .find("cargo run -p my-app-api -- --bootstrap-database")
        .unwrap();
    assert!(web_bootstrap < database_guard);
    assert!(database_guard < database_bootstrap);
    assert!(!answers.contains("(cd web && bun install)"));
    assert!(answers.contains("name = \"web\""));
    assert!(answers.contains("dir = \"landing\""));
    assert!(answers.contains("kind = \"env-port\""));
    assert!(answers.contains("name = \"admin-panel\""));
    assert!(answers.contains("role = \"spa\""));
    assert!(answers.contains("role = \"astro\""));
    assert!(answers.contains("role = \"admin\""));
}
