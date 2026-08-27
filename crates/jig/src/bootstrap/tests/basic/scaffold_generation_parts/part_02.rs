
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
    let jig_config: toml::Value =
        toml::from_str(&fs::read_to_string(destination.join(".jig.toml")).unwrap()).unwrap();
    assert_eq!(jig_config["application_contracts_enabled"].as_bool(), Some(true));
    assert_eq!(
        jig_config["commands"]["application_contract_check_command"].as_str(),
        Some("scripts/check-webapps.sh application-contracts")
    );
    assert_eq!(
        jig_config["commands"]["public_artifacts_check_command"].as_str(),
        Some("scripts/check-webapps.sh public-artifacts")
    );
    let work_gates = jig_config["work"]["gates"].as_array().unwrap();
    let application_gate = work_gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("application-contracts"))
        .unwrap();
    assert_eq!(
        application_gate["tool"].as_str(),
        Some("jig.application_contract_check")
    );
    assert!(
        application_gate["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("scripts/contracts.mjs"))
    );
    for public_docs in ["docs/public/**", "public-docs/**"] {
        assert!(
            application_gate["paths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path.as_str() == Some(public_docs)),
            "application contract scope omitted {public_docs}"
        );
    }
    for app_path in ["web/**", "landing/**", "admin-panel/**"] {
        assert!(
            application_gate["paths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path.as_str() == Some(app_path)),
            "application contract scope omitted {app_path}"
        );
    }
    let public_gate = work_gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("public-artifacts"))
        .unwrap();
    assert_eq!(
        public_gate["tool"].as_str(),
        Some("jig.public_artifacts_check")
    );
    assert!(
        public_gate["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("web/**"))
    );
    for public_docs in ["docs/public/**", "public-docs/**"] {
        assert!(
            public_gate["paths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path.as_str() == Some(public_docs)),
            "public artifact scope omitted {public_docs}"
        );
    }
    assert!(
        public_gate["paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| path.as_str() != Some("admin-panel/**"))
    );
    let web_build_gate = work_gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("typescript-web-build"))
        .unwrap();
    let rust_test_gate = work_gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("rust-tests"))
        .unwrap();
    let sqlx_gate = work_gates
        .iter()
        .find(|gate| gate["id"].as_str() == Some("sqlx"))
        .unwrap();
    assert!(
        rust_test_gate["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("migrations/**")),
        "Rust tests must own the migration tree embedded by sqlx::migrate!"
    );
    for authority in crate::bootstrap::renderer::FRONTEND_GATE_SHARED_PATHS {
        for gate in [application_gate, public_gate, web_build_gate] {
            assert!(
                gate["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path.as_str() == Some(authority)),
                "gate {} omitted frontend authority {authority}",
                gate["id"]
            );
        }
    }
    for authority in crate::bootstrap::renderer::RUST_GATE_COMMAND_AUTHORITY_PATHS {
        for gate in [application_gate, public_gate, sqlx_gate] {
            assert!(
                gate["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path.as_str() == Some(authority)),
                "gate {} omitted Rust authority {authority}",
                gate["id"]
            );
        }
    }
    assert!(
        public_gate["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str() == Some("admin-panel/package.json"))
    );
    let jig_contract: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(destination.join(".agent/jig-contract.json")).unwrap(),
    )
    .unwrap();
    for tool in [
        "jig.application_contract_check",
        "jig.public_artifacts_check",
    ] {
        assert!(
            jig_contract["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"].as_str() == Some(tool))
        );
    }
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
    assert!(contracts_script.starts_with(
        "// jig-application-contract-checker: v1 modes=check,public-check\n"
    ));
    assert!(contracts_script.contains("await withStagedContracts(mode)"));
    assert!(contracts_script.contains("async function publishAtomically("));
    assert!(contracts_script.contains("async function assertPublicBoundary("));
    assert!(contracts_script.contains(r#"["tree", "--quiet", "-p", "my-app-api""#));
    assert!(contracts_script.contains(r#"cargoPackage: "my-app-api""#));
    assert!(contracts_script.contains(r#"cargoPackage: "my-app-admin-api""#));
    assert!(contracts_script.contains("Contract recovery data was preserved"));
    let web_checker = fs::read_to_string(destination.join("scripts/check-webapps.sh")).unwrap();
    assert!(web_checker.contains("run_application_contract_check"));
    let dependency_preparation = web_checker
        .split("prepare_application_contract_dependencies() {")
        .nth(1)
        .unwrap()
        .split("run_application_contract_check() {")
        .next()
        .unwrap();
    assert!(dependency_preparation.contains("install_dependencies \".\""));
    for app_dir in ["web", "landing", "admin-panel"] {
        assert!(
            dependency_preparation.contains(&format!("app_dir=\"{app_dir}\"")),
            "application contract dependency preparation omitted {app_dir}"
        );
    }
    assert!(dependency_preparation.contains("prepared_scopes"));
    assert!(dependency_preparation.contains("prepared_scopes=(\"/\")"));
    #[cfg(unix)]
    {
        let function_start = web_checker
            .find("prepare_application_contract_dependencies() {")
            .unwrap();
        let function_end = web_checker[function_start..]
            .find("\n}\n\nrun_application_contract_check() {")
            .map(|offset| function_start + offset + 2)
            .unwrap();
        let function = &web_checker[function_start..function_end];
        let shell_fixture = tempdir().unwrap();
        let script = format!(
            "set -u\ndependency_scope() {{ printf '%s\\n' \"$1\"; }}\ninstall_dependencies() {{ printf '%s\\n' \"$1\"; }}\n{function}\nprepare_application_contract_dependencies\n"
        );
        let output = Command::new("bash")
            .current_dir(shell_fixture.path())
            .arg("--noprofile")
            .arg("--norc")
            .arg("-c")
            .arg(script)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "independent dependency preparation failed under nounset\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "web\nlanding\nadmin-panel\n"
        );
    }
    assert_eq!(
        web_checker
            .matches("prepare_application_contract_dependencies\n")
            .count(),
        2,
        "application and public contract checks must share dependency preparation"
    );
    assert!(web_checker.contains("run_public_artifacts_check"));
    let public_artifacts_check = web_checker
        .split_once("run_public_artifacts_check() {")
        .unwrap()
        .1
        .split_once("\n}\n\ncase \"$mode\"")
        .unwrap()
        .0;
    assert!(public_artifacts_check.contains(r#"run_check "web" "80" "build:bundle""#));
    assert!(public_artifacts_check.contains(r#"run_check "landing" "0" "build:bundle""#));
    assert!(!public_artifacts_check.contains(r#"run_check "admin-panel" "80" "build:bundle""#));
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

    assert_generated_backend(&destination);
}
