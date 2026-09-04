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
    assert!(database_config < setup);
    let context = crate::context::RepoContext::load_from(destination).unwrap();
    let agent_map_check = crate::policy::run_check(
        &context,
        crate::policy::PolicyCheckCommand::AgentMap(crate::policy::AgentMapInput {
            map_path: PathBuf::from("agent-map.md"),
        }),
    )
    .unwrap();
    assert_eq!(agent_map_check["ok"], true);
    assert_eq!(agent_map_check["agents"], 7);
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
        crate::policy::run_check(&context, crate::policy::PolicyCheckCommand::AgentGuides).unwrap();
    assert_eq!(agent_guides_check["ok"], true);
    assert_eq!(agent_guides_check["guide_count"], 6);
    assert!(
        agent_guides_check["missing_entry_ref"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

fn assert_rust_react_report_and_paths(destination: &Path, output: &serde_json::Value) {
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
            "web/package.json",
        ],
    );
}

fn assert_workspace_and_contract_tooling(destination: &Path) {
    let web_gitignore = fs::read_to_string(destination.join("web/.gitignore")).unwrap();
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
        &["landing/astro.config.mjs", "admin-panel/package.json"],
    );
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    let workspace_package_json: serde_json::Value =
        serde_json::from_str(&workspace_package).unwrap();
    let expected_node_engine = format!(">={GENERATED_NODE_VERSION}");
    assert_contains_all(
        &workspace_package,
        &[
            r#""packageManager": "bun@1.3.14""#,
            r#""admin-panel""#,
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
        "bash scripts/check-webapps.sh bootstrap"
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

fn assert_public_spa_package_and_clients(destination: &Path) {
    let web_package = fs::read_to_string(destination.join("web/package.json")).unwrap();
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
    let web_eslint = fs::read_to_string(destination.join("web/eslint.config.js")).unwrap();
    assert_contains_all(
        &web_eslint,
        &[
            r#"from "../eslint.config.shared.mjs""#,
            "forbiddenApiClientPackages",
            r#""my-app-admin-api-client""#,
        ],
    );
    assert_paths_exist(
        destination,
        &[
            "web/src/api.ts",
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
    assert!(destination.join("web/src/app/providers.tsx").exists());
    assert!(destination.join("web/src/app/router-context.ts").exists());
    assert!(destination.join("web/src/app/router.ts").exists());
    assert!(destination.join("web/src/lib/query-client.ts").exists());
    assert!(destination.join("web/src/routes/__root.tsx").exists());
    assert!(destination.join("web/src/routes/index.tsx").exists());
    assert!(destination.join("web/src/routeTree.gen.ts").exists());
    assert!(destination.join("web/playwright.config.ts").exists());
    assert!(destination.join("web/e2e/app.spec.ts").exists());
    assert!(destination.join("web/tsconfig.app.json").exists());
    assert!(destination.join("web/tsconfig.node.json").exists());
}

fn assert_public_spa_source_files(destination: &Path) {
    let web_tsconfig_app = fs::read_to_string(destination.join("web/tsconfig.app.json")).unwrap();
    assert_contains_all(
        &web_tsconfig_app,
        &[
            r#""types": ["vite/client", "vitest/globals"]"#,
            r#""include": ["src"]"#,
        ],
    );
    assert_contains_none(&web_tsconfig_app, &[r#""node""#]);
    let web_tsconfig_node = fs::read_to_string(destination.join("web/tsconfig.node.json")).unwrap();
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
            "web/components.json",
            "web/src/components/ui/button.tsx",
            "web/src/components/ui/card.tsx",
            "web/src/lib/utils.ts",
        ],
    );
    let web_components = fs::read_to_string(destination.join("web/components.json")).unwrap();
    assert!(web_components.contains(r#""style": "radix-nova""#));
    let web_css = fs::read_to_string(destination.join("web/src/index.css")).unwrap();
    assert_contains_all(
        &web_css,
        &[
            r#"@import "tailwindcss";"#,
            r#"@import "shadcn/tailwind.css";"#,
        ],
    );
    let web_app = fs::read_to_string(destination.join("web/src/App.tsx")).unwrap();
    assert_contains_all(
        &web_app,
        &[
            r#"from "@/components/ui/card""#,
            "useSuspenseQuery(appStatusQueryOptions)",
            "useQueryErrorResetBoundary()",
            "appStatusQueryOptions",
        ],
    );
    let web_api = fs::read_to_string(destination.join("web/src/api.ts")).unwrap();
    assert_contains_all(
        &web_api,
        &[
            "export const appStatusQueryOptions = getAppStatusOptions({",
            "baseUrl: globalThis.location.origin",
            "export type AppStatus = AppStatusResponse",
            "my-app-public-api-client",
        ],
    );
    let web_providers = fs::read_to_string(destination.join("web/src/app/providers.tsx")).unwrap();
    assert!(web_providers.contains("<QueryClientProvider client={client}>"));
    let web_router = fs::read_to_string(destination.join("web/src/app/router.ts")).unwrap();
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
    let web_index_route = fs::read_to_string(destination.join("web/src/routes/index.tsx")).unwrap();
    assert_contains_all(
        &web_index_route,
        &[
            r#"createFileRoute("/")"#,
            "context.queryClient.ensureQueryData",
            "errorComponent: AppError",
        ],
    );
    let web_query_client =
        fs::read_to_string(destination.join("web/src/lib/query-client.ts")).unwrap();
    assert!(web_query_client.contains("retry: 1"));
}

fn assert_public_spa_vite_config(destination: &Path) {
    let web_vite_config = fs::read_to_string(destination.join("web/vite.config.ts")).unwrap();
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

fn assert_public_spa_source_and_vite(destination: &Path) {
    assert_generated_api_clients_and_spa_paths(destination);
    assert_public_spa_source_files(destination);
    assert_public_spa_vite_config(destination);
}

fn assert_public_spa_playwright(destination: &Path) {
    let web_playwright = fs::read_to_string(destination.join("web/playwright.config.ts")).unwrap();
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
    let web_e2e = fs::read_to_string(destination.join("web/e2e/app.spec.ts")).unwrap();
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
            "- name: \"web\"\n            dir: \"web\"",
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
        &["dir: landing", "dir: admin-panel", "bun run test:e2e"],
    );
}

fn assert_public_spa_e2e(destination: &Path) {
    assert_public_spa_playwright(destination);
    assert_public_spa_e2e_workflow(destination);
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

fn assert_landing_tooling(destination: &Path) {
    let landing_package = fs::read_to_string(destination.join("landing/package.json")).unwrap();
    assert!(landing_package.contains(r#""dev": "astro dev""#));
    assert!(!landing_package.contains(" install && "));
    let landing_config = fs::read_to_string(destination.join("landing/astro.config.mjs")).unwrap();
    assert!(landing_config.contains("process.env.HOST?.trim() || '127.0.0.1'"));
    assert!(landing_config.contains("strictPort: true"));
    assert!(landing_config.contains("Number(process.env.PORT || '4321')"));
    assert!(landing_config.contains("port < 1 || port > 65_535"));
    assert!(!destination.join("landing/playwright.config.ts").exists());
}

fn assert_admin_package_tooling(destination: &Path) {
    let admin_package = fs::read_to_string(destination.join("admin-panel/package.json")).unwrap();
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
        fs::read_to_string(destination.join("admin-panel/eslint.config.js")).unwrap();
    assert!(admin_eslint.contains(r#"from "../eslint.config.shared.mjs""#));
    assert!(!admin_eslint.contains("forbiddenApiClientPackages"));
    let admin_readme = fs::read_to_string(destination.join("admin-panel/README.md")).unwrap();
    assert!(admin_readme.contains("real-backend Playwright starter for product SPA roles only"));
}

fn assert_admin_vite_config(destination: &Path) {
    let admin_vite_config =
        fs::read_to_string(destination.join("admin-panel/vite.config.ts")).unwrap();
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

fn assert_landing_and_admin_tooling(destination: &Path) {
    assert_landing_tooling(destination);
    assert_admin_package_tooling(destination);
    assert_admin_vite_config(destination);
}
