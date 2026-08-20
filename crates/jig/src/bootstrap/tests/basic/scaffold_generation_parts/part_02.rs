
#[test]
// agentic-loc-exception: retain the end-to-end generated-stack contract in one readable test.
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

    let context = crate::context::RepoContext::load_from(&destination).unwrap();
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
    assert!(destination.join(".env.example").exists());
    assert!(destination.join("Cargo.toml").exists());
    assert!(destination.join("apps/my-app-api/src/main.rs").exists());
    assert!(destination.join("crates/my-app-core/src/lib.rs").exists());
    assert!(destination.join("crates/my-app/src/lib.rs").exists());
    assert!(destination.join("crates/my-app/AGENTS.md").exists());
    assert!(destination.join("crates/my-app-http/src/lib.rs").exists());
    assert!(
        destination
            .join("crates/my-app-http/src/public.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-http-common/src/lib.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-admin-http/src/lib.rs")
            .exists()
    );
    assert!(
        destination
            .join("apps/my-app-admin-api/src/main.rs")
            .exists()
    );
    assert!(destination.join("crates/my-app-http/AGENTS.md").exists());
    assert!(
        destination
            .join("apps/my-app-api/src/bin/export-openapi.rs")
            .exists()
    );
    assert!(destination.join("openapi/public.json").exists());
    assert!(destination.join("openapi/admin.json").exists());
    assert!(destination.join("README.md").exists());
    assert!(destination.join("scripts/test-postgres.sh").exists());
    assert!(
        destination
            .join("crates/my-app-test-support/tests/postgres.rs")
            .exists()
    );
    assert!(destination.join("crates/my-app-db/src/lib.rs").exists());
    assert!(destination.join("crates/my-app-db/AGENTS.md").exists());
    assert!(
        destination
            .join("crates/my-app-test-support/src/lib.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/AGENTS.md")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/app.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/http.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/responses.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/src/db.rs")
            .exists()
    );
    assert!(
        destination
            .join("crates/my-app-test-support/tests/http.rs")
            .exists()
    );
    assert!(destination.join("web/package.json").exists());
    let web_gitignore = fs::read_to_string(destination.join("web/.gitignore")).unwrap();
    assert!(web_gitignore.contains("playwright-report/"));
    assert!(web_gitignore.contains("test-results/"));
    assert!(web_gitignore.contains("blob-report/"));
    assert!(web_gitignore.contains("*.tsbuildinfo"));
    assert!(destination.join("landing/astro.config.mjs").exists());
    assert!(destination.join("admin-panel/package.json").exists());
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    let workspace_package_json: serde_json::Value =
        serde_json::from_str(&workspace_package).unwrap();
    let expected_node_engine = format!(">={GENERATED_NODE_VERSION}");
    assert!(workspace_package.contains(r#""packageManager": "bun@1.3.14""#));
    assert_eq!(
        workspace_package_json["engines"]["node"].as_str(),
        Some(expected_node_engine.as_str())
    );
    assert!(workspace_package.contains(r#""admin-panel""#));
    assert!(workspace_package.contains(r#""api:generate""#));
    assert!(workspace_package.contains(r#""api:check""#));
    assert!(workspace_package.contains(r#""contract:generate""#));
    assert!(workspace_package.contains(r#""contract:check""#));
    assert!(workspace_package.contains(r#""public:artifacts:check""#));
    assert_eq!(
        workspace_package_json["scripts"]["bootstrap"],
        "bash scripts/check-webapps.sh bootstrap"
    );
    assert_eq!(
        workspace_package_json["scripts"]["test:postgres"],
        "bash scripts/test-postgres.sh"
    );
    assert_eq!(workspace_package_json["overrides"]["js-yaml"], "4.3.1");
    assert!(workspace_package.contains(r#""packages/public-api-client""#));
    assert!(workspace_package.contains(r#""packages/admin-api-client""#));
    let shared_eslint = fs::read_to_string(destination.join("eslint.config.shared.mjs")).unwrap();
    assert!(shared_eslint.contains("tseslint.configs.recommendedTypeChecked"));
    assert!(shared_eslint.contains("reactHooks.configs.flat.recommended"));
    assert!(shared_eslint.contains("testingLibrary.configs[\"flat/react\"]"));
    assert!(shared_eslint.contains("vitest.configs.recommended"));
    assert!(shared_eslint.contains("reportUnusedDisableDirectives: \"error\""));
    assert!(shared_eslint.contains("reportUnusedInlineConfigs: \"error\""));
    assert!(shared_eslint.contains("src/components/**/*.{ts,tsx}"));
    assert!(shared_eslint.contains("src/domain/**/*.{ts,tsx}"));
    let contracts_script = fs::read_to_string(destination.join("scripts/contracts.mjs")).unwrap();
    assert!(contracts_script.contains("await withStagedContracts(mode)"));
    assert!(contracts_script.contains("async function publishAtomically("));
    assert!(contracts_script.contains("async function assertPublicBoundary("));
    assert!(contracts_script.contains(r#"["tree", "--quiet", "-p", "my-app-api""#));
    assert!(contracts_script.contains(r#"cargoPackage: "my-app-api""#));
    assert!(contracts_script.contains(r#"cargoPackage: "my-app-admin-api""#));
    assert!(contracts_script.contains("Contract recovery data was preserved"));
    assert_eq!(
        fs::read_to_string(destination.join(".node-version")).unwrap(),
        format!("{GENERATED_NODE_VERSION}\n")
    );
    let web_package = fs::read_to_string(destination.join("web/package.json")).unwrap();
    let web_package_json: serde_json::Value = serde_json::from_str(&web_package).unwrap();
    assert_eq!(
        web_package_json["devDependencies"]["@types/node"].as_str(),
        Some(GENERATED_NODE_TYPES_VERSION)
    );
    assert!(web_package.contains(r#""dev": "vite""#));
    assert!(web_package.contains(r#""shadcn": "4.18.0""#));
    assert!(web_package.contains(r#""tailwindcss": "4.3.3""#));
    assert!(web_package.contains(r#""@tanstack/react-query": "5.101.4""#));
    assert!(web_package.contains(r#""@tanstack/react-router": "1.170.29""#));
    assert!(web_package.contains(r#""@tanstack/eslint-plugin-query": "5.101.4""#));
    assert!(web_package.contains(r#""@tanstack/router-plugin": "1.168.32""#));
    assert!(web_package.contains(r#""@vitest/eslint-plugin": "1.6.27""#));
    assert!(web_package.contains(r#""eslint-plugin-testing-library": "7.16.2""#));
    assert!(web_package.contains(r#""my-app-public-api-client": "*""#));
    assert!(!web_package.contains("my-app-admin-api-client"));
    assert!(web_package.contains(r#""build": "vite build && tsc -b""#));
    assert!(web_package.contains(r#""@testing-library/dom": "10.4.1""#));
    assert!(web_package.contains(r#""@playwright/test": "1.62.1""#));
    assert!(web_package.contains(r#""test:e2e": "playwright test""#));
    assert!(web_package.contains(r#""test:e2e:install": "playwright install chromium""#));
    assert!(
        web_package.contains(r#""test:e2e:install:ci": "playwright install --with-deps chromium""#)
    );
    assert!(!web_package.contains(" install && "));
    assert!(web_package.contains(r#""lint": "eslint . --max-warnings 0""#));
    assert!(web_package.contains(r#""lint:cached": "eslint . --cache --cache-location node_modules/.cache/eslint --max-warnings 0""#));
    let web_eslint = fs::read_to_string(destination.join("web/eslint.config.js")).unwrap();
    assert!(web_eslint.contains(r#"from "../eslint.config.shared.mjs""#));
    assert!(web_eslint.contains("forbiddenApiClientPackages"));
    assert!(web_eslint.contains(r#""my-app-admin-api-client""#));
    assert!(destination.join("web/src/api.ts").exists());
    assert!(
        destination
            .join("packages/public-api-client/src/generated/sdk.gen.ts")
            .exists()
    );
    assert!(
        destination
            .join("packages/admin-api-client/src/generated/sdk.gen.ts")
            .exists()
    );
    assert!(
        destination
            .join("packages/admin-api-client/src/generated/zod.gen.ts")
            .exists()
    );
    let admin_query = fs::read_to_string(
        destination.join("packages/admin-api-client/src/generated/@tanstack/react-query.gen.ts"),
    )
    .unwrap();
    assert!(admin_query.contains("getAdminStatusOptions"));
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
    let web_tsconfig_app = fs::read_to_string(destination.join("web/tsconfig.app.json")).unwrap();
    assert!(web_tsconfig_app.contains(r#""types": ["vite/client", "vitest/globals"]"#));
    assert!(!web_tsconfig_app.contains(r#""node""#));
    assert!(web_tsconfig_app.contains(r#""include": ["src"]"#));
    let web_tsconfig_node = fs::read_to_string(destination.join("web/tsconfig.node.json")).unwrap();
    assert!(web_tsconfig_node.contains(r#""types": ["node"]"#));
    assert!(web_tsconfig_node.contains(r#""playwright.config.ts""#));
    assert!(web_tsconfig_node.contains(r#""e2e""#));
    assert!(destination.join("web/components.json").exists());
    assert!(
        destination
            .join("web/src/components/ui/button.tsx")
            .exists()
    );
    assert!(destination.join("web/src/components/ui/card.tsx").exists());
    assert!(destination.join("web/src/lib/utils.ts").exists());
    let web_components = fs::read_to_string(destination.join("web/components.json")).unwrap();
    assert!(web_components.contains(r#""style": "radix-nova""#));
    let web_css = fs::read_to_string(destination.join("web/src/index.css")).unwrap();
    assert!(web_css.contains(r#"@import "tailwindcss";"#));
    assert!(web_css.contains(r#"@import "shadcn/tailwind.css";"#));
    let web_app = fs::read_to_string(destination.join("web/src/App.tsx")).unwrap();
    assert!(web_app.contains(r#"from "@/components/ui/card""#));
    assert!(web_app.contains("useSuspenseQuery(appStatusQueryOptions)"));
    assert!(web_app.contains("useQueryErrorResetBoundary()"));
    assert!(web_app.contains("appStatusQueryOptions"));
    let web_api = fs::read_to_string(destination.join("web/src/api.ts")).unwrap();
    assert!(web_api.contains("export const appStatusQueryOptions = getAppStatusOptions({"));
    assert!(web_api.contains("baseUrl: globalThis.location.origin"));
    assert!(web_api.contains("export type AppStatus = AppStatusResponse"));
    assert!(web_api.contains("my-app-public-api-client"));
    let web_providers = fs::read_to_string(destination.join("web/src/app/providers.tsx")).unwrap();
    assert!(web_providers.contains("<QueryClientProvider client={client}>"));
    let web_router = fs::read_to_string(destination.join("web/src/app/router.ts")).unwrap();
    assert!(web_router.contains("import { routeTree } from \"@/routeTree.gen\""));
    assert!(web_router.contains("export function createAppRouter("));
    assert!(web_router.contains("context: { queryClient }"));
    assert!(web_router.contains("defaultPreloadStaleTime: 0"));
    assert!(web_router.contains(r#"declare module "@tanstack/react-router""#));
    let web_index_route = fs::read_to_string(destination.join("web/src/routes/index.tsx")).unwrap();
    assert!(web_index_route.contains(r#"createFileRoute("/")"#));
    assert!(web_index_route.contains("context.queryClient.ensureQueryData"));
    assert!(web_index_route.contains("errorComponent: AppError"));
    let web_query_client =
        fs::read_to_string(destination.join("web/src/lib/query-client.ts")).unwrap();
    assert!(web_query_client.contains("retry: 1"));
    let web_vite_config = fs::read_to_string(destination.join("web/vite.config.ts")).unwrap();
    assert!(web_vite_config.contains(r#"from "@tanstack/router-plugin/vite""#));
    assert!(web_vite_config.contains("path.resolve(import.meta.dirname, \"./src\")"));
    assert!(!web_vite_config.contains("__dirname"));
    assert!(web_vite_config.contains("autoCodeSplitting: true"));
    assert!(web_vite_config.contains("const devPort = Number(process.env.PORT);"));
    assert!(web_vite_config.contains("port: devPort"));
    assert!(web_vite_config.contains("process.env.API_ORIGIN"));
    assert!(web_vite_config.contains("process.env.JIG_DEV_API_ORIGIN"));
    assert!(
        web_vite_config
            .contains("firstNonEmpty(process.env.JIG_DEV_API_ORIGIN, process.env.API_ORIGIN)")
    );
    assert!(
        !web_vite_config
            .contains("firstNonEmpty(process.env.API_ORIGIN, process.env.JIG_DEV_API_ORIGIN)")
    );
    assert!(web_vite_config.contains(r#""http://api.my-app.localhost:1355""#));
    assert!(web_vite_config.contains(r#""/api""#));
    assert!(web_vite_config.contains(r"target: apiOrigin"));
    assert!(!web_vite_config.contains("apiOrigin ?"));
    assert!(web_vite_config.contains(r#"host: "127.0.0.1""#));
    assert!(web_vite_config.contains("strictPort: true"));
    assert!(web_vite_config.contains("clientPort: devPort"));
    assert!(
        web_vite_config.contains(r#"include: ["src/**/*.test.{ts,tsx}"]"#),
        "Vitest must not collect Playwright specs"
    );
    assert!(web_vite_config.contains(r#"include: ["src/**/*.{ts,tsx}"]"#));
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
    assert!(
        !web_vite_config.contains(r#"include: ["src/App.tsx", "src/api.ts"]"#),
        "future production modules must not escape the coverage denominator"
    );
    let web_playwright = fs::read_to_string(destination.join("web/playwright.config.ts")).unwrap();
    assert!(web_playwright.contains("cargo run --locked -p my-app-api"));
    assert!(web_playwright.contains("-- --bootstrap-database"));
    assert!(web_playwright.contains("my_app_web_e2e"));
    assert!(web_playwright.contains(r"url: `${apiOrigin}/health/ready`"));
    assert!(web_playwright.contains("reuseExistingServer: false"));
    assert!(web_playwright.contains("E2E_SERVER_TIMEOUT_MS"));
    assert!(web_playwright.contains("E2E_GLOBAL_TIMEOUT_MS"));
    assert!(web_playwright.contains("managedWebServerCount * serverTimeout + 5 * 60_000"));
    assert!(web_playwright.contains("const configured = process.env[name]?.trim()"));
    assert!(web_playwright.contains("E2E_WEB_PORT and E2E_API_PORT must use different ports"));
    assert!(web_playwright.contains("failOnFlakyTests keeps a recovered retry red"));
    assert!(web_playwright.contains(r#"gracefulShutdown: { signal: "SIGTERM""#));
    assert!(web_playwright.contains(r#"command: "vite --host 127.0.0.1 --strictPort""#));
    assert!(web_playwright.contains("API_ORIGIN: apiOrigin"));
    assert!(web_playwright.contains("JIG_DEV_API_ORIGIN: apiOrigin"));
    let web_e2e = fs::read_to_string(destination.join("web/e2e/app.spec.ts")).unwrap();
    assert!(web_e2e.contains("page.waitForResponse"));
    assert!(web_e2e.contains(r#"statusResponse.headers()["x-request-id"]"#));
    assert!(web_e2e.contains(r#"name: "my-app""#));
    assert!(web_e2e.contains(r#"getByRole("group", { name: "Application", exact: true })"#));
    assert!(web_e2e.contains(r#"locator('[data-slot="card-title"]')"#));
    assert!(web_e2e.contains(r#"getByRole("group", { name: "Rust API", exact: true })"#));
    assert!(web_e2e.contains(r#"serviceStatusCard.getByText("Ready", { exact: true })"#));
    assert!(!web_e2e.contains("page.route"));
    let e2e_workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    let e2e_workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&e2e_workflow)
        .expect("generated Postgres E2E workflow must be valid YAML");
    assert_eq!(e2e_workflow_yaml["jobs"]["e2e"]["runs-on"], "ubuntu-latest");
    assert_eq!(
        e2e_workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(e2e_workflow.contains("name: Browser E2E"));
    assert!(e2e_workflow.contains("timeout-minutes: 30"));
    assert!(e2e_workflow.contains("outside Playwright's 15-minute default CI suite budget"));
    assert_eq!(e2e_workflow.matches(r#"- "rust-toolchain""#).count(), 2);
    assert_eq!(
        e2e_workflow.matches(r#"- "npm-shrinkwrap.json""#).count(),
        2
    );
    assert!(e2e_workflow.contains("E2E_SERVER_TIMEOUT_MS: \"300000\""));
    assert!(e2e_workflow.contains("- name: \"web\"\n            dir: \"web\""));
    assert!(!e2e_workflow.contains("dir: landing"));
    assert!(!e2e_workflow.contains("dir: admin-panel"));
    assert!(e2e_workflow.contains(r#"- "migrations/**""#));
    assert!(e2e_workflow.contains(r#"- ".sqlx/**""#));
    assert!(e2e_workflow.contains("image: postgres:18"));
    assert!(e2e_workflow.contains(
        "postgres://postgres:postgres@127.0.0.1:5432/jig_e2e_${{ github.run_id }}_${{ github.run_attempt }}"
    ));
    assert!(e2e_workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
    assert!(
        e2e_workflow
            .contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e:install:ci"#)
    );
    assert!(e2e_workflow.contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e"#));
    assert!(!e2e_workflow.contains("bun run test:e2e"));
    assert!(e2e_workflow.contains("actions/upload-artifact@v6"));
    let rust_workflow =
        fs::read_to_string(destination.join(".github/workflows/rust-tests.yml")).unwrap();
    let rust_workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&rust_workflow).unwrap();
    for job in ["fmt", "clippy", "test"] {
        assert_eq!(rust_workflow_yaml["jobs"][job]["runs-on"], "macos-14");
    }
    for event in ["pull_request", "push"] {
        let paths = rust_workflow_yaml["on"][event]["paths"].as_array().unwrap();
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
                "no-mod-rs",
                "rust-file-loc",
                "sqlx-unchecked-queries",
                "migration-immutability",
            ][..],
        ),
    ] {
        let workflow =
            fs::read_to_string(destination.join(".github/workflows").join(workflow_name)).unwrap();
        let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
        for job in jobs {
            assert_eq!(workflow["jobs"][job]["runs-on"], "macos-14");
        }
    }
    let landing_package = fs::read_to_string(destination.join("landing/package.json")).unwrap();
    assert!(landing_package.contains(r#""dev": "astro dev""#));
    assert!(!landing_package.contains(" install && "));
    let landing_config = fs::read_to_string(destination.join("landing/astro.config.mjs")).unwrap();
    assert!(landing_config.contains("process.env.HOST?.trim() || '127.0.0.1'"));
    assert!(landing_config.contains("strictPort: true"));
    assert!(landing_config.contains("Number(process.env.PORT || '4321')"));
    assert!(landing_config.contains("port < 1 || port > 65_535"));
    assert!(!destination.join("landing/playwright.config.ts").exists());
    let admin_package = fs::read_to_string(destination.join("admin-panel/package.json")).unwrap();
    let admin_package_json: serde_json::Value = serde_json::from_str(&admin_package).unwrap();
    assert_eq!(
        admin_package_json["devDependencies"]["@types/node"].as_str(),
        Some(GENERATED_NODE_TYPES_VERSION)
    );
    assert!(admin_package.contains(r#""shadcn": "4.18.0""#));
    assert!(admin_package.contains(r#""tailwindcss": "4.3.3""#));
    assert!(admin_package.contains(r#""@tanstack/react-query": "5.101.4""#));
    assert!(admin_package.contains(r#""@tanstack/react-router": "1.170.29""#));
    assert!(admin_package.contains(r#""@tanstack/eslint-plugin-query": "5.101.4""#));
    assert!(admin_package.contains(r#""@tanstack/router-plugin": "1.168.32""#));
    assert!(admin_package.contains(r#""@vitest/eslint-plugin": "1.6.27""#));
    assert!(admin_package.contains(r#""eslint-plugin-testing-library": "7.16.2""#));
    assert!(admin_package.contains(r#""my-app-public-api-client": "*""#));
    assert!(admin_package.contains(r#""my-app-admin-api-client": "*""#));
    assert!(admin_package.contains(r#""build": "vite build && tsc -b""#));
    assert!(!admin_package.contains("react-router-dom"));
    assert!(admin_package.contains(r#""@testing-library/dom": "10.4.1""#));
    assert!(admin_package.contains(r#""lint": "eslint . --max-warnings 0 && prettier --check .""#));
    assert!(admin_package.contains(r#""lint:cached": "eslint . --cache --cache-location node_modules/.cache/eslint --max-warnings 0 && prettier --check .""#));
    assert!(admin_package.contains(r#""format": "prettier --write .""#));
    assert!(admin_package.contains(r#""format:check": "prettier --check .""#));
    assert!(!admin_package.contains("@playwright/test"));
    let admin_eslint =
        fs::read_to_string(destination.join("admin-panel/eslint.config.js")).unwrap();
    assert!(admin_eslint.contains(r#"from "../eslint.config.shared.mjs""#));
    assert!(!admin_eslint.contains("forbiddenApiClientPackages"));
    let admin_readme = fs::read_to_string(destination.join("admin-panel/README.md")).unwrap();
    assert!(admin_readme.contains("real-backend Playwright starter for product SPA roles only"));
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
    let admin_index = fs::read_to_string(destination.join("admin-panel/index.html")).unwrap();
    let theme_storage_key = "admin-panel-theme";
    let theme_bootstrap = admin_index
        .find(&format!("const themeStorageKey = \"{theme_storage_key}\""))
        .unwrap();
    let react_entry = admin_index.find("/src/main.tsx").unwrap();
    assert!(theme_bootstrap < react_entry);
    assert_eq!(admin_index.matches(theme_storage_key).count(), 1);
    assert!(admin_index.contains("localStorage.getItem(themeStorageKey)"));
    assert!(admin_index.contains("<!-- prettier-ignore -->\n    <title>Admin Panel</title>"));
    assert!(admin_index.contains("prefers-color-scheme: dark"));
    assert!(admin_index.contains("root.style.colorScheme = resolved"));
    let theme_provider =
        fs::read_to_string(destination.join("admin-panel/src/components/theme-provider.tsx"))
            .unwrap();
    assert!(theme_provider.contains("storage = window.localStorage"));
    assert!(theme_provider.contains("if (event.storageArea !== storage)"));
    let providers =
        fs::read_to_string(destination.join("admin-panel/src/app/providers.tsx")).unwrap();
    assert!(providers.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(providers.matches(theme_storage_key).count(), 1);
    assert!(providers.contains("storageKey={themeStorageKey}"));
    assert!(providers.contains("<QueryClientProvider client={client}>"));
    let admin_router =
        fs::read_to_string(destination.join("admin-panel/src/app/router.ts")).unwrap();
    assert!(admin_router.contains("import { routeTree } from \"@/routeTree.gen\""));
    assert!(admin_router.contains("export function createAppRouter("));
    assert!(admin_router.contains("context: { queryClient }"));
    assert!(admin_router.contains("defaultPreloadStaleTime: 0"));
    assert!(admin_router.contains(r#"declare module "@tanstack/react-router""#));
    let admin_shell =
        fs::read_to_string(destination.join("admin-panel/src/app/shell.tsx")).unwrap();
    assert!(admin_shell.contains(r#"from "@tanstack/react-router""#));
    assert!(admin_shell.contains("const appTitle = \"Admin Panel\""));
    assert!(admin_shell.contains(">{appTitle}</p>"));
    let admin_sidebar =
        fs::read_to_string(destination.join("admin-panel/src/components/app-sidebar.tsx")).unwrap();
    assert!(admin_sidebar.contains("const appName = \"my-app\""));
    assert_eq!(admin_sidebar.matches("\"my-app\"").count(), 1);
    assert!(admin_sidebar.contains(">{appName}</span>"));
    assert!(admin_sidebar.contains(r#"from "@tanstack/react-router""#));
    assert!(admin_sidebar.contains("useRouterState({"));
    let admin_overview_test = fs::read_to_string(
        destination.join("admin-panel/src/features/overview/overview-page.test.tsx"),
    )
    .unwrap();
    assert!(admin_overview_test.contains("const expectedAppName = \"my-app\""));
    assert_eq!(admin_overview_test.matches("\"my-app\"").count(), 1);
    assert!(admin_overview_test.contains("name: expectedAppName"));
    assert!(admin_overview_test.contains("screen.findAllByText(expectedAppName)"));
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
            &destination,
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
    assert!(!postgres_script.contains("seq 1 60"));
    assert!(postgres_script.contains("attempt=$((attempt + 1))"));
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
