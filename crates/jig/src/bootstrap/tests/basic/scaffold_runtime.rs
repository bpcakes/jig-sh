use super::*;

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
fn scaffold_database_bootstrap_validates_env_then_creates_and_migrates_database() {
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
    assert!(env_check < cargo_fetch);
    assert!(cargo_fetch < database_bootstrap);
    assert!(database_bootstrap < frontend_bootstrap);
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

#[test]
fn scaffold_uses_explicit_frontend_role_without_name_inference() {
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
            frontend_apps: vec![
                FrontendApp {
                    name: "admin".into(),
                    dir: "plain-admin-name".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "operations".into(),
                    dir: "operations".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "admin".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["frontends"][0]["role"], "spa");
    assert_eq!(report["frontends"][0]["ui"]["style"], "radix-nova");
    assert!(temp.path().join("plain-admin-name/src/App.tsx").exists());
    assert!(
        temp.path()
            .join("plain-admin-name/components.json")
            .exists()
    );
    assert!(
        !temp
            .path()
            .join("plain-admin-name/src/components/ui/sidebar.tsx")
            .exists()
    );
    assert_eq!(report["frontends"][1]["role"], "admin");
    assert_eq!(report["frontends"][1]["ui"]["style"], "radix-nova");
    assert!(temp.path().join("operations/components.json").exists());
    assert!(
        temp.path()
            .join("operations/src/components/ui/sidebar.tsx")
            .exists()
    );

    for (index, dir) in [(0, "plain-admin-name"), (1, "operations")] {
        let ui = &report["frontends"][index]["ui"];
        let package: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path().join(dir).join("package.json")).unwrap())
                .unwrap();
        let components: serde_json::Value = serde_json::from_slice(
            &fs::read(temp.path().join(dir).join("components.json")).unwrap(),
        )
        .unwrap();
        let readme = fs::read_to_string(temp.path().join(dir).join("README.md")).unwrap();
        let cli_version = ui["cli_version"].as_str().unwrap();
        let preset = ui["preset"].as_str().unwrap();
        let base = ui["base"].as_str().unwrap();
        let base_display = format!("{}{}", base[..1].to_ascii_uppercase(), &base[1..]);
        let tailwind_major = ui["tailwind_major"].as_u64().unwrap();

        assert_eq!(package["dependencies"]["shadcn"], cli_version);
        assert_eq!(components["style"], ui["style"]);
        assert!(readme.contains(&format!("shadcn CLI {cli_version}")));
        assert!(readme.contains(&format!("`{preset}` preset")));
        assert!(readme.contains(&format!("{base_display} primitives")));
        assert!(readme.contains(&format!("Tailwind CSS {tailwind_major}")));
        assert!(readme.contains(&format!("shadcn@{cli_version} info")));
    }
}

#[test]
fn scaffold_rejects_unknown_frontend_role() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            frontend_apps: vec![FrontendApp {
                name: "console".into(),
                dir: "console".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "dashboard".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported frontend app role 'dashboard'"));
    assert!(error.contains("spa, admin, or astro"));
}

#[test]
fn scaffold_rejects_duplicate_and_unsafe_frontend_app_dirs() {
    let temp = tempdir().unwrap();
    let duplicate = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate.contains("Duplicate scaffold frontend 'web'"));

    let duplicate_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![
                FrontendApp {
                    name: "docs".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "marketing".into(),
                    dir: "shared".into(),
                    coverage_threshold: 0,
                    kind: "env-port".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_dir.contains("Duplicate scaffold frontend dir 'shared'"));

    let duplicate_package_name = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![
                FrontendApp {
                    name: "foo_bar".into(),
                    dir: "foo_bar".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "foo-bar".into(),
                    dir: "foo-bar".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(duplicate_package_name.contains("names 'foo_bar' and 'foo-bar' normalize"));
    assert!(duplicate_package_name.contains("workspace package name 'foo-bar'"));

    let unsafe_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "../web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(unsafe_dir.contains("Scaffold frontend dir must not contain '.' or '..'"));

    let empty_segment_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web//app".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(empty_segment_dir.contains("must not contain empty path segments"));

    let rust_root_dir = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "ui".into(),
                dir: "crates/ui".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(rust_root_dir.contains("uses reserved directory 'crates/ui'"));
}

#[test]
fn scaffold_rejects_frontend_package_name_reserved_by_root_workspace() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("demo_workspace").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("frontend 'demo_workspace'"));
    assert!(error.contains("reserved root workspace package name 'demo-workspace'"));
}

#[test]
fn scaffold_rejects_mixed_scaffold_and_existing_frontend_app_inputs() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            frontend_apps: vec![FrontendApp {
                name: "admin".into(),
                dir: "admin".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot be combined with --frontend-app"));
}

#[test]
fn scaffold_rejects_frontend_dirs_reserved_for_rust_roots() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![parse_scaffold_frontend("apps").unwrap()],
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("uses reserved directory 'apps'"));
}

