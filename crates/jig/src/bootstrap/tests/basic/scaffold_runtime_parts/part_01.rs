#[test]
fn scaffold_defaults_to_web_frontend_and_no_db() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["db"], "none");
    assert_eq!(report["frontends"][0]["name"], "web");
    assert_eq!(report["frontends"][0]["kind"], "vite");
    assert_eq!(report["frontends"][0]["role"], "spa");
    assert!(temp.path().join("web/package.json").exists());
    let has_db_crate = fs::read_dir(temp.path().join("crates"))
        .unwrap()
        .any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with("-db")
        });
    assert!(!has_db_crate);
    let cargo_toml = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(!cargo_toml.contains("sqlx ="));
    assert!(cargo_toml.contains("\"signal\", \"time\""));
    assert!(cargo_toml.ends_with('\n'));
    let repo_name = report["repo_name"].as_str().unwrap();
    let module_name = repo_name.replace('-', "_");
    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    assert_eq!(
        env_example,
        format!(
            "BIND_ADDR=127.0.0.1:3000\nRUST_LOG={module_name}=info,{module_name}_api=info,tower_http=info\n"
        )
    );
    let playwright = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    assert!(playwright.contains("const backendCommand = \"cargo run --locked"));
    assert!(!playwright.contains("-- --bootstrap-database"));
    assert!(!playwright.contains("E2E_DATABASE_URL"));
    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    assert!(!workflow.contains("image: postgres"));
    assert!(!workflow.contains("E2E_DATABASE_URL"));
    assert!(!workflow.contains("SQLX_OFFLINE"));
    let api_main = fs::read_to_string(
        temp.path()
            .join("apps")
            .join(format!("{repo_name}-api/src/main.rs")),
    )
    .unwrap();
    assert!(
        api_main
            .contains("    parse_command()?;\n    let config = app_crate::AppConfig::from_env()")
    );
    assert!(!api_main.contains("let command = parse_command()?;"));
    assert!(!api_main.contains("--bootstrap-database"));
    assert!(api_main.contains("match (arguments.next(), arguments.next())"));
    assert!(api_main.contains("unexpected API argument"));
    assert!(!api_main.contains("args_os().any"));

    let output = std::process::Command::new("cargo")
        .args(["fmt", "--all", "--", "--check"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo fmt failed for the no-database scaffold\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scaffold_playwright_api_environment_overrides_hostile_inherited_bindings() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    plan.write(temp.path(), false).unwrap();

    let config = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    let api_server_config = config
        .split_once(r#"name: "Rust API""#)
        .unwrap()
        .1
        .split_once(r#"name: "Vite web""#)
        .unwrap()
        .0;
    for fixed_binding in [
        r#"HOST: "127.0.0.1""#,
        "PORT: String(apiPort)",
        r"BIND_ADDR: `127.0.0.1:${apiPort}`",
    ] {
        assert!(
            api_server_config.contains(fixed_binding),
            "hostile inherited bindings must be replaced by {fixed_binding}"
        );
    }
    assert!(!api_server_config.contains("process.env.HOST"));
    assert!(!api_server_config.contains("process.env.PORT"));
}

#[test]
fn scaffold_e2e_workflow_uses_each_package_manager_portably() {
    let temp = tempdir().unwrap();
    for (package_manager, setup, run, root_cache_locks, app_cache_locks) in [
        (
            "bun",
            "oven-sh/setup-bun@v2",
            "bun run",
            "'bun.lock', 'bun.lockb'",
            "format('{0}/bun.lock', matrix.app.dir), format('{0}/bun.lockb', matrix.app.dir)",
        ),
        (
            "npm",
            "npm install --global npm@",
            "npm run",
            "'npm-shrinkwrap.json', 'package-lock.json'",
            "format('{0}/npm-shrinkwrap.json', matrix.app.dir), format('{0}/package-lock.json', matrix.app.dir)",
        ),
        (
            "pnpm",
            r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec"#,
            "pnpm run",
            "'pnpm-lock.yaml'",
            "format('{0}/pnpm-lock.yaml', matrix.app.dir)",
        ),
        (
            "yarn",
            r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec"#,
            "yarn run",
            "'yarn.lock'",
            "format('{0}/yarn.lock', matrix.app.dir)",
        ),
    ] {
        let destination = temp.path().join(package_manager);
        fs::create_dir(&destination).unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                ci_github_runner: Some("macos-14".into()),
                web_package_manager: Some(package_manager.into()),
                ..AnswerOpts::default()
            },
            &destination,
        )
        .unwrap()
        .unwrap();

        plan.write(&destination, false).unwrap();

        let workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
        let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow)
            .expect("generated E2E workflow must be valid YAML");
        assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], "macos-14");
        assert_eq!(
            workflow_yaml["jobs"]["e2e"]["defaults"]["run"]["shell"],
            "bash"
        );
        assert!(workflow.contains(setup), "missing {package_manager} setup");
        assert!(workflow.contains("Classic required status checks can remain pending"));
        assert!(workflow.contains("Bootstrap Node for dependency metadata"));
        assert!(workflow.contains("scripts/check-webapps.sh node-version-file"));
        assert!(workflow.contains("status=$?"));
        assert!(workflow.contains("if [ \"$status\" -eq 1 ]"));
        assert!(workflow.contains("exit \"$status\""));
        assert!(!workflow.contains("if ! node_version_file="));
        assert!(workflow.contains("${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"));
        assert!(workflow.contains("mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\""));
        assert!(workflow.contains("set -o noclobber"));
        assert!(!workflow.contains("> .node-version"));
        assert!(workflow.contains("APP_DIR: ${{ matrix.app.dir }}"));
        assert_eq!(workflow.matches(r#"- "rust-toolchain""#).count(), 2);
        assert!(workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
        assert!(workflow.contains(
            "PLAYWRIGHT_BROWSERS_PATH: ${{ github.workspace }}/.agent/tmp/ms-playwright"
        ));
        assert!(workflow.contains("- name: Cache Playwright Chromium"));
        assert!(workflow.contains("path: ${{ env.PLAYWRIGHT_BROWSERS_PATH }}"));
        assert!(workflow.contains("playwright-chromium-${{ hashFiles("));
        assert!(
            workflow
                .contains("hashFiles('package.json', format('{0}/package.json', matrix.app.dir),")
        );
        assert!(
            workflow.contains(root_cache_locks),
            "Playwright cache key is missing root {package_manager} lockfiles"
        );
        assert!(
            workflow.contains(app_cache_locks),
            "Playwright cache key is missing app {package_manager} lockfiles"
        );
        if package_manager == "npm" {
            for cache_path in [
                "            npm-shrinkwrap.json",
                "            package-lock.json",
                "            ${{ matrix.app.dir }}/npm-shrinkwrap.json",
                "            ${{ matrix.app.dir }}/package-lock.json",
            ] {
                assert!(
                    workflow.contains(cache_path),
                    "npm dependency cache is missing {cache_path}"
                );
            }
            assert_eq!(workflow.matches(r#"- "npm-shrinkwrap.json""#).count(), 2);
        }
        assert!(workflow.contains(r#"- "**/.yarnrc.yml""#));
        assert!(workflow.contains(r#"- "**/.yarn/**""#));
        assert!(workflow.contains(r#"- "**/.node-version""#));
        assert!(workflow.contains(r#"- "**/.npmrc""#));
        assert!(workflow.contains("${{ matrix.app.dir }}/"));
        assert!(
            workflow
                .contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e:install:ci"#)
        );
        assert!(workflow.contains(r#"scripts/check-webapps.sh run-script "$APP_DIR" test:e2e"#));
        assert!(
            !workflow.contains(&format!("{run} test:e2e")),
            "{package_manager} E2E must use the managed checker launcher"
        );
        assert!(!workflow.contains(r#"cd "$APP_DIR" &&"#));
        assert!(!workflow.contains("test:e2e:install --"));
    }
}

#[test]
fn scaffold_omits_e2e_workflow_without_spa_frontends() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![
                parse_scaffold_frontend("docs:astro").unwrap(),
                parse_scaffold_frontend("operations:admin").unwrap(),
            ],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(
        !plan
            .output_paths()
            .iter()
            .any(|path| path == Path::new(".github/workflows/e2e.yml"))
    );
    plan.write(temp.path(), false).unwrap();
    assert!(!temp.path().join(".github/workflows/e2e.yml").exists());
}

#[test]
fn scaffold_named_ready_scopes_the_live_status_badge() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("ready:spa").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();
    let app = fs::read_to_string(temp.path().join("ready/src/App.tsx")).unwrap();
    let spec = fs::read_to_string(temp.path().join("ready/e2e/app.spec.ts")).unwrap();

    assert!(app.contains(r#"aria-labelledby="service-status-card-label""#));
    assert!(app.contains(r#"id="service-status-card-label">Rust API"#));
    assert!(spec.contains(r#"getByRole("heading", { name: "Ready" })"#));
    assert!(spec.contains(r#"getByRole("group", { name: "Rust API", exact: true })"#));
    assert!(spec.contains(r#"serviceStatusCard.getByText("Ready", { exact: true })"#));
    assert!(!spec.contains(r#"page.getByText("Ready", { exact: true })"#));
}

#[test]
fn scaffold_e2e_workflow_serializes_dynamic_yaml_scalars() {
    let temp = tempdir().unwrap();
    let default_branch = r#"release/"quoted"\branch"#;
    let runner = "self-hosted # e2e";
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("null:spa").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            default_branch: Some(default_branch.into()),
            ci_github_runner: Some(runner.into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow_yaml["on"]["push"]["branches"][0], default_branch);
    assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], runner);
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["defaults"]["run"]["shell"],
        "bash"
    );
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["strategy"]["matrix"]["app"][0]["name"],
        "null"
    );
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["strategy"]["matrix"]["app"][0]["dir"],
        "null"
    );
    assert_eq!(
        workflow_yaml["on"]["pull_request"]["paths"], workflow_yaml["on"]["push"]["paths"],
        "pull and push must render from one E2E path authority"
    );
    let setup_bun = workflow_yaml["jobs"]["e2e"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["name"] == "Setup Bun")
        .unwrap();
    assert_eq!(setup_bun["with"]["bun-version"], "1.3.14");
}

#[test]
fn scaffold_postgres_development_database_name_respects_identifier_limit() {
    let temp = tempdir().unwrap();
    let repo_name = "project".repeat(12);
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some(repo_name),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    let database_name = env_example
        .lines()
        .find_map(|line| line.strip_prefix("DATABASE_URL="))
        .and_then(|url| url.rsplit('/').next())
        .unwrap();
    assert_eq!(database_name.len(), 63);
    assert!(database_name.contains('_'));
}

#[test]
fn scaffold_db_defaults_set_sqlx_metadata_and_disable_schema_dump() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(answers.rust_sqlx_metadata_dir.as_deref(), Some(".sqlx"));
    assert_eq!(answers.schema_dump_enabled, Some(false));
}

#[test]
fn scaffold_bootstrap_command_records_shared_web_dependency_state() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
            ],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    for package_manager in ["bun", "npm", "pnpm", "yarn"] {
        let mut answers = AnswerOpts {
            web_package_manager: Some(package_manager.into()),
            ..AnswerOpts::default()
        };
        plan.apply_answer_defaults(&mut answers);
        let bootstrap_command = answers.bootstrap_command.unwrap();
        assert!(bootstrap_command.ends_with("&& scripts/check-webapps.sh bootstrap"));
        assert_eq!(
            bootstrap_command
                .matches("scripts/check-webapps.sh bootstrap")
                .count(),
            1
        );
        assert!(!bootstrap_command.contains("cd web"));
        assert!(!bootstrap_command.contains("cd landing"));
    }

    let mut default_answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut default_answers);
    assert_eq!(default_answers.web_package_manager.as_deref(), Some("bun"));
    assert!(
        default_answers
            .bootstrap_command
            .unwrap()
            .ends_with("&& scripts/check-webapps.sh bootstrap")
    );
}

#[test]
fn scaffold_bootstraps_frontend_before_validating_and_migrating_database() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    let command = answers.bootstrap_command.unwrap();
    let env_check = command
        .find("if [ -z \"${DATABASE_URL:-}\" ] && ! awk")
        .unwrap();
    assert!(command.contains("export it or copy .env.example to .env"));
    assert!(command.contains("export[[:space:]]+)?DATABASE_URL"));
    let cargo_fetch = command.find("cargo fetch").unwrap();
    let database_bootstrap = command
        .find("cargo run -p demo-api -- --bootstrap-database")
        .unwrap();
    let frontend_bootstrap = command.find("scripts/check-webapps.sh bootstrap").unwrap();
    assert!(cargo_fetch < frontend_bootstrap);
    assert!(frontend_bootstrap < env_check);
    assert!(env_check < database_bootstrap);
}

#[test]
fn go_scaffold_bootstrap_uses_the_same_database_lifecycle_as_runtime() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            go_module: Some("github.com/acme/demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(
        answers.migration_dir.as_deref(),
        Some("internal/database/migrations")
    );
    let command = answers.bootstrap_command.unwrap();
    let module_tidy = command.find("go mod tidy").unwrap();
    let frontend_bootstrap = command.find("scripts/check-webapps.sh bootstrap").unwrap();
    let database_guard = command.find("Missing DATABASE_URL").unwrap();
    let sqlc_generate = command.find("go tool sqlc generate").unwrap();
    let database_bootstrap = command
        .find("go run ./cmd/api --bootstrap-database")
        .unwrap();
    let contract_generate = command.find("node scripts/contracts.mjs generate").unwrap();
    assert!(module_tidy < frontend_bootstrap);
    assert!(frontend_bootstrap < database_guard);
    assert!(database_guard < sqlc_generate);
    assert!(sqlc_generate < database_bootstrap);
    assert!(database_bootstrap < contract_generate);
}

#[test]
fn go_scaffold_without_postgres_does_not_emit_migration_configuration() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("example-project".into()),
            go_module: Some("example.com/example-project".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    let mut answers = AnswerOpts::default();

    plan.apply_answer_defaults(&mut answers);

    assert_eq!(answers.go_database, Some(crate::backend::GoDatabase::None));
    assert_eq!(answers.migration_dir, None);
}

#[test]
fn scaffold_frontend_dev_scripts_only_launch_the_dev_server() {
    for package_manager in ["bun", "npm", "pnpm", "yarn"] {
        let temp = tempdir().unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: vec![
                    parse_scaffold_frontend("web").unwrap(),
                    parse_scaffold_frontend("landing").unwrap(),
                ],
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                web_package_manager: Some(package_manager.into()),
                ..AnswerOpts::default()
            },
            temp.path(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            plan.output_paths()
                .iter()
                .any(|path| path == Path::new(".yarnrc.yml")),
            package_manager == "yarn"
        );
        plan.write(temp.path(), false).unwrap();

        let web_package = fs::read_to_string(temp.path().join("web/package.json")).unwrap();
        assert!(web_package.contains(r#""dev": "vite""#));
        assert!(!web_package.contains(" install && "));
        let landing_package = fs::read_to_string(temp.path().join("landing/package.json")).unwrap();
        assert!(landing_package.contains(r#""dev": "astro dev""#));
        assert!(!landing_package.contains(" install && "));
        let landing_config =
            fs::read_to_string(temp.path().join("landing/astro.config.mjs")).unwrap();
        assert!(landing_config.contains("process.env.HOST?.trim()"));
        assert!(landing_config.contains("process.env.PORT"));
        assert!(landing_config.contains("strictPort: true"));
        let workspace_package = fs::read_to_string(temp.path().join("package.json")).unwrap();
        assert!(workspace_package.contains(&format!(r#""packageManager": "{package_manager}@"#)));
        assert_eq!(
            temp.path().join("pnpm-workspace.yaml").exists(),
            package_manager == "pnpm"
        );
        assert_eq!(
            temp.path().join(".yarnrc.yml").exists(),
            package_manager == "yarn"
        );
        if package_manager == "pnpm" {
            let pnpm_workspace =
                fs::read_to_string(temp.path().join("pnpm-workspace.yaml")).unwrap();
            let pnpm_workspace_yaml: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&pnpm_workspace).unwrap();
            assert_eq!(
                pnpm_workspace_yaml["enableGlobalVirtualStore"].as_bool(),
                Some(false)
            );
            assert_eq!(
                pnpm_workspace_yaml["linkWorkspacePackages"].as_bool(),
                Some(true)
            );
            assert_eq!(pnpm_workspace_yaml["overrides"]["js-yaml"], "4.3.1");
            assert!(
                pnpm_workspace.contains("pre-run validation rewrite installed executable shims")
            );
            assert!(pnpm_workspace.contains("Keep\n# this allowlist narrow"));
            assert!(pnpm_workspace.contains("authorizes dependency code execution"));
            assert!(pnpm_workspace.contains("\nallowBuilds:\n  esbuild: true\n"));
        }
        if package_manager == "yarn" {
            assert_eq!(
                fs::read_to_string(temp.path().join(".yarnrc.yml")).unwrap(),
                "nodeLinker: node-modules\n"
            );
        }
    }
}

#[test]
fn scaffold_preserves_legacy_frontend_kind_role_inference() {
    let temp = tempdir().unwrap();
    let legacy_astro = toml::from_str::<FrontendApp>(
        r#"name = "docs"
dir = "docs-site"
coverage_threshold = 0
kind = "env-port"
"#,
    )
    .unwrap();
    assert_eq!(legacy_astro.role, "astro");
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            frontend_apps: vec![
                legacy_astro,
                FrontendApp {
                    name: "marketing".into(),
                    dir: "marketing".into(),
                    coverage_threshold: 0,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();
    assert_eq!(report["frontends"][0]["kind"], "env-port");
    assert_eq!(report["frontends"][0]["role"], "astro");
    assert_eq!(report["frontends"][1]["kind"], "vite");
    assert_eq!(report["frontends"][1]["role"], "spa");
    assert!(temp.path().join("docs-site/astro.config.mjs").exists());
    assert!(temp.path().join("marketing/vite.config.ts").exists());

    let mut answers = AnswerOpts::default();
    plan.apply_answer_defaults(&mut answers);
    assert_eq!(answers.frontend_apps[0].name, "docs");
    assert_eq!(answers.frontend_apps[0].dir, "docs-site");
    assert_eq!(answers.frontend_apps[0].kind, "env-port");
    assert_eq!(answers.frontend_apps[0].role, "astro");
    assert_eq!(answers.frontend_apps[1].name, "marketing");
    assert_eq!(answers.frontend_apps[1].dir, "marketing");
    assert_eq!(answers.frontend_apps[1].kind, "vite");
    assert_eq!(answers.frontend_apps[1].role, "spa");
}

#[test]
fn scaffold_playwright_resolves_repo_root_from_nested_spa_dir() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "clients/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    plan.write(temp.path(), false).unwrap();

    let config = fs::read_to_string(temp.path().join("clients/web/playwright.config.ts")).unwrap();
    assert!(config.contains(r#"path.resolve(appDir, "../..")"#));
}
