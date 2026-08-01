use super::*;

#[test]
fn run_init_uses_native_renderer_and_git() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nexec git \"$@\"\n",
            log_path.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains(" init -b main"));
    assert!(destination.exists());
    assert!(destination.join(".jig.toml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("[vault]"));
    assert!(answers.contains("scope = \"repo\""));
    assert!(answers.contains("allow_global = false"));
    assert!(answers.contains(
        "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx sqlx prepare --check --workspace -- --workspace --all-targets"
    ));
    assert!(!answers.contains("cargo sqlx prepare --check"));
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("node_modules/"));
    assert!(gitignore.contains(".pnp.*"));
    assert!(gitignore.contains("!.yarn/patches"));
    assert!(gitignore.contains("target/"));
    assert!(gitignore.contains(".agent/.cache/*"));
    assert!(gitignore.contains(".agent/tmp/"));
    assert!(gitignore.contains("# BEGIN JIG MANAGED BLOCK"));
    let attributes = fs::read_to_string(destination.join(".gitattributes")).unwrap();
    assert!(attributes.contains(".agent/state/*.jsonl merge=union"));
    assert!(destination.join("scripts/jig").exists());
    let manifest_paths = managed_manifest_paths(&destination);
    assert!(
        manifest_paths
            .iter()
            .any(|path| path == managed_paths::MANIFEST_PATH)
    );
    assert!(
        manifest_paths
            .iter()
            .all(|path| destination.join(path).is_file())
    );
}

#[test]
fn run_init_sqlx_disabled_defaults_to_harness_only_safe_commands() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("Command values are project-owned."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo bootstrap."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test."));
}

#[test]
fn run_init_explicit_harness_only_writes_no_starter_application() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    assert!(!destination.join("Cargo.toml").exists());
    assert!(!destination.join("package.json").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn run_init_rejects_minimal_answers_with_rust_react_before_writes() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repo");
    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            harness_footprint: Some(HarnessFootprint::Minimal),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot combine harness_footprint = \"minimal\""));
    assert!(error.contains("Rust React scaffold"));
    assert!(!destination.exists());
}

#[test]
fn run_init_normalizes_minimal_answers_to_harness_only() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            harness_footprint: Some(HarnessFootprint::Minimal),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    assert!(destination.join(".agent/jig-contract.json").is_file());
    assert!(
        fs::read_to_string(destination.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
    assert!(!destination.join("scripts/jig").exists());
    assert!(!destination.join("Cargo.toml").exists());
}

#[test]
fn run_init_applies_relative_answers_file_before_scaffold_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("caller");
    let other = temp.path().join("other");
    let template = invocation.join("template");
    fs::create_dir_all(&invocation).unwrap();
    fs::create_dir_all(&other).unwrap();
    copy_dir_recursive(
        &template_repo_root().join("templates"),
        &template.join("templates"),
    );
    fs::write(
        invocation.join("answers.toml"),
        r#"repo_name = "file-app"
default_branch = "trunk"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf file-bootstrap"
web_package_manager = "pnpm"

[[frontend_apps]]
name = "portal"
dir = "clients/portal"
coverage_threshold = 77
kind = "vite"
role = "spa"

[dev]
[[dev.apps]]
name = "worker"
kind = "env-port"
command = "cargo run -p worker"
proxy = false
"#,
    )
    .unwrap();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);
    let destination = invocation.join("generated");

    let output = run_init(InitOpts {
        path: PathBuf::from("generated"),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some("template".into()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(PathBuf::from("answers.toml")),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(destination.join("apps/file-app-api").is_dir());
    assert!(destination.join("clients/portal/package.json").is_file());
    assert!(!destination.join("web").exists());
    assert_eq!(output["scaffold"]["frontends"][0]["name"], "portal");
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packageManager": "pnpm@"#));
    assert!(destination.join("pnpm-workspace.yaml").is_file());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"file-app\""));
    assert!(answers.contains("default_branch = \"trunk\""));
    assert!(answers.contains("bootstrap_command = \"printf file-bootstrap\""));
    assert!(answers.contains("name = \"worker\""));
    assert!(answers.contains("command = \"cargo run -p worker\""));
    assert!(!answers.contains("[[dev.apps]]\nname = \"api\""));
    assert!(!answers.contains("cargo run -p file-app-api -- --bootstrap-database"));
    let workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert!(workflow.contains("      - \"trunk\""));
    assert_eq!(
        git_stdout(&destination, ["symbolic-ref", "--short", "HEAD"]).unwrap(),
        "trunk"
    );
}