#[test]
fn scaffold_db_rejects_explicit_sqlx_disabled_answer() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold --db requires SQLx"));
}

#[test]
fn scaffold_prefixes_repo_names_that_are_invalid_rust_crate_identifiers() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(plan.summary().contains("repo name app-123-type"));
    assert!(
        plan.sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'app-123-type'")
    );
    plan.write(temp.path(), false).unwrap();

    assert!(
        temp.path()
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
    let main_rs =
        fs::read_to_string(temp.path().join("apps/app-123-type-api/src/main.rs")).unwrap();
    assert!(main_rs.contains("use ::app_123_type_http as app_http_crate;"));
    assert!(main_rs.contains("app_http_crate::router"));
    let core_lib =
        fs::read_to_string(temp.path().join("crates/app-123-type-core/src/lib.rs")).unwrap();
    assert!(core_lib.contains("#[allow(clippy::useless_concat)]\npub const APP_NAME"));
    assert!(core_lib.contains("pub const APP_NAME: &str = concat!("));
    assert!(core_lib.contains("\"app-123-type\","));

    let mixed_case = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("MyApp".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    assert!(
        mixed_case
            .sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'myapp'")
    );
}

#[test]
fn run_init_sqlite_scaffold_keeps_sanitized_database_names_and_ignores_aligned() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["scaffold"]["repo_name"], "app-123-type");
    assert_eq!(output["scaffold"]["repo_name_sanitized_from"], "123-type");
    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("requested repo name '123-type' was normalized")
    }));
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"app-123-type\""));
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=app_123_type=info,app_123_type_api=info,tower_http=info\nDATABASE_URL=sqlite:app_123_type.db\n"
    );
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/app_123_type.db\n"));
    assert!(gitignore.contains("/app_123_type.db-*\n"));
    for database_file in [
        "app_123_type.db",
        "app_123_type.db-wal",
        "app_123_type.db-shm",
        "app_123_type.db-journal",
        "app_123_type.db-jig-migrate.lock",
    ] {
        fs::write(destination.join(database_file), "local database artifact").unwrap();
    }
    assert_eq!(
        git_stdout(
            &destination,
            [
                "check-ignore",
                "--",
                "app_123_type.db",
                "app_123_type.db-wal",
                "app_123_type.db-shm",
                "app_123_type.db-journal",
                "app_123_type.db-jig-migrate.lock",
            ],
        )
        .unwrap(),
        "app_123_type.db\napp_123_type.db-wal\napp_123_type.db-shm\napp_123_type.db-journal\napp_123_type.db-jig-migrate.lock"
    );
    assert!(
        destination
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
}

