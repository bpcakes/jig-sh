use super::*;

pub(super) fn assert_public_spa_package_and_clients(destination: &Path) {
    let web_package = fs::read_to_string(destination.join("apps/web/package.json")).unwrap();
    let web_package_json: serde_json::Value = serde_json::from_str(&web_package).unwrap();
    assert_eq!(
        web_package_json["devDependencies"]["@types/node"].as_str(),
        Some(GENERATED_NODE_TYPES_VERSION)
    );
    assert_contains_all(
        &web_package,
        &[
            r#""dev": "vite""#,
            r#""shadcn": "4.18.0""#,
            r#""tailwindcss": "4.3.3""#,
            r#""@tanstack/react-query": "5.101.4""#,
            r#""@tanstack/react-router": "1.170.29""#,
            r#""@tanstack/eslint-plugin-query": "5.101.4""#,
            r#""@tanstack/router-plugin": "1.168.32""#,
            r#""@vitest/eslint-plugin": "1.6.27""#,
            r#""eslint-plugin-testing-library": "7.16.2""#,
            r#""my-app-public-api-client": "*""#,
            r#""build": "vite build && tsc -b""#,
            r#""@testing-library/dom": "10.4.1""#,
            r#""@playwright/test": "1.62.1""#,
            r#""test:e2e": "playwright test""#,
            r#""test:e2e:install": "playwright install chromium""#,
            r#""test:e2e:install:ci": "playwright install --with-deps chromium""#,
            r#""lint": "eslint . --max-warnings 0""#,
            r#""lint:cached": "eslint . --cache --cache-location node_modules/.cache/eslint --max-warnings 0""#,
        ],
    );
    assert_contains_none(&web_package, &["my-app-admin-api-client", " install && "]);
    let web_eslint = fs::read_to_string(destination.join("apps/web/eslint.config.js")).unwrap();
    assert_contains_all(
        &web_eslint,
        &[
            r#"from "../../eslint.config.shared.mjs""#,
            "forbiddenApiClientPackages",
            r#""my-app-admin-api-client""#,
        ],
    );
    assert_paths_exist(
        destination,
        &[
            "apps/web/src/api.ts",
            "packages/public-api-client/src/generated/sdk.gen.ts",
            "packages/admin-api-client/src/generated/sdk.gen.ts",
            "packages/admin-api-client/src/generated/zod.gen.ts",
        ],
    );
    let admin_query = fs::read_to_string(
        destination.join("packages/admin-api-client/src/generated/@tanstack/react-query.gen.ts"),
    )
    .unwrap();
    assert!(admin_query.contains("getAdminStatusOptions"));
}

