use super::*;

fn assert_admin_package_tooling(destination: &Path) {
    let admin_package =
        fs::read_to_string(destination.join("apps/admin-panel/package.json")).unwrap();
    let admin_package_json: serde_json::Value = serde_json::from_str(&admin_package).unwrap();
    assert_eq!(
        admin_package_json["devDependencies"]["@types/node"].as_str(),
        Some(GENERATED_NODE_TYPES_VERSION)
    );
    assert_contains_all(
        &admin_package,
        &[
            r#""shadcn": "4.18.0""#,
            r#""tailwindcss": "4.3.3""#,
            r#""@tanstack/react-query": "5.101.4""#,
            r#""@tanstack/react-router": "1.170.29""#,
            r#""@tanstack/eslint-plugin-query": "5.101.4""#,
            r#""@tanstack/router-plugin": "1.168.32""#,
            r#""@vitest/eslint-plugin": "1.6.27""#,
            r#""eslint-plugin-testing-library": "7.16.2""#,
            r#""my-app-public-api-client": "*""#,
            r#""my-app-admin-api-client": "*""#,
            r#""build": "vite build && tsc -b""#,
            r#""@testing-library/dom": "10.4.1""#,
            r#""lint": "eslint . --max-warnings 0 && prettier --check .""#,
            r#""lint:cached": "eslint . --cache --cache-location node_modules/.cache/eslint --max-warnings 0 && prettier --check .""#,
            r#""format": "prettier --write .""#,
            r#""format:check": "prettier --check .""#,
        ],
    );
    assert_contains_none(&admin_package, &["react-router-dom", "@playwright/test"]);
    let admin_eslint =
        fs::read_to_string(destination.join("apps/admin-panel/eslint.config.js")).unwrap();
    assert!(admin_eslint.contains(r#"from "../../eslint.config.shared.mjs""#));
    assert!(!admin_eslint.contains("forbiddenApiClientPackages"));
    let admin_readme = fs::read_to_string(destination.join("apps/admin-panel/README.md")).unwrap();
    assert!(admin_readme.contains("real-backend Playwright starter for product SPA roles only"));
}

fn assert_admin_vite_config(destination: &Path) {
    let admin_vite_config =
        fs::read_to_string(destination.join("apps/admin-panel/vite.config.ts")).unwrap();
    assert!(admin_vite_config.contains(r#"from "@tanstack/router-plugin/vite""#));
    assert!(admin_vite_config.contains("path.resolve(import.meta.dirname, \"./src\")"));
    assert!(!admin_vite_config.contains("__dirname"));
    assert!(admin_vite_config.contains("autoCodeSplitting: true"));
    assert!(admin_vite_config.contains("codeSplitting:"));
    assert!(admin_vite_config.contains("name: \"vendor\""));
    assert!(admin_vite_config.contains("maxSize: 350_000"));
    assert!(admin_vite_config.contains("const devPort = Number(process.env.PORT)"));
    assert!(admin_vite_config.contains("port: devPort"));
    assert!(admin_vite_config.contains("strictPort: true"));
    assert!(admin_vite_config.contains("clientPort: devPort"));
    assert!(
        admin_vite_config
            .contains("firstNonEmpty(process.env.JIG_DEV_API_ORIGIN, process.env.API_ORIGIN)")
    );
    assert!(admin_vite_config.contains("process.env.JIG_DEV_ADMIN_API_ORIGIN"));
    assert!(admin_vite_config.contains("process.env.ADMIN_API_ORIGIN"));
    assert!(admin_vite_config.contains(r#""/admin-api""#));
    assert!(admin_vite_config.contains("target: adminApiOrigin"));
    assert!(
        !admin_vite_config
            .contains("firstNonEmpty(process.env.API_ORIGIN, process.env.JIG_DEV_API_ORIGIN)")
    );
}

pub(super) fn assert_landing_and_admin_tooling(destination: &Path) {
    assert_landing_tooling(destination);
    assert_admin_package_tooling(destination);
    assert_admin_vite_config(destination);
}

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
        fs::read_to_string(destination.join("apps/admin-panel/src/components/app-sidebar.tsx"))
            .unwrap();
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
        fs::read_to_string(destination.join("apps/admin-panel/src/components/ui/empty.tsx"))
            .unwrap();
    assert!(admin_empty.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_empty.contains("React.ComponentProps"));
    let admin_skeleton =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/ui/skeleton.tsx"))
            .unwrap();
    assert!(admin_skeleton.contains(r#"import type { ComponentProps } from "react""#));
    assert!(!admin_skeleton.contains("React.ComponentProps"));
    let admin_sonner =
        fs::read_to_string(destination.join("apps/admin-panel/src/components/ui/sonner.tsx"))
            .unwrap();
    assert!(admin_sonner.contains(r#"import type { CSSProperties } from "react""#));
    assert!(!admin_sonner.contains("React.CSSProperties"));
    let components =
        fs::read_to_string(destination.join("apps/admin-panel/components.json")).unwrap();
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

pub(super) fn assert_admin_theme_and_components(destination: &Path) {
    assert_admin_theme_contract(destination);
    assert_admin_component_sources(destination);
}

pub(super) fn assert_admin_data_and_routes(destination: &Path) {
    let admin_api =
        fs::read_to_string(destination.join("apps/admin-panel/src/lib/api.ts")).unwrap();
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
    let admin_overview = fs::read_to_string(
        destination.join("apps/admin-panel/src/features/overview/overview-page.tsx"),
    )
    .unwrap();
    assert!(admin_overview.contains("useSuspenseQuery(appStatusQueryOptions)"));
    assert!(admin_overview.contains("useQueryErrorResetBoundary()"));
}

pub(super) fn assert_admin_http_crate(destination: &Path) {
    let admin_http_lib =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/lib.rs")).unwrap();
    let authorization =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/authorization.rs"))
            .unwrap();
    let admin_tests =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/tests.rs")).unwrap();
    assert_contains_all(
        &authorization,
        &[
            "pub trait AdminAuthorizer",
            "pub struct DenyAllAdminAuthorizer",
            "admitted: AdmittedRequest,",
            "let correlation_id = admitted.correlation_id();",
            "ApiError::unauthorized(Some(correlation_id))",
            "ApiError::forbidden(Some(correlation_id))",
        ],
    );
    assert_contains_all(
        &admin_http_lib,
        &[
            "pub use authorization::{",
            "pub async fn assemble<A, E>(",
            "authorization::require_admin_authorization::<A>",
            "GuardedRouter::from_router(protected, inventory)",
            "GroupPolicy::browser(requests::policy(admission), browser_policy()?)",
            ".with_probe_response_policy(PrivateResponsePolicy::NoReferrer)",
            "MutationPolicy::required_header(marker)",
            "PrivateResponsePolicy::NoReferrer,",
            "pub const ADMIN_REQUEST_HEADER: &str = \"x-admin-request\";",
            "pub async fn in_process<A: AdminAuthorizer>(",
            "pub fn openapi() -> OpenApiDocument",
            "components(schemas(ApiErrorResponse))",
            r#"path = "/admin-api/status""#,
            "operation_id = \"getAdminStatus\"",
        ],
    );
    assert_contains_all(
        &admin_tests,
        &[
            "admin_status_is_protected_and_reflects_readiness_after_authorization",
            "let expected_ready = state.is_ready();",
            "assert_eq!(json(response).await[\"ready\"], expected_ready);",
            "unknown_admin_paths_remain_not_found_without_authorization_challenge",
            "probes_answer_outside_authorization_with_private_headers",
            "mutations_require_same_origin_fetch_metadata_and_the_admin_marker",
            "committed_admin_openapi_is_current",
        ],
    );
    for source in [&admin_http_lib, &authorization] {
        assert_contains_none(
            source,
            &[
                "REQUEST_ID_HEADER",
                "SetRequestIdLayer",
                "PropagateRequestIdLayer",
                "router_with_shutdown",
                "router_with_lifecycle",
                "observe_http",
                "low_level",
                "ApiError::unauthorized(request.headers())",
                "ApiError::forbidden(request.headers())",
            ],
        );
    }
    let admin_api_main =
        fs::read_to_string(destination.join("apps/my-app-admin-api/src/main.rs")).unwrap();
    assert_contains_all(
        &admin_api_main,
        &[
            "runtime::serve(config, |state, admission, readiness| {",
            concat!(
                "admin_http_crate::assemble(\n",
                "            state,\n",
                "            admin_http_crate::DenyAllAdminAuthorizer,\n",
                "            admission,\n",
                "            readiness,\n",
                "        )"
            ),
        ],
    );
    assert_contains_none(
        &admin_api_main,
        &["router_with_shutdown", "serve_with_jobs"],
    );
}

#[test]
fn run_init_rust_react_scaffold_omits_admin_contract_without_admin_frontend() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("public-app");

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
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(destination.join("openapi/public.json").exists());
    assert!(
        destination
            .join("packages/public-api-client/src/generated/sdk.gen.ts")
            .exists()
    );
    assert!(!destination.join("openapi/admin.json").exists());
    assert!(!destination.join("crates/public-app-admin-http").exists());
    assert!(!destination.join("apps/public-app-admin-api").exists());
    assert!(!destination.join("packages/admin-api-client").exists());

    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("rust-version = \"1.94\""));
    assert_generated_rust_clippy_defaults(&destination);
    assert!(!workspace_cargo.contains("sqlx ="));
    let root_readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert!(root_readme.contains("Prerequisites: Rust 1.94 or newer"));

    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packages/public-api-client""#));
    assert!(!workspace_package.contains(r#""packages/admin-api-client""#));
    let exporter =
        fs::read_to_string(destination.join("apps/public-app-api/src/bin/export-openapi.rs"))
            .unwrap();
    assert!(exporter.contains("public_openapi"));
    assert!(!exporter.contains("admin"));
    let public_api_manifest =
        fs::read_to_string(destination.join("apps/public-app-api/Cargo.toml")).unwrap();
    assert!(!public_api_manifest.contains("admin"));
    let contracts = fs::read_to_string(destination.join("scripts/contracts.mjs")).unwrap();
    assert!(!contracts.contains("cargoPackage: \"public-app-admin-api\""));
    assert!(!contracts.contains("name: \"admin\""));
}

#[test]
fn rust_react_admin_dynamic_values_use_formatter_stable_boundaries() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo_name = "r".repeat(120);
    let frontend_name = format!("admin-{}", "x".repeat(100));
    let destination = temp.path().join(&repo_name);

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend(&format!("{frontend_name}:admin")).unwrap(),
            ],
            metrics: None,
            jobs: None,
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let admin = destination.join("apps").join(&frontend_name);
    let theme_storage_key = format!("{frontend_name}-theme");
    let index = fs::read_to_string(admin.join("index.html")).unwrap();
    assert!(index.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(index.matches(&theme_storage_key).count(), 1);
    assert!(index.contains("localStorage.getItem(themeStorageKey)"));
    assert!(index.contains("<!-- prettier-ignore -->\n    <title>"));

    let providers = fs::read_to_string(admin.join("src/app/providers.tsx")).unwrap();
    assert!(providers.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(providers.matches(&theme_storage_key).count(), 1);
    assert!(providers.contains("storageKey={themeStorageKey}"));

    let shell = fs::read_to_string(admin.join("src/app/shell.tsx")).unwrap();
    assert!(shell.contains("const appTitle = \""));
    assert!(shell.contains(">{appTitle}</p>"));

    let sidebar = fs::read_to_string(admin.join("src/components/app-sidebar.tsx")).unwrap();
    assert!(sidebar.contains(&format!("const appName = \"{repo_name}\"")));
    assert_eq!(sidebar.matches(&repo_name).count(), 1);
    assert!(sidebar.contains(">{appName}</span>"));

    let overview_test =
        fs::read_to_string(admin.join("src/features/overview/overview-page.test.tsx")).unwrap();
    assert!(overview_test.contains(&format!("const expectedAppName = \"{repo_name}\"")));
    assert_eq!(overview_test.matches(&repo_name).count(), 1);
    assert!(overview_test.contains("name: expectedAppName"));
    assert!(overview_test.contains("screen.findAllByText(expectedAppName)"));
}