#[test]
fn scaffold_sqlite_branch_generates_sqlite_db_helper() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("db/migrations".into()),
            ci_github_runner: Some("macos-14".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let report = plan.write(temp.path(), false).unwrap();

    assert_eq!(report["db"], "sqlite");
    let cargo_toml = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("\"sqlite\""));
    assert!(cargo_toml.contains("\"signal\", \"sync\", \"time\""));
    assert!(cargo_toml.contains("fs4 = \"0.13.1\""));
    assert!(cargo_toml.contains("url = \"2\""));
    assert!(cargo_toml.ends_with('\n'));
    assert_eq!(
        fs::read_to_string(temp.path().join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=demo=info,demo_api=info,tower_http=info\nDATABASE_URL=sqlite:demo.db\n"
    );
    let db_cargo = fs::read_to_string(temp.path().join("crates/demo-db/Cargo.toml")).unwrap();
    assert!(db_cargo.contains("anyhow.workspace = true"));
    assert!(db_cargo.contains("fs4.workspace = true"));
    assert!(db_cargo.contains("url.workspace = true"));
    assert!(db_cargo.contains("tokio.workspace = true"));
    let db_lib = fs::read_to_string(temp.path().join("crates/demo-db/src/lib.rs")).unwrap();
    assert!(db_lib.contains("SqlitePool"));
    assert!(db_lib.contains("sqlx::Sqlite::database_exists"));
    assert!(db_lib.contains("OpenOptions::new()"));
    assert!(db_lib.contains(".create_new(true)"));
    assert!(db_lib.contains("options.get_filename()"));
    assert!(db_lib.contains("fs::create_dir_all(parent)"));
    assert!(!db_lib.contains("sqlx::Sqlite::create_database"));
    assert!(db_lib.contains("create_if_missing"));
    assert!(db_lib.contains("concurrent_create_if_missing_calls_are_idempotent"));
    assert!(db_lib.contains("sqlx::migrate!(\n"));
    assert!(db_lib.contains("\"../../db/migrations\"\n        )"));
    assert!(db_lib.contains("DEFAULT_DB_TIMEOUT"));
    assert!(db_lib.contains("connect_with_timeout"));
    assert!(db_lib.contains("fs::canonicalize(&database_filename)"));
    assert!(db_lib.contains("sqlite_database_url_is_in_memory"));
    assert!(db_lib.contains("sqlite_database_url_semantics"));
    assert!(db_lib.contains("requires_single_connection_pool"));
    assert!(db_lib.contains("SqlitePoolOptions::new()"));
    assert!(db_lib.contains(".max_connections(1)"));
    assert!(db_lib.contains(".min_connections(1)"));
    assert!(db_lib.contains(".idle_timeout(None)"));
    assert!(db_lib.contains(".max_lifetime(None)"));
    assert!(db_lib.contains(".test_before_acquire(false)"));
    assert!(!db_lib.contains("num_idle()"));
    assert!(db_lib.contains("mirrors_sqlx_ordered_in_memory_cache_semantics"));
    assert!(db_lib.contains("in_memory_mode_ignores_an_existing_filename_for_locking"));
    assert!(db_lib.contains("create_if_missing_does_not_materialize_an_in_memory_filename"));
    assert!(db_lib.contains("symlink_aliases_share_the_canonical_migration_lock"));
    assert!(db_lib.contains("migrate_with_timeout"));
    assert!(db_lib.contains("static SQLITE_MIGRATION_LOCK"));
    assert!(db_lib.contains("fs4::fs_std::FileExt::try_lock_exclusive(&file)"));
    assert!(db_lib.contains("Ok(true) => return Ok(Some(file))"));
    assert!(db_lib.contains("Ok(false) =>"));
    assert!(!db_lib.contains("fs4::lock_contended_error"));
    assert!(db_lib.contains("in_memory_database_connects_and_migrates_without_a_file_lock"));
    assert!(db_lib.contains("private_cache_in_memory_pool_waits_for_the_active_checkout"));
    assert!(db_lib.contains("shared_in_memory_urls_keep_multiple_schema_aware_connections"));
    assert!(db_lib.contains("ordinary_file_pool_keeps_multiple_schema_aware_connections"));
    assert!(db_lib.contains("migration_mutex_is_shared_by_separate_in_memory_connections"));
    assert!(temp.path().join("db/migrations/.gitkeep").exists());
    let playwright = fs::read_to_string(temp.path().join("web/playwright.config.ts")).unwrap();
    assert!(playwright.contains("E2E_DATABASE_URL"));
    assert!(playwright.contains("sqlite:${defaultDatabasePath}"));
    assert!(playwright.contains("demo_web_e2e.sqlite"));
    assert!(playwright.contains("-- --bootstrap-database"));
    assert!(playwright.contains("['','-shm','-wal','-journal']"));
    #[cfg(unix)]
    {
        let reset_line = playwright
            .lines()
            .find(|line| line.contains("node -e") && line.contains("fs.rmSync"))
            .unwrap()
            .trim();
        let reset_command = reset_line
            .strip_prefix('`')
            .and_then(|line| line.strip_suffix("`,"))
            .unwrap()
            .replace("${defaultDatabasePath}", ".agent/tmp/demo_web_e2e.sqlite");
        let database = temp.path().join(".agent/tmp/demo_web_e2e.sqlite");
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        for suffix in ["", "-shm", "-wal", "-journal"] {
            fs::write(format!("{}{}", database.display(), suffix), "stale\n").unwrap();
        }
        assert!(
            std::process::Command::new("bash")
                .args(["-c", &reset_command])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        for suffix in ["", "-shm", "-wal", "-journal"] {
            assert!(!Path::new(&format!("{}{}", database.display(), suffix)).exists());
        }
    }
    let workflow = fs::read_to_string(temp.path().join(".github/workflows/e2e.yml")).unwrap();
    let workflow_yaml = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow_yaml["jobs"]["e2e"]["runs-on"], "macos-14");
    assert_eq!(
        workflow_yaml["jobs"]["e2e"]["env"]["SQLX_OFFLINE_DIR"],
        "${{ github.workspace }}/.sqlx"
    );
    assert!(!workflow.contains("image: postgres"));
    assert!(!workflow.contains("E2E_DATABASE_URL"));
    assert!(workflow.contains(r#"- "db/migrations/**""#));
    assert!(workflow.contains(r#"- ".sqlx/**""#));
    assert!(workflow.contains(r#"SQLX_OFFLINE: "true""#));
}

#[test]
fn scaffold_output_paths_include_template_collision_candidates() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    let paths = plan.output_paths();
    for expected in [
        ".env.example",
        "Cargo.toml",
        "crates/demo-http/Cargo.toml",
        "crates/demo-http/AGENTS.md",
        "crates/demo-http/src/lib.rs",
        "crates/demo-db/Cargo.toml",
        "crates/demo-db/AGENTS.md",
        "crates/demo-db/src/lib.rs",
        "crates/demo/AGENTS.md",
        "crates/demo-test-support/AGENTS.md",
        "crates/demo-test-support/src/app.rs",
        "crates/demo-test-support/src/db.rs",
        "crates/demo-test-support/tests/http.rs",
        "migrations/.gitkeep",
        "package.json",
        ".node-version",
        ".github/workflows/e2e.yml",
        "web/package.json",
        "web/.gitignore",
        "web/playwright.config.ts",
        "web/e2e/app.spec.ts",
        "web/components.json",
        "web/src/App.tsx",
        "web/src/api.ts",
        "web/src/app/router.ts",
        "web/src/routeTree.gen.ts",
        "web/src/routes/index.tsx",
        "web/src/components/ui/button.tsx",
        "web/src/lib/utils.ts",
        "landing/package.json",
        "landing/src/pages/index.astro",
        "admin-panel/package.json",
        "admin-panel/components.json",
        "admin-panel/src/app/router.ts",
        "admin-panel/src/routeTree.gen.ts",
        "admin-panel/src/routes/index.tsx",
        "admin-panel/src/routes/settings.tsx",
        "admin-panel/src/components/ui/sidebar.tsx",
        "admin-panel/src/features/overview/overview-page.tsx",
    ] {
        assert!(
            paths.iter().any(|path| path == Path::new(expected)),
            "missing output path {expected}"
        );
    }
}

#[test]
fn scaffold_rejects_unsupported_package_manager_before_scripts_render() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            web_package_manager: Some("cargo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported web_package_manager 'cargo'"));
}

#[test]
fn scaffold_generated_rust_workspace_has_valid_cargo_metadata() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
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

    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let package_names = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| package["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        "demo",
        "demo-api",
        "demo-core",
        "demo-db",
        "demo-http",
        "demo-test-support",
    ] {
        assert!(
            package_names.contains(&expected),
            "missing package {expected}"
        );
    }
}

#[test]
fn scaffold_test_support_uses_absolute_paths_for_local_module_name_collisions() {
    let temp = tempdir().unwrap();

    for repo_name in ["app", "db", "http", "responses"] {
        let destination = temp.path().join(repo_name);
        fs::create_dir(&destination).unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: Some(ScaffoldDb::Sqlite),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some(repo_name.into()),
                ..AnswerOpts::default()
            },
            &destination,
        )
        .unwrap()
        .unwrap();
        plan.write(&destination, false).unwrap();

        let module_name = repo_name.replace('-', "_");
        let test_support = destination
            .join("crates")
            .join(format!("{repo_name}-test-support"));
        let lib = fs::read_to_string(test_support.join("src/lib.rs")).unwrap();
        assert!(
            lib.contains(&format!("use ::{module_name} as app_crate;"))
                && lib.contains("app_crate::AppState::new()"),
            "application crate path was ambiguous for {repo_name}:\n{lib}"
        );
        let app = fs::read_to_string(test_support.join("src/app.rs")).unwrap();
        assert!(
            app.contains(&format!("use ::{module_name} as app_crate;"))
                && app.contains("app_crate::AppState::for_tests()"),
            "application crate path was ambiguous for {repo_name}:\n{app}"
        );
        let db = fs::read_to_string(test_support.join("src/db.rs")).unwrap();
        assert!(
            db.contains(&format!("use ::{module_name}_db as app_db_crate;"))
                && db.contains("pub type TestDbPool = app_db_crate::DbPool;"),
            "database crate path was ambiguous for {repo_name}:\n{db}"
        );

        if repo_name == "app" {
            let output = std::process::Command::new("cargo")
                .args(["fmt", "--all", "--", "--check"])
                .current_dir(&destination)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "cargo fmt failed for the colliding-name database scaffold\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