fn assert_generated_api_clients_and_spa_paths(destination: &Path) {
    for client in ["public-api-client", "admin-api-client"] {
        let client_index = fs::read_to_string(
            destination
                .join("packages")
                .join(client)
                .join("src/index.ts"),
        )
        .unwrap();
        assert!(
            client_index.contains(r#"export * from "./generated/@tanstack/react-query.gen";"#),
            "{client} must export generated React Query helpers"
        );
    }
    assert!(destination.join("apps/web/src/app/providers.tsx").exists());
    assert!(
        destination
            .join("apps/web/src/app/router-context.ts")
            .exists()
    );
    assert!(destination.join("apps/web/src/app/router.ts").exists());
    assert!(
        destination
            .join("apps/web/src/lib/query-client.ts")
            .exists()
    );
    assert!(destination.join("apps/web/src/routes/__root.tsx").exists());
    assert!(destination.join("apps/web/src/routes/index.tsx").exists());
    assert!(destination.join("apps/web/src/routeTree.gen.ts").exists());
    assert!(destination.join("apps/web/playwright.config.ts").exists());
    assert!(destination.join("apps/web/e2e/app.spec.ts").exists());
    assert!(destination.join("apps/web/tsconfig.app.json").exists());
    assert!(destination.join("apps/web/tsconfig.node.json").exists());
}

fn assert_public_spa_source_files(destination: &Path) {
    let web_tsconfig_app =
        fs::read_to_string(destination.join("apps/web/tsconfig.app.json")).unwrap();
    assert_contains_all(
        &web_tsconfig_app,
        &[
            r#""types": ["vite/client", "vitest/globals"]"#,
            r#""include": ["src"]"#,
        ],
    );
    assert_contains_none(&web_tsconfig_app, &[r#""node""#]);
    let web_tsconfig_node =
        fs::read_to_string(destination.join("apps/web/tsconfig.node.json")).unwrap();
    assert_contains_all(
        &web_tsconfig_node,
        &[
            r#""types": ["node"]"#,
            r#""playwright.config.ts""#,
            r#""e2e""#,
        ],
    );
    assert_paths_exist(
        destination,
        &[
            "apps/web/components.json",
            "apps/web/src/components/ui/button.tsx",
            "apps/web/src/components/ui/card.tsx",
            "apps/web/src/lib/utils.ts",
        ],
    );
    let web_components = fs::read_to_string(destination.join("apps/web/components.json")).unwrap();
    assert!(web_components.contains(r#""style": "radix-nova""#));
    let web_css = fs::read_to_string(destination.join("apps/web/src/index.css")).unwrap();
    assert_contains_all(
        &web_css,
        &[
            r#"@import "tailwindcss";"#,
            r#"@import "shadcn/tailwind.css";"#,
        ],
    );
    let web_app = fs::read_to_string(destination.join("apps/web/src/App.tsx")).unwrap();
    assert_contains_all(
        &web_app,
        &[
            r#"from "@/components/ui/card""#,
            "useSuspenseQuery(appStatusQueryOptions)",
            "useQueryErrorResetBoundary()",
            "appStatusQueryOptions",
        ],
    );
    let web_api = fs::read_to_string(destination.join("apps/web/src/api.ts")).unwrap();
    assert_contains_all(
        &web_api,
        &[
            "export const appStatusQueryOptions = getAppStatusOptions({",
            "baseUrl: globalThis.location.origin",
            "export type AppStatus = AppStatusResponse",
            "my-app-public-api-client",
        ],
    );
    let web_providers =
        fs::read_to_string(destination.join("apps/web/src/app/providers.tsx")).unwrap();
    assert!(web_providers.contains("<QueryClientProvider client={client}>"));
    let web_router = fs::read_to_string(destination.join("apps/web/src/app/router.ts")).unwrap();
    assert_contains_all(
        &web_router,
        &[
            "import { routeTree } from \"@/routeTree.gen\"",
            "export function createAppRouter(",
            "context: { queryClient }",
            "defaultPreloadStaleTime: 0",
            r#"declare module "@tanstack/react-router""#,
        ],
    );
    let web_index_route =
        fs::read_to_string(destination.join("apps/web/src/routes/index.tsx")).unwrap();
    assert_contains_all(
        &web_index_route,
        &[
            r#"createFileRoute("/")"#,
            "context.queryClient.ensureQueryData",
            "errorComponent: AppError",
        ],
    );
    let web_query_client =
        fs::read_to_string(destination.join("apps/web/src/lib/query-client.ts")).unwrap();
    assert!(web_query_client.contains("retry: 1"));
}

fn assert_public_spa_vite_config(destination: &Path) {
    let web_vite_config = fs::read_to_string(destination.join("apps/web/vite.config.ts")).unwrap();
    assert_contains_all(
        &web_vite_config,
        &[
            r#"from "@tanstack/router-plugin/vite""#,
            "path.resolve(import.meta.dirname, \"./src\")",
            "autoCodeSplitting: true",
            "const devPort = Number(process.env.PORT);",
            "port: devPort",
            "process.env.API_ORIGIN",
            "process.env.JIG_DEV_API_ORIGIN",
            "firstNonEmpty(process.env.JIG_DEV_API_ORIGIN, process.env.API_ORIGIN)",
            r#""http://api.my-app.localhost:1355""#,
            r#""/api""#,
            r"target: apiOrigin",
            r#"host: "127.0.0.1""#,
            "strictPort: true",
            "clientPort: devPort",
            r#"include: ["src/**/*.test.{ts,tsx}"]"#,
            r#"include: ["src/**/*.{ts,tsx}"]"#,
        ],
    );
    assert_contains_none(
        &web_vite_config,
        &[
            "__dirname",
            "firstNonEmpty(process.env.API_ORIGIN, process.env.JIG_DEV_API_ORIGIN)",
            "apiOrigin ?",
            r#"include: ["src/App.tsx", "src/api.ts"]"#,
        ],
    );
    for excluded in [
        "src/**/*.d.ts",
        "src/**/*.test.{ts,tsx}",
        "src/test-setup.ts",
        "src/main.tsx",
        "src/routeTree.gen.ts",
        "src/components/ui/**/*.{ts,tsx}",
        "src/lib/utils.ts",
    ] {
        assert!(
            web_vite_config.contains(&format!(r#""{excluded}""#)),
            "SPA coverage must explicitly exclude {excluded}"
        );
    }
}

pub(super) fn assert_public_spa_source_and_vite(destination: &Path) {
    assert_generated_api_clients_and_spa_paths(destination);
    assert_public_spa_source_files(destination);
    assert_public_spa_vite_config(destination);
}

fn assert_public_spa_playwright(destination: &Path) {
    let web_playwright =
        fs::read_to_string(destination.join("apps/web/playwright.config.ts")).unwrap();
    assert_contains_all(
        &web_playwright,
        &[
            "cargo run --locked -p my-app-api",
            "-- --bootstrap-database",
            "my_app_web_e2e",
            r"url: `${apiOrigin}/health/ready`",
            "reuseExistingServer: false",
            "E2E_SERVER_TIMEOUT_MS",
            "E2E_GLOBAL_TIMEOUT_MS",
            "managedWebServerCount * serverTimeout + 5 * 60_000",
            "const configured = process.env[name]?.trim()",
            "E2E_WEB_PORT and E2E_API_PORT must use different ports",
            "failOnFlakyTests keeps a recovered retry red",
            r#"gracefulShutdown: { signal: "SIGTERM""#,
            r#"command: "vite --host 127.0.0.1 --strictPort""#,
            "API_ORIGIN: apiOrigin",
            "JIG_DEV_API_ORIGIN: apiOrigin",
        ],
    );
    let web_e2e = fs::read_to_string(destination.join("apps/web/e2e/app.spec.ts")).unwrap();
    assert_contains_all(
        &web_e2e,
        &[
            "page.waitForResponse",
            r#"statusResponse.headers()["x-request-id"]"#,
            r#"name: "my-app""#,
            r#"getByRole("group", { name: "Application", exact: true })"#,
            r#"locator('[data-slot="card-title"]')"#,
            r#"getByRole("group", { name: "Rust API", exact: true })"#,
            r#"serviceStatusCard.getByText("Ready", { exact: true })"#,
        ],
    );
    assert_contains_none(&web_e2e, &["page.route"]);
}

fn assert_public_spa_e2e_workflow(destination: &Path) {
    let e2e_workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    let e2e_workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&e2e_workflow)
        .expect("generated Postgres E2E workflow must be valid YAML");
    assert_eq!(e2e_workflow_yaml["jobs"]["e2e"]["runs-on"], "ubuntu-latest");
    assert_eq!(
        e2e_workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert_contains_all(
        &e2e_workflow,
        &[
            "name: Browser E2E",
            "timeout-minutes: 30",
            "outside Playwright's 15-minute default CI suite budget",
            "E2E_SERVER_TIMEOUT_MS: \"300000\"",
            "- name: \"web\"\n            dir: \"apps/web\"",
            r#"- "migrations/**""#,
            r#"- ".sqlx/**""#,
            "image: postgres:18",
            "postgres://postgres:postgres@127.0.0.1:5432/jig_e2e_${{ github.run_id }}_${{ github.run_attempt }}",
            r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#,
            r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e:install:ci"#,
            r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e"#,
            "actions/upload-artifact@v6",
        ],
    );
    assert_eq!(e2e_workflow.matches(r#"- "rust-toolchain""#).count(), 2);
    assert_eq!(
        e2e_workflow.matches(r#"- "npm-shrinkwrap.json""#).count(),
        2
    );
    assert_contains_none(
        &e2e_workflow,
        &[
            "dir: \"apps/landing\"",
            "dir: \"apps/admin-panel\"",
            "bun run test:e2e",
        ],
    );
}

pub(super) fn assert_public_spa_e2e(destination: &Path) {
    assert_public_spa_playwright(destination);
    assert_public_spa_e2e_workflow(destination);
}

pub(super) fn assert_landing_tooling(destination: &Path) {
    let landing_package =
        fs::read_to_string(destination.join("apps/landing/package.json")).unwrap();
    assert!(landing_package.contains(r#""dev": "astro dev""#));
    assert!(!landing_package.contains(" install && "));
    let landing_config =
        fs::read_to_string(destination.join("apps/landing/astro.config.mjs")).unwrap();
    assert!(landing_config.contains("process.env.HOST?.trim() || '127.0.0.1'"));
    assert!(landing_config.contains("strictPort: true"));
    assert!(landing_config.contains("Number(process.env.PORT || '4321')"));
    assert!(landing_config.contains("port < 1 || port > 65_535"));
    assert!(
        !destination
            .join("apps/landing/playwright.config.ts")
            .exists()
    );
}

// Repeat the dependency-backed proof with:
// cargo test -p jig-bootstrap tests::basic::scaffold_generation::rust_react_spa::generated_spa_coverage_counts_uncovered_future_production_modules -- --ignored --exact --nocapture
#[cfg(unix)]
#[test]
#[ignore = "requires npm registry access and a local Node/npm toolchain"]
fn generated_spa_coverage_counts_uncovered_future_production_modules() {
    use std::fmt::Write as _;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("coverage-proof");

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

    let dependencies = Command::new("scripts/check-webapps.sh")
        .args(["dependencies-bootstrap", "apps/web"])
        .env("NODE_ENV", "production")
        .env("NPM_CONFIG_OMIT", "dev")
        .current_dir(&destination)
        .output()
        .unwrap();
    assert!(
        dependencies.status.success(),
        "generated dependency bootstrap could not prepare the coverage fixture:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dependencies.stdout),
        String::from_utf8_lossy(&dependencies.stderr)
    );

    let run_coverage = || {
        Command::new("scripts/check-webapps.sh")
            .arg("coverage")
            .current_dir(&destination)
            .output()
            .unwrap()
    };
    let statement_coverage = || {
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(destination.join("apps/web/coverage/coverage-summary.json"))
                .unwrap(),
        )
        .unwrap()["total"]["statements"]["pct"]
            .as_f64()
            .unwrap()
    };

    let baseline = run_coverage();
    assert!(
        baseline.status.success(),
        "generated SPA coverage baseline failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&baseline.stdout),
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baseline_statements = statement_coverage();
    assert!(baseline_statements >= 80.0);

    let mut uncovered_module = String::new();
    for index in 0..20 {
        write!(
            uncovered_module,
            "export function uncovered{index}(value: number): number {{\n  const shifted = value + {index};\n  const doubled = shifted * 2;\n  return doubled > 10 ? doubled : 10;\n}}\n\n"
        )
        .unwrap();
    }
    fs::write(
        destination.join("apps/web/src/uncovered-production.ts"),
        uncovered_module,
    )
    .unwrap();

    let negative = run_coverage();
    assert!(!negative.status.success());
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&negative.stdout),
        String::from_utf8_lossy(&negative.stderr)
    );
    assert!(diagnostics.contains("Coverage below threshold 80%"));
    let uncovered_statements = statement_coverage();
    assert!(
        uncovered_statements < 80.0,
        "future production module stayed outside the coverage denominator: baseline {baseline_statements}%, after addition {uncovered_statements}%"
    );
}