#[test]
fn run_init_cli_answers_override_answers_file_before_scaffold_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "file-app"
default_branch = "file-branch"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf file-bootstrap"
web_package_manager = "pnpm"
"#,
    )
    .unwrap();
    let destination = temp.path().join("generated");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
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
            answers_file: Some(answers_file),
            repo_name: Some("cli-app".into()),
            default_branch: Some("cli-branch".into()),
            bootstrap_command: Some("printf cli-bootstrap".into()),
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(destination.join("apps/cli-app-api").is_dir());
    assert!(!destination.join("apps/file-app-api").exists());
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packageManager": "npm@"#));
    assert!(!destination.join("pnpm-workspace.yaml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"cli-app\""));
    assert!(answers.contains("default_branch = \"cli-branch\""));
    assert!(answers.contains("bootstrap_command = \"printf cli-bootstrap\""));
    assert!(!answers.contains("printf file-bootstrap"));
    assert_eq!(
        git_stdout(&destination, ["symbolic-ref", "--short", "HEAD"]).unwrap(),
        "cli-branch"
    );
}

#[test]
fn run_init_rejects_malformed_or_conflicting_answers_before_destination_writes() {
    let temp = tempdir().unwrap();
    let malformed_answers = temp.path().join("malformed.toml");
    fs::write(&malformed_answers, "repo_name = [\n").unwrap();
    let malformed_destination = temp.path().join("malformed-repo");

    let error = run_init(InitOpts {
        path: malformed_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(malformed_answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("Failed to parse"));
    assert!(!malformed_destination.exists());

    let conflicting_answers = temp.path().join("conflicting.toml");
    fs::write(
        &conflicting_answers,
        r#"repo_name = "demo"
sqlx_enabled = false
schema_dump_enabled = false

[[frontend_apps]]
name = "portal"
dir = "portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let conflicting_destination = temp.path().join("conflicting-repo");
    let error = run_init(InitOpts {
        path: conflicting_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(conflicting_answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot be combined with --frontend-app answers"));
    assert!(!conflicting_destination.exists());

    let unsafe_metadata_destination = temp.path().join("unsafe-metadata-repo");
    let error = run_init(InitOpts {
        path: unsafe_metadata_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some("../sqlx-cache".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("Scaffold SQLx metadata dir must not contain '.' or '..'"));
    assert!(!unsafe_metadata_destination.exists());

    let custom_metadata_destination = temp.path().join("custom-metadata-repo");
    let error = run_init(InitOpts {
        path: custom_metadata_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some("db/sqlx-cache".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("pin SQLx 0.8"));
    assert!(error.contains("rust_sqlx_metadata_dir = '.sqlx'"));
    assert!(error.contains("jig adopt"));
    assert!(error.contains("sqlx_check_command"));
    assert!(!custom_metadata_destination.exists());
}

#[test]
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
    assert_eq!(agent_map_check["agents"], 5);
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
    assert_eq!(agent_guides_check["guide_count"], 4);
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
        "4.13.0"
    );
    assert!(destination.join(".env.example").exists());
    assert!(destination.join("Cargo.toml").exists());
    assert!(destination.join("apps/my-app-api/src/main.rs").exists());
    assert!(destination.join("crates/my-app-core/src/lib.rs").exists());
    assert!(destination.join("crates/my-app/src/lib.rs").exists());
    assert!(destination.join("crates/my-app/AGENTS.md").exists());
    assert!(destination.join("crates/my-app-http/src/lib.rs").exists());
    assert!(destination.join("crates/my-app-http/AGENTS.md").exists());
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
    assert!(web_package.contains(r#""shadcn": "4.13.0""#));
    assert!(web_package.contains(r#""tailwindcss": "4.3.2""#));
    assert!(web_package.contains(r#""@tanstack/react-query": "5.101.4""#));
    assert!(web_package.contains(r#""@tanstack/react-router": "1.170.18""#));
    assert!(web_package.contains(r#""@tanstack/eslint-plugin-query": "5.101.4""#));
    assert!(web_package.contains(r#""@tanstack/router-plugin": "1.168.23""#));
    assert!(web_package.contains(r#""build": "vite build && tsc -b""#));
    assert!(web_package.contains(r#""@testing-library/dom": "10.4.1""#));
    assert!(web_package.contains(r#""@playwright/test": "1.61.1""#));
    assert!(web_package.contains(r#""test:e2e": "playwright test""#));
    assert!(web_package.contains(r#""test:e2e:install": "playwright install chromium""#));
    assert!(
        web_package.contains(r#""test:e2e:install:ci": "playwright install --with-deps chromium""#)
    );
    assert!(!web_package.contains(" install && "));
    assert!(destination.join("web/src/api.ts").exists());
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
    assert!(web_api.contains("queryOptions({"));
    assert!(web_api.contains(r#"queryKey: ["app-status"]"#));
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
    assert!(web_e2e.contains(r#"versionResponse.headers()["x-request-id"]"#));
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
    assert!(admin_package.contains(r#""shadcn": "4.13.0""#));
    assert!(admin_package.contains(r#""tailwindcss": "4.3.2""#));
    assert!(admin_package.contains(r#""@tanstack/react-query": "5.101.4""#));
    assert!(admin_package.contains(r#""@tanstack/react-router": "1.170.18""#));
    assert!(admin_package.contains(r#""@tanstack/eslint-plugin-query": "5.101.4""#));
    assert!(admin_package.contains(r#""@tanstack/router-plugin": "1.168.23""#));
    assert!(admin_package.contains(r#""build": "vite build && tsc -b""#));
    assert!(!admin_package.contains("react-router-dom"));
    assert!(admin_package.contains(r#""@testing-library/dom": "10.4.1""#));
    assert!(admin_package.contains(r#""lint": "eslint . && prettier --check .""#));
    assert!(admin_package.contains(r#""format": "prettier --write .""#));
    assert!(admin_package.contains(r#""format:check": "prettier --check .""#));
    assert!(!admin_package.contains("@playwright/test"));
    let admin_readme = fs::read_to_string(destination.join("admin-panel/README.md")).unwrap();
    assert!(admin_readme.contains("real-backend Playwright starter for product SPA roles only"));
    let admin_vite_config =
        fs::read_to_string(destination.join("admin-panel/vite.config.ts")).unwrap();
    assert!(admin_vite_config.contains(r#"from "@tanstack/router-plugin/vite""#));
    assert!(admin_vite_config.contains("autoCodeSplitting: true"));
    assert!(admin_vite_config.contains("const devPort = Number(process.env.PORT)"));
    assert!(admin_vite_config.contains("port: devPort"));
    assert!(admin_vite_config.contains("strictPort: true"));
    assert!(admin_vite_config.contains("clientPort: devPort"));
    assert!(
        admin_vite_config
            .contains("firstNonEmpty(process.env.JIG_DEV_API_ORIGIN, process.env.API_ORIGIN)")
    );
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
    assert!(!jig_toml.contains("BIND_ADDR=\"${HOST}:${PORT}\""));
    assert!(!jig_toml.contains("port = 3000"));
    assert_eq!(
        fs::read_to_string(destination.join(".env.example")).unwrap(),
        "BIND_ADDR=127.0.0.1:3000\nRUST_LOG=my_app=info,my_app_api=info,tower_http=info\nDATABASE_URL=postgres://postgres:postgres@localhost:5432/my_app_dev\n"
    );
    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("dotenvy = \"0.15\""));
    let api_cargo = fs::read_to_string(destination.join("apps/my-app-api/Cargo.toml")).unwrap();
    assert!(api_cargo.contains("dotenvy.workspace = true"));
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
    assert!(http_lib.contains(r#".route("/health/live", get(live))"#));
    assert!(http_lib.contains(r#".route("/health/ready", get(ready))"#));
    assert!(http_lib.contains(r#".route("/api/version", get(version))"#));
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
    assert!(test_support_http_test.contains("async fn version_returns_json()"));
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
    let http_agents = fs::read_to_string(destination.join("crates/my-app-http/AGENTS.md")).unwrap();
    assert!(http_agents.contains("routes, handlers, middleware, extractors, and HTTP DTOs"));
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
    assert!(answers.contains(
        "cargo run -p my-app-api -- --bootstrap-database && scripts/check-webapps.sh bootstrap"
    ));
    assert!(!answers.contains("(cd web && bun install)"));
    assert!(answers.contains("name = \"web\""));
    assert!(answers.contains("dir = \"landing\""));
    assert!(answers.contains("kind = \"env-port\""));
    assert!(answers.contains("name = \"admin-panel\""));
    assert!(answers.contains("role = \"spa\""));
    assert!(answers.contains("role = \"astro\""));
    assert!(answers.contains("role = \"admin\""));
}

// Repeat the dependency-backed proof with:
// cargo test -p jig-sh bootstrap::tests::basic::scaffold_generation::generated_spa_coverage_counts_uncovered_future_production_modules -- --ignored --exact --nocapture
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
        .args(["dependencies-bootstrap", "web"])
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
            &fs::read_to_string(destination.join("web/coverage/coverage-summary.json")).unwrap(),
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
        destination.join("web/src/uncovered-production.ts"),
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

    let admin = destination.join(&frontend_name);
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

#[test]
fn scaffold_options_require_preset() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: None,
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold options require --preset rust-react"));
}

#[test]
fn rust_react_reserves_backend_dev_identity_across_frontend_sources() {
    let cases = vec![
        (
            "--frontend",
            ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: vec![parse_scaffold_frontend("api:spa").unwrap()],
                frontend_list: Vec::new(),
            },
            AnswerOpts::default(),
            "api",
        ),
        (
            "--frontends",
            ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: Vec::new(),
                frontend_list: vec![parse_scaffold_frontend("API:admin").unwrap()],
            },
            AnswerOpts::default(),
            "API",
        ),
        (
            "frontend_apps",
            ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                ..ScaffoldOpts::default()
            },
            AnswerOpts {
                frontend_apps: vec![FrontendApp {
                    name: "Api".into(),
                    dir: "site".into(),
                    coverage_threshold: 80,
                    kind: "env-port".into(),
                    role: "astro".into(),
                }],
                ..AnswerOpts::default()
            },
            "Api",
        ),
    ];

    for (source, opts, answers, supplied_name) in cases {
        let error = opts
            .validate_init_invariants(&answers)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("frontend app name '{supplied_name}'")),
            "{source}: {error}"
        );
        assert!(
            error.contains("reserved backend dev app 'api'"),
            "{source}: {error}"
        );
        assert!(error.contains("JIG_DEV_API"), "{source}: {error}");
        assert!(
            error.contains("choose another frontend name"),
            "{source}: {error}"
        );
    }
}

#[test]
fn reserved_backend_dev_identity_is_scoped_to_rust_react() {
    let api_frontend = FrontendApp {
        name: "api".into(),
        dir: "api".into(),
        coverage_threshold: 80,
        kind: "vite".into(),
        role: "spa".into(),
    };
    let answers = AnswerOpts {
        frontend_apps: vec![api_frontend],
        ..AnswerOpts::default()
    };

    for preset in [None, Some(ScaffoldPreset::HarnessOnly)] {
        ScaffoldOpts {
            preset,
            ..ScaffoldOpts::default()
        }
        .validate_init_invariants(&answers)
        .unwrap();
    }

    ScaffoldOpts {
        preset: Some(ScaffoldPreset::RustReact),
        frontends: vec![parse_scaffold_frontend("api-client:spa").unwrap()],
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts::default())
    .unwrap();
}

#[test]
fn run_init_rejects_merged_backend_named_frontend_before_template_or_destination_writes() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"[[frontend_apps]]
name = "Api"
dir = "site"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        template: Some(temp.path().join("missing-template").display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: false,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("frontend app name 'Api'"));
    assert!(error.contains("reserved backend dev app 'api'"));
    assert!(!destination.exists());
}

#[test]
fn run_init_rejects_invalid_frontend_package_names_before_writes() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repo");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: vec![ScaffoldFrontend {
                name: "-".into(),
                kind: ScaffoldFrontendKind::Spa,
                custom_default_name: false,
            }],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold frontend name must contain"));
    assert!(!destination.exists());
}

#[cfg(unix)]
fn assert_rendered_scaffold_rust_is_formatted(plan: &scaffold::InitScaffoldPlan, case: &str) {
    let rendered = plan.render_files().unwrap();
    let temp = tempdir().unwrap();
    let mut rust_paths = Vec::new();

    for (index, file) in rendered
        .into_iter()
        .filter(|file| file.relative.ends_with(".rs"))
        .enumerate()
    {
        let path = temp.path().join(format!("rendered-{index}.rs"));
        fs::write(&path, file.contents).unwrap();
        rust_paths.push(path);
    }

    assert!(!rust_paths.is_empty(), "{case}: scaffold rendered no Rust");
    let output = Command::new("rustfmt")
        .args([
            "--edition",
            "2024",
            "--check",
            "--config",
            "skip_children=true",
        ])
        .args(&rust_paths)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rendered Rust was not rustfmt-stable for {case}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn rust_react_package_stem_limit_is_applied_before_destination_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();

    let accepted_name = "r".repeat(216);
    let accepted_destination = temp.path().join("accepted");
    fs::create_dir(&accepted_destination).unwrap();
    let accepted_plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some(accepted_name.clone()),
            ..AnswerOpts::default()
        },
        &accepted_destination,
    )
    .unwrap()
    .unwrap();
    accepted_plan.write(&accepted_destination, false).unwrap();

    assert!(
        accepted_destination
            .join(format!("crates/{accepted_name}-test-support/Cargo.toml"))
            .is_file()
    );
    let vite_config = fs::read_to_string(accepted_destination.join("web/vite.config.ts")).unwrap();
    let repo_label = vite_config
        .split_once("http://api.")
        .unwrap()
        .1
        .split_once(".localhost:1355")
        .unwrap()
        .0;
    assert_eq!(repo_label.len(), 63);
    assert_eq!(repo_label, "r".repeat(63));

    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&accepted_destination)
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "maximum supported scaffold has invalid Cargo metadata\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&metadata.stdout),
        String::from_utf8_lossy(&metadata.stderr)
    );

    let rejected_destination = temp.path().join("rejected");
    let error = run_init(InitOpts {
        path: rejected_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(materialize_template_worktree().path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("r".repeat(217)),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("217-byte Cargo package stem"), "{error}");
    assert!(error.contains("at most 216 bytes"), "{error}");
    assert!(
        error.contains("lib<stem>_test_support-<hash>.rmeta"),
        "{error}"
    );
    assert!(!rejected_destination.exists());
}

#[cfg(unix)]
#[test]
fn scaffold_rendered_rust_is_formatted_across_names_databases_and_migration_paths() {
    let planning_root = tempdir().unwrap();
    let names = [
        ("manual-qa", "node22-npm12-sqlite-web".to_string()),
        ("width-40", format!("r{}", "a".repeat(39))),
        ("width-52", format!("r{}", "a".repeat(51))),
        ("width-71", format!("r{}", "a".repeat(70))),
        ("supported-max-216", format!("r{}", "a".repeat(215))),
    ];
    for (label, name) in &names {
        let expected_len = match *label {
            "manual-qa" => 23,
            "width-40" => 40,
            "width-52" => 52,
            "width-71" => 71,
            "supported-max-216" => 216,
            _ => unreachable!(),
        };
        assert_eq!(name.len(), expected_len, "{label}");
    }

    for db in [ScaffoldDb::None, ScaffoldDb::Sqlite, ScaffoldDb::Postgres] {
        let db_label = match db {
            ScaffoldDb::None => "none",
            ScaffoldDb::Sqlite => "sqlite",
            ScaffoldDb::Postgres => "postgres",
        };
        for (name_label, repo_name) in &names {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                &AnswerOpts {
                    repo_name: Some(repo_name.clone()),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(&plan, &format!("{db_label}/{name_label}"));
        }
    }

    for db in [ScaffoldDb::Sqlite, ScaffoldDb::Postgres] {
        let db_label = match db {
            ScaffoldDb::Sqlite => "sqlite",
            ScaffoldDb::Postgres => "postgres",
            ScaffoldDb::None => unreachable!(),
        };
        for migration_len in [13, 80, 216] {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                &AnswerOpts {
                    repo_name: Some("demo".into()),
                    rust_migration_dir: Some("m".repeat(migration_len)),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(
                &plan,
                &format!("{db_label}/migration-width-{migration_len}"),
            );
        }
    }
}
