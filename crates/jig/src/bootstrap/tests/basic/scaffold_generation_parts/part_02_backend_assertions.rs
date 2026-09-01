fn assert_admin_theme_contract(destination: &Path) {
    let admin_index = fs::read_to_string(destination.join("admin-panel/index.html")).unwrap();
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
        fs::read_to_string(destination.join("admin-panel/src/components/theme-provider.tsx"))
            .unwrap();
    assert_contains_all(
        &theme_provider,
        &[
            "storage = window.localStorage",
            "if (event.storageArea !== storage)",
        ],
    );
    let providers =
        fs::read_to_string(destination.join("admin-panel/src/app/providers.tsx")).unwrap();
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
        fs::read_to_string(destination.join("admin-panel/src/app/router.ts")).unwrap();
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
        fs::read_to_string(destination.join("admin-panel/src/app/shell.tsx")).unwrap();
    assert_contains_all(
        &admin_shell,
        &[
            r#"from "@tanstack/react-router""#,
            "const appTitle = \"Admin Panel\"",
            ">{appTitle}</p>",
        ],
    );
    let admin_sidebar =
        fs::read_to_string(destination.join("admin-panel/src/components/app-sidebar.tsx")).unwrap();
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
        destination.join("admin-panel/src/features/overview/overview-page.test.tsx"),
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
        fs::read_to_string(destination.join("admin-panel/.prettierignore")).unwrap();
    assert_eq!(admin_prettierignore.matches("dist/\n").count(), 1);
    assert_eq!(admin_prettierignore.matches("pnpm-lock.yaml").count(), 1);
    assert_eq!(
        admin_prettierignore.matches("npm-shrinkwrap.json").count(),
        1
    );
    assert!(admin_prettierignore.contains("bun.lock\nbun.lockb\n"));
    assert!(admin_prettierignore.contains("src/routeTree.gen.ts"));
    let admin_empty =
        fs::read_to_string(destination.join("admin-panel/src/components/ui/empty.tsx")).unwrap();
    assert!(admin_empty.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_empty.contains("React.ComponentProps"));
    let admin_skeleton =
        fs::read_to_string(destination.join("admin-panel/src/components/ui/skeleton.tsx")).unwrap();
    assert!(admin_skeleton.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_skeleton.contains("React.ComponentProps"));
    let admin_sonner =
        fs::read_to_string(destination.join("admin-panel/src/components/ui/sonner.tsx")).unwrap();
    assert!(admin_sonner.contains(r#"import type { CSSProperties } from "react""#));
    assert!(!admin_sonner.contains("React.CSSProperties"));
    let components = fs::read_to_string(destination.join("admin-panel/components.json")).unwrap();
    assert!(components.contains(r#""style": "radix-nova""#));
    assert!(
        destination
            .join("admin-panel/src/components/ui/sidebar.tsx")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/features/overview/overview-page.tsx")
            .exists()
    );
    assert!(destination.join("admin-panel/src/lib/api.ts").exists());
}

fn assert_admin_theme_and_components(destination: &Path) {
    assert_admin_theme_contract(destination);
    assert_admin_component_sources(destination);
}

fn assert_admin_data_and_routes(destination: &Path) {
    let admin_api = fs::read_to_string(destination.join("admin-panel/src/lib/api.ts")).unwrap();
    assert!(admin_api.contains("getAdminStatusOptions"));
    assert!(admin_api.contains("adminStatusQueryOptions"));
    assert!(
        destination
            .join("admin-panel/src/lib/query-client.ts")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/app/router-context.ts")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/routes/__root.tsx")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/routes/index.tsx")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/routes/settings.tsx")
            .exists()
    );
    assert!(
        destination
            .join("admin-panel/src/routeTree.gen.ts")
            .exists()
    );
    let admin_index_route =
        fs::read_to_string(destination.join("admin-panel/src/routes/index.tsx")).unwrap();
    assert!(admin_index_route.contains(r#"createFileRoute("/")"#));
    assert!(admin_index_route.contains("context.queryClient.ensureQueryData"));
    let admin_query_client =
        fs::read_to_string(destination.join("admin-panel/src/lib/query-client.ts")).unwrap();
    assert!(admin_query_client.contains("retry: 1"));
    let admin_overview =
        fs::read_to_string(destination.join("admin-panel/src/features/overview/overview-page.tsx"))
            .unwrap();
    assert!(admin_overview.contains("useSuspenseQuery(appStatusQueryOptions)"));
    assert!(admin_overview.contains("useQueryErrorResetBoundary()"));
}

fn assert_agent_map_and_database_ignores(destination: &Path) {
    let agent_map = fs::read_to_string(destination.join("agent-map.md")).unwrap();
    for guide in [
        "crates/my-app/AGENTS.md",
        "crates/my-app-db/AGENTS.md",
        "crates/my-app-http/AGENTS.md",
        "crates/my-app-test-support/AGENTS.md",
    ] {
        assert!(agent_map.contains(guide), "agent map is missing {guide}");
    }
    let root_gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(root_gitignore.contains("/my_app.db\n"));
    assert!(root_gitignore.contains("/my_app.db-*\n"));
    for database_file in [
        "my_app.db",
        "my_app.db-wal",
        "my_app.db-shm",
        "my_app.db-journal",
        "my_app.db-jig-migrate.lock",
    ] {
        fs::write(destination.join(database_file), "local database artifact").unwrap();
    }
    assert_eq!(
        git_stdout(
            destination,
            [
                "check-ignore",
                "--",
                "my_app.db",
                "my_app.db-wal",
                "my_app.db-shm",
                "my_app.db-journal",
                "my_app.db-jig-migrate.lock",
            ],
        )
        .unwrap(),
        "my_app.db\nmy_app.db-wal\nmy_app.db-shm\nmy_app.db-journal\nmy_app.db-jig-migrate.lock"
    );
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
            "let bound_addr = listener",
            "Failed to read API listener address after bind",
            "tracing::info!(%bound_addr, \"listening\")",
            "app_http_crate::router",
            "app_crate::AppConfig::from_env()",
            "app_crate::AppState::from_config(config)",
            "--bootstrap-database",
            "    let command = parse_command()?;\n    let config = app_crate::AppConfig::from_env()",
            "match (arguments.next(), arguments.next())",
            "unexpected API argument",
            "app_crate::AppState::bootstrap_database(&config)",
            "install_panic_hook",
            "tracing::error!(error = ?error, \"API server failed\")",
            "#[allow(clippy::useless_concat)]\n    let default_filter",
            "let default_filter = concat!(",
            "\"my_app=info,\",",
            "\"my_app_api=info,\",",
            "\"tower_http=info\",",
            "Failed to bind API listener",
            "API server exited with an error",
            "SignalKind::terminate",
            "failed to listen for Ctrl-C",
        ],
    );
    assert_contains_none(&api_main, &["args_os().any"]);
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
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=my_app=info,my_app_api=info,my_app_admin_api=info,tower_http=info\nDATABASE_URL=postgres://postgres:postgres@localhost:5432/my_app_dev\n"
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
    let app_lib = fs::read_to_string(destination.join("crates/my-app/src/lib.rs")).unwrap();
    assert_contains_all(
        &app_lib,
        &[
            "pub struct AppConfig",
            "pub fn from_env() -> Result<Self>",
            "std::env::var(\"HOST\")",
            "std::env::var(\"PORT\")",
            "fn resolve_bind_addr(",
            "injected_host_and_port_override_the_dotenv_bind_address",
            "partial_jig_bind_values_fall_back_to_bind_addr",
            "DATABASE_URL is required when the db feature is enabled",
            "pub async fn from_config(config: AppConfig) -> Result<Self>",
            "pub async fn bootstrap_database(config: &AppConfig)",
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
        ],
    );
    let http_lib = fs::read_to_string(destination.join("crates/my-app-http/src/lib.rs")).unwrap();
    assert_contains_all(
        &http_lib,
        &[
            "pub fn router(state: AppState) -> Router",
            "TraceLayer::new_for_http()",
            "SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid)",
            "Router::from(public::routes()).fallback(not_found)",
        ],
    );
    assert_contains_none(&http_lib, &["admin"]);
}

fn assert_admin_http_crate(destination: &Path) {
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
}

fn assert_workspace_and_backend_crates(destination: &Path) {
    assert_workspace_and_binary_manifests(destination);
    assert_application_and_public_http_crates(destination);
    assert_admin_http_crate(destination);
}

fn assert_public_http_contract(destination: &Path) {
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
}

fn assert_http_test_support(destination: &Path) {
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
}

fn assert_http_contract_and_test_support(destination: &Path) {
    assert_public_http_contract(destination);
    assert_http_test_support(destination);
}

fn assert_database_crate_and_test_support(destination: &Path) {
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
}

fn assert_postgres_test_script(destination: &Path) {
    let postgres_script = fs::read_to_string(destination.join("scripts/test-postgres.sh")).unwrap();
    assert!(postgres_script.contains("--publish 127.0.0.1::5432"));
    assert!(postgres_script.contains("docker rm --force"));
    assert!(postgres_script.contains("TEST_DATABASE_URL="));
    assert!(postgres_script.contains("test_db_my_app"));
    assert!(postgres_script.contains("--command 'SELECT 1'"));
    assert!(!postgres_script.contains("pg_isready"));
    assert!(!postgres_script.contains("seq 1 60"));
    assert!(postgres_script.contains("attempt=$((attempt + 1))"));
    assert!(postgres_script.contains("-- --ignored --nocapture"));
}

fn assert_generated_backend_docs(destination: &Path) {
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
            "web_package_manager = \"bun\"",
            "if [ -f Cargo.toml ]; then cargo fetch;",
            "cargo run -p my-app-api -- --bootstrap-database",
            "export it or copy .env.example to .env before bootstrap",
            "${DATABASE_URL:-}",
            "name = \"web\"",
            "dir = \"landing\"",
            "kind = \"env-port\"",
            "name = \"admin-panel\"",
            "role = \"spa\"",
            "role = \"astro\"",
            "role = \"admin\"",
        ],
    );
    let web_bootstrap = answers.find("scripts/check-webapps.sh bootstrap").unwrap();
    let database_guard = answers.find("Missing DATABASE_URL").unwrap();
    let database_bootstrap = answers
        .find("cargo run -p my-app-api -- --bootstrap-database")
        .unwrap();
    assert!(web_bootstrap < database_guard);
    assert!(database_guard < database_bootstrap);
    assert_contains_none(&answers, &["(cd web && bun install)"]);
}
