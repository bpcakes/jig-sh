use super::*;

#[test]
fn init_rejects_unsafe_frontend_app_values() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let bad_name = run_init(InitOpts {
        path: temp.path().join("bad-name"),
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
            frontend_apps: vec![FrontendApp {
                name: "web;rm".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(bad_name.contains("Invalid frontend app name"));

    let bad_dir = run_init(InitOpts {
        path: temp.path().join("bad-dir"),
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
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "../web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(bad_dir.contains("must not contain '..'"));

    let absolute_dir = run_init(InitOpts {
        path: temp.path().join("absolute-dir"),
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
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "/tmp/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(
        absolute_dir.contains("portable repository-relative"),
        "unexpected absolute frontend dir error: {absolute_dir}"
    );

    let unsupported_dir = run_init(InitOpts {
        path: temp.path().join("unsupported-dir"),
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
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web:dev".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(unsupported_dir.contains("contains unsupported characters"));

    let env_prefix_collision = run_init(InitOpts {
        path: temp.path().join("env-prefix-collision"),
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
            frontend_apps: vec![
                FrontendApp {
                    name: "web-app".into(),
                    dir: "apps/web-app".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "web_app".into(),
                    dir: "apps/web_app".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(env_prefix_collision.contains("share derived dev environment prefix JIG_DEV_WEB_APP"));
}

#[test]
fn init_normalizes_harmless_frontend_and_dev_dir_spellings_before_validation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("normalized-dirs");

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
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "./apps//web/./".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            dev_apps: vec![DevApp {
                name: "web".into(),
                dir: Some("apps/web".into()),
                kind: "vite".into(),
                command: None,
                argv: vec!["npm".into(), "run".into(), "dev".into()],
                port: None,
                host: None,
                proxy: true,
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert_eq!(
        answers.matches("dir = \"apps/web\"").count(),
        2,
        "{answers}"
    );
    assert!(!answers.contains("./apps"), "{answers}");
    assert!(!answers.contains("apps//web"), "{answers}");
}

#[test]
fn init_rejects_answers_file_frontend_dev_app_dir_mismatch() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
kind = "vite"

[[dev.apps]]
name = "web"
dir = "apps/admin"
kind = "vite"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();

    let error = run_init(InitOpts {
        path: temp.path().join("repo"),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[dev.apps] entry 'web' uses dir 'apps/admin'"));
    assert!(error.contains("matching [[frontend_apps]] uses 'apps/web'"));
}

#[test]
fn init_rejects_answers_file_frontend_dev_app_missing_dir() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
kind = "vite"

[[dev.apps]]
name = "web"
kind = "vite"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();

    let error = run_init(InitOpts {
        path: temp.path().join("repo"),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[dev.apps] entry 'web' matches [[frontend_apps]]"));
    assert!(error.contains("must set dir = 'apps/web'"));
}

#[test]
fn init_recovers_and_persists_legacy_frontend_kind_and_role_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[frontend_apps]]
name = "docs"
dir = "./apps//docs"
coverage_threshold = 80

[[frontend_apps]]
name = "admin-panel"
dir = "apps/admin"
coverage_threshold = 80

[[frontend_apps]]
name = "marketing"
dir = "apps/marketing"
coverage_threshold = 80
kind = "vite"

[[dev.apps]]
name = "docs"
dir = "apps/docs/./"
kind = "env-port"
argv = ["npm", "run", "dev"]

[[dev.apps]]
name = "admin-panel"
dir = "apps/admin"
kind = "vite"
argv = ["npm", "run", "dev"]

[[dev.apps]]
name = "marketing"
dir = "apps/marketing"
kind = "env-port"
argv = ["npm", "run", "dev"]
"#,
    )
    .unwrap();
    let repo = temp.path().join("repo");

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(rendered.contains(
        "name = \"docs\"\ndir = \"apps/docs\"\ncoverage_threshold = 80\nkind = \"env-port\"\nrole = \"astro\""
    ));
    assert!(rendered.contains(
        "name = \"admin-panel\"\ndir = \"apps/admin\"\ncoverage_threshold = 80\nkind = \"vite\"\nrole = \"admin\""
    ));
    assert!(rendered.contains(
        "name = \"marketing\"\ndir = \"apps/marketing\"\ncoverage_threshold = 80\nkind = \"vite\"\nrole = \"spa\""
    ));

    let round_trip = RenderAnswers::from_answers_file(&repo.join(".jig.toml")).unwrap();
    assert_eq!(round_trip.frontend_apps()[0].kind, "env-port");
    assert_eq!(round_trip.frontend_apps()[0].role, "astro");
    assert_eq!(round_trip.frontend_apps()[1].kind, "vite");
    assert_eq!(round_trip.frontend_apps()[1].role, "admin");
    assert_eq!(round_trip.frontend_apps()[2].kind, "vite");
    assert_eq!(round_trip.frontend_apps()[2].role, "spa");
}

#[test]
fn init_defaults_answers_file_dev_app_kind_to_env_port() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[dev.apps]]
name = "api"
command = "cargo run -p demo-api"
"#,
    )
    .unwrap();
    let repo = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(rendered.contains("[[dev.apps]]\nname = \"api\""));
    assert!(rendered.contains("kind = \"env-port\""));
    assert!(rendered.contains("command = \"cargo run -p demo-api\""));
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str() == Some("scripts/jig dev"))
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command
                .as_str()
                .unwrap()
                .contains("[[dev.apps]] configured"))
    );
    assert!(
        output["render_report"]["suggested_jig_toml_edits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edit| edit.as_str().unwrap().contains("Tune [dev]"))
    );
}

#[test]
fn init_reports_and_preserves_legacy_dev_command_answer() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let repo = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: repo.clone(),
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
            dev_command: Some("npm run dev".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("Preserved deprecated dev_command")
    }));
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("dev_command = \"npm run dev\""));
    assert!(answers.contains("Deprecated and ignored by generated commands"));
}

#[test]
fn adopt_accepts_npm_frontend_app_and_renders_current_web_and_dev_config() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        "[package]\nname = \"api\"\n",
    )
    .unwrap();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("npm-shrinkwrap.json"), "{}").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage",
    "dev": "vite"
  }
}
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("scripts/jig check agent-guides")
    }));
    for command in [
        "scripts/jig check typescript-lint",
        "typescript-typecheck",
        "typescript-build",
        "typescript-coverage",
    ] {
        assert!(
            output["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|note| note.as_str().unwrap().contains(command)),
            "missing note for {command}"
        );
    }
    assert!(
        output["render_report"]["files_created"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/jig")
    );
    assert!(
        output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".github/workflows/webapp-checks.yml")
    );
    assert!(
        !output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".github/workflows/webapp-checks.yml")
    );
    assert!(
        output["render_report"]["todos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|todo| todo.as_str().unwrap().contains("frontend app"))
    );
    assert!(!repo.join("crates/api/AGENTS.md").exists());

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("web_package_manager = \"npm\""));
    assert!(answers.contains("[[frontend_apps]]"));
    assert!(answers.contains("[commands]"));
    assert!(answers.contains("typescript_lint_command = \"scripts/check-webapps.sh lint\""));
    assert!(answers.contains("tool = \"jig.typescript_lint\""));
    assert!(answers.contains("tool = \"jig.typescript_typecheck\""));
    assert!(answers.contains("tool = \"jig.typescript_build\""));
    assert!(answers.contains("tool = \"jig.typescript_coverage\""));
    assert!(answers.contains("[[dev.apps]]"));
    assert!(answers.contains(
        "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]"
    ));
    assert!(!answers.contains("dev_command"));

    assert!(!repo.join("Makefile").exists());
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    assert!(web_check.contains("run_managed_npm_command install"));
    assert!(web_check.contains("run_managed_npm_command run-script"));
    assert!(web_check.contains("--location=project"));
    assert!(
        web_check
            .contains("if [ -f npm-shrinkwrap.json ]; then printf '%s\\n' \"npm-shrinkwrap.json\"")
    );
    assert!(web_check.contains("dependencies_present"));
    assert!(web_check.contains("dependency_fingerprint"));
    assert!(web_check.contains("root.sha256"));
    assert!(web_check.contains("web-dependencies.lock"));
    assert!(web_check.contains("scripts/check-webapp-scripts.mjs"));
    assert!(web_check.contains("scripts/enforce-coverage.cjs"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains("\"typescript_lint_command\""));
    assert!(contract.contains(r#""name": "jig.typescript_lint""#));
    assert!(contract.contains(r#""name": "jig.typescript_typecheck""#));
    assert!(contract.contains(r#""name": "jig.typescript_build""#));
    assert!(contract.contains(r#""name": "jig.typescript_coverage""#));
    assert!(repo.join("scripts/check-webapp-scripts.mjs").is_file());
    let script_helper = fs::read_to_string(repo.join("scripts/check-webapp-scripts.mjs")).unwrap();
    assert!(script_helper.contains("typeof command !== \"string\""));
    assert!(script_helper.contains("command.trim().length === 0"));

    let web_workflow =
        fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert!(web_workflow.contains("actions/setup-node@v5"));
    assert!(web_workflow.contains("cache: npm"));
    assert_eq!(
        web_workflow.matches(r#"- "npm-shrinkwrap.json""#).count(),
        2
    );
    assert!(web_workflow.contains("${{ matrix.app.dir }}/npm-shrinkwrap.json"));
    assert!(web_workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
    assert!(web_workflow.contains("node scripts/check-webapp-scripts.mjs"));
    assert!(web_workflow.contains("node scripts/enforce-coverage.cjs"));
    assert!(!web_workflow.contains("make enforce-coverage"));
    assert!(!web_workflow.contains("oven-sh/setup-bun"));

    let rust_workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    assert!(rust_workflow.contains("scripts/jig check fmt"));
    assert_eq!(rust_workflow.matches(r#"- "rust-toolchain""#).count(), 2);
    assert!(!rust_workflow.contains("scripts/jig fmt-check"));

    let agent_map_workflow =
        fs::read_to_string(repo.join(".github/workflows/agent-map-check.yml")).unwrap();
    assert!(agent_map_workflow.contains("scripts/jig check agent-map"));
    assert!(!agent_map_workflow.contains("scripts/jig agent-map check"));
}

#[test]
fn adopted_yarn_classic_repo_selects_a_compatible_corepack_version() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("yarn.lock"),
        "# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.\n# yarn lockfile v1\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage",
    "dev": "vite"
  }
}
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("yarn".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let resolve_spec = || {
        let output = std::process::Command::new("bash")
            .args([
                "scripts/check-webapps.sh",
                "package-manager-spec",
                "apps/web",
            ])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Yarn spec resolver failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };

    assert_eq!(resolve_spec(), "yarn@1.22.22");

    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"yarn@1.22.19"}"#,
    )
    .unwrap();
    assert_eq!(resolve_spec(), "yarn@1.22.19");

    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"devEngines":{"packageManager":{"name":"yarn","version":"3.8.7"}}}"#,
    )
    .unwrap();
    assert_eq!(resolve_spec(), "yarn@3.8.7");

    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
    assert_eq!(resolve_spec(), "yarn@4.17.1");

    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert!(workflow.contains("scripts/check-webapps.sh package-manager-spec"));
    assert_eq!(workflow.matches(r#"- ".yarnrc""#).count(), 2);
}

#[test]
fn adopt_with_project_owned_makefile_keeps_file_and_emits_direct_typescript_gates() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("Makefile"), "project-owned:\n\t@true\n").unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage",
    "dev": "vite"
  }
}
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("Makefile")).unwrap(),
        "project-owned:\n\t@true\n"
    );

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(!answers.contains("makefile_enabled"));
    assert!(answers.contains("[[frontend_apps]]"));
    assert!(answers.contains("[commands]"));
    assert!(answers.contains("typescript_lint_command = \"scripts/check-webapps.sh lint\""));
    assert!(answers.contains("jig.typescript_lint"));

    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains("typescript_lint_command"));
    assert!(contract.contains("jig.typescript_lint"));

    let agent_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(agent_guide.contains("scripts/jig check typescript-lint"));
    assert!(!agent_guide.contains("make ci-webapps"));
}

#[test]
fn init_renders_web_commands_for_all_supported_package_managers() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let cases = [
        ("bun", "bun install --frozen-lockfile", "bun run"),
        (
            "npm",
            "run_managed_npm_command install",
            "run_npm_package_script",
        ),
        ("pnpm", "pnpm install --frozen-lockfile", "pnpm run"),
        ("yarn", "yarn install --frozen-lockfile", "yarn run"),
    ];

    for (package_manager, install_command, run_command) in cases {
        let repo = temp.path().join(package_manager);
        run_init(InitOpts {
            path: repo.clone(),
            scaffold: ScaffoldOpts::default(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some(format!("demo-{package_manager}")),
                sqlx_enabled: Some(false),
                web_package_manager: Some(package_manager.into()),
                frontend_apps: vec![FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        assert!(!repo.join("Makefile").exists());
        let agent_guidance = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(agent_guidance.contains(
            "Generated install steps select the package-manager project from workspace membership, not root-lock presence"
        ));
        assert!(agent_guidance.contains("It ignores only real top-level tool-cache directories"));
        assert!(
            !agent_guidance
                .contains("Generated install steps use a repo-root lockfile when one exists")
        );
        let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
        if package_manager == "npm" {
            assert!(web_check.contains("run_npm_dependency_install"));
            assert!(web_check.contains(install_command));
        } else {
            assert!(
                web_check.contains(install_command),
                "missing install command for {package_manager}"
            );
        }
        assert!(
            web_check.contains(run_command),
            "missing run command for {package_manager}"
        );
        assert!(web_check.contains("if dependencies_present \"$app_dir\""));
        assert!(web_check.contains("acquire_install_lock"));
        assert!(web_check.contains("dependency_stamp_path"));
        assert!(web_check.contains("dependency_fingerprint"));
        assert!(web_check.contains("jig-web-dependencies-v3"));
        assert!(web_check.contains("record_dependency_state"));
        assert!(web_check.contains("bootstrap_dependencies"));
        assert!(web_check.contains("dependencies-bootstrap"));
        assert!(web_check.contains("dependencies-ready"));
        assert!(web_check.contains("dependencies-install"));
        assert!(web_check.contains("node_version_file"));
        assert!(web_check.contains(r#"app_dir="apps/web""#));
        assert!(web_check.contains(r#""$app_dir/.node-version""#));
        assert!(web_check.contains("start_install_worker"));
        assert!(web_check.contains("transfer_install_lock_to_worker"));
        assert!(web_check.contains("recover_stale_install_lock"));
        assert!(web_check.contains("maximumEntries = 10_000"));
        assert!(!web_check.contains("root_lock_exists"));
        assert_eq!(
            web_check.contains("volatilePnpmWorkspaceState"),
            package_manager == "pnpm",
            "only pnpm checkers should exclude pnpm's volatile workspace-state cache"
        );
        #[cfg(unix)]
        {
            let syntax = std::process::Command::new("/bin/bash")
                .args(["-n", "scripts/check-webapps.sh"])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(
                syntax.success(),
                "invalid rendered Bash for {package_manager}"
            );
        }
        if package_manager == "yarn" {
            assert!(web_check.contains("dependency_artifact_kind"));
            assert!(web_check.contains("yarn_berry_config_payload"));
            assert!(web_check.contains("yarn_berry_pnp_artifact_proof"));
            assert!(web_check.contains("pnpEnableInlining"));
            assert!(web_check.contains("pnpEnableEsmLoader"));
            assert!(web_check.contains("installStatePath"));
            assert!(web_check.contains("pnpUnpluggedFolder"));
            assert!(web_check.contains("const RAW_RUNTIME_STATE"));
            assert!(web_check.contains("setupStatePackageLocations"));
            assert!(web_check.contains("maximumParsedInputBytes = 64 * 1024 * 1024"));
            assert!(web_check.contains("hashFile(loader, \"loader\", maximumParsedInputBytes)"));
            assert!(web_check.contains("hashFile(dataPath, \"data\", maximumParsedInputBytes)"));
            assert!(web_check.contains("too many PnP package locations"));
            assert!(web_check.contains("yarn_classic_actual_artifact_kind"));
            assert!(web_check.contains("yarn_classic_pnp_artifact_proof"));
            assert!(web_check.contains("YARN_PLUGNPLAY_OVERRIDE"));
            assert!(web_check.contains(r#""$scope/.pnp.cjs""#));
            assert!(web_check.contains(r#""$scope/.pnp.js""#));
            assert!(web_check.contains(r#"yarn_scope_authority_paths "$scope" "$authority""#));
            assert!(web_check.contains("yarn_runtime_identity"));
            assert!(web_check.contains("yarn@1.22.22"));
            for runtime_dir in [".yarn/patches", ".yarn/plugins", ".yarn/releases"] {
                assert!(
                    web_check.contains(runtime_dir),
                    "missing Yarn dependency input {runtime_dir}"
                );
            }
        }
        let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
        let expected_dev_argv = if package_manager == "npm" {
            "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]".to_string()
        } else {
            format!("argv = [\"{package_manager}\", \"run\", \"dev\"]")
        };
        assert!(
            answers.contains(&expected_dev_argv),
            "missing dev app argv for {package_manager}"
        );
        #[cfg(feature = "dev-proxy")]
        if package_manager == "npm" {
            let config = toml::from_str::<toml::Value>(&answers).unwrap();
            let generated_app = config["dev"]["apps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|app| app["name"].as_str() == Some("web"))
                .unwrap();
            let argv = generated_app["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|argument| argument.as_str().unwrap().to_owned())
                .collect::<Vec<_>>();
            assert!(
                jig_dev_proxy::is_generated_npm_dev_argv(&argv),
                "rendered npm dev argv drifted from the runtime matcher: {argv:?}"
            );
        }

        let workflow =
            fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
        assert!(workflow.contains("Classic required status checks can remain pending"));
        assert!(workflow.contains("APP_DIR: ${{ matrix.app.dir }}"));
        assert!(workflow.contains(r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#));
        for script in ["lint", "typecheck", "build:bundle", "test:coverage"] {
            assert!(
                workflow.contains(&format!(
                    r#"scripts/check-webapps.sh run-script "$APP_DIR" {script}"#
                )),
                "{package_manager} workflow bypassed the managed runner for {script}"
            );
        }
        assert_eq!(
            workflow
                .matches(r#"scripts/check-webapps.sh run-script "$APP_DIR""#)
                .count(),
            4
        );
        assert!(!workflow.contains(r#"cd "$APP_DIR" &&"#));
        assert!(!workflow.contains("if [ -f package.json ]"));
        assert!(workflow.contains("scripts/check-webapps.sh node-version-file \"$APP_DIR\""));
        assert!(workflow.contains("${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"));
        assert!(workflow.contains("mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\""));
        assert!(workflow.contains("set -o noclobber"));
        assert!(workflow.contains("'22.22.2' > \"$node_version_file\""));
        assert!(!workflow.contains("> .node-version"));
        assert!(workflow.contains("status=$?"));
        assert!(workflow.contains("if [ \"$status\" -eq 1 ]"));
        assert!(workflow.contains("exit \"$status\""));
        assert!(!workflow.contains("if ! node_version_file="));
        assert!(workflow.contains("$node_version_file\" >> \"$GITHUB_OUTPUT"));
        assert!(!workflow.contains("node-version: 22.12.0"));
        let expected_node_setup_count = if matches!(package_manager, "pnpm" | "yarn") {
            2
        } else {
            1
        };
        assert_eq!(
            workflow
                .matches("node-version-file: ${{ steps.node-version.outputs.path }}")
                .count(),
            expected_node_setup_count
        );
        assert!(!workflow.contains("node-version-file: .node-version"));
        for trigger in [
            r#"- "**/package.json""#,
            r#"- "**/package.json5""#,
            r#"- "**/package.yaml""#,
            r#"- "**/.node-version""#,
            r#"- "**/.npmrc""#,
            r#"- "**/*.patch""#,
            r#"- "**/*.diff""#,
        ] {
            assert_eq!(
                workflow.matches(trigger).count(),
                2,
                "missing recursive web workflow trigger {trigger}"
            );
        }
        let bootstrap_node = workflow
            .find("Bootstrap Node for dependency metadata")
            .unwrap();
        let resolve_node = workflow.find("Resolve Node version file").unwrap();
        assert!(bootstrap_node < resolve_node);
        if package_manager == "bun" {
            assert!(workflow.contains("oven-sh/setup-bun@v2"));
            assert!(workflow.contains("bun-version: \"1.3.14\""));
            assert_eq!(workflow.matches(r#"- "bun.lockb""#).count(), 2);
        } else {
            assert!(workflow.contains(&format!("cache: {package_manager}")));
        }
        if package_manager == "npm" {
            assert!(web_check.contains("run_managed_npm_command install \"$1\" \"$2\""));
            assert!(web_check.contains("run_managed_npm_command run-script \"$1\" \"$2\""));
            assert!(web_check.contains("const result = spawnSync(\"npm\", args, {"));
            assert!(web_check.contains("stdio: \"inherit\""));
            assert!(web_check.contains("shell: false"));
            assert!(web_check.contains("const match = /^npm_config_(.*)$/i.exec(key);"));
            assert!(web_check.contains("match[1].replaceAll(\"_\", \"-\").toLowerCase()"));
            for setting in [
                "omit",
                "include",
                "production",
                "optional",
                "only",
                "dev",
                "also",
                "bin-links",
                "dry-run",
                "package-lock-only",
                "package-lock",
                "global",
                "location",
                "if-present",
                "workspace",
                "workspaces",
                "include-workspace-root",
                "prefix",
                "cpu",
                "os",
                "libc",
            ] {
                assert!(web_check.contains(&format!("  \"{setting}\",")));
            }
            assert!(!web_check.contains("  \"ignore-scripts\","));
            assert!(!web_check.contains("  \"install-strategy\","));
            for argument in [
                "--include=dev",
                "--include=optional",
                "--include=peer",
                "--bin-links=true",
                "--dry-run=false",
                "--package-lock-only=false",
                "--package-lock=true",
                "--global=false",
                "--location=project",
                "--libc=glibc",
                "--libc=musl",
                "--workspaces=true",
                "--include-workspace-root=true",
                "--workspaces=false",
                "--workspace=.",
                "--if-present=false",
            ] {
                assert!(
                    web_check.contains(argument),
                    "missing npm argument {argument}"
                );
            }
            #[cfg(unix)]
            {
                use std::ffi::OsString;
                use std::io::Write as _;
                use std::os::unix::fs::PermissionsExt;
                use std::process::{Command, Stdio};

                let node = Command::new("node")
                    .args([
                        "-p",
                        "JSON.stringify({execPath:process.execPath,arch:process.arch,platform:process.platform,libc:process.platform==='linux'?(process.report.getReport().header.glibcVersionRuntime?'glibc':'musl'):null})",
                    ])
                    .output();
                if node.as_ref().is_ok_and(|output| output.status.success()) {
                    let node = node.unwrap();
                    let identity: serde_json::Value = serde_json::from_slice(&node.stdout).unwrap();
                    let node_executable = identity["execPath"].as_str().unwrap();
                    let fake_bin = repo.join("npm-launcher-bin");
                    fs::create_dir_all(&fake_bin).unwrap();
                    let fake_npm = fake_bin.join("npm");
                    fs::write(
                        &fake_npm,
                        r#"#!/bin/sh
set -eu
printf '%s\n' "$@" > "$INSTALL_ARGV"
env | LC_ALL=C sort > "$INSTALL_ENV"
"#,
                    )
                    .unwrap();
                    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();
                    let install_argv = repo.join("npm-launcher-argv");
                    let install_env = repo.join("npm-launcher-env");
                    let mut path = OsString::from(fake_bin.as_os_str());
                    path.push(":");
                    path.push(std::env::var_os("PATH").unwrap_or_default());
                    let launcher = web_check
                        .split_once(
                            "\"$node_bin\" - --jig-managed-npm \"$operation\" \"$app_dir\" \"$operation_argument\" <<'NODE'\n",
                        )
                        .unwrap()
                        .1
                        .split_once("\nNODE\n}")
                        .unwrap()
                        .0;
                    let mut command = Command::new(node_executable);
                    command
                        .args(["-", "--jig-managed-npm", "install", ".", "bootstrap"])
                        .current_dir(&repo)
                        .stdin(Stdio::piped())
                        .env("PATH", &path)
                        .env("INSTALL_ARGV", &install_argv)
                        .env("INSTALL_ENV", &install_env)
                        .env("NODE_ENV", "production")
                        .env("NPM_CONFIG_OMIT", "dev optional peer")
                        .env("NPM_CONFIG_ONLY", "production")
                        .env("NPM_CONFIG_DEV", "false")
                        .env("NPM_CONFIG_ALSO", "production")
                        .env("Npm_Config_Bin_Links", "false")
                        .env("npm_CONFIG_dry_run", "true")
                        .env("NPM_CONFIG_PACKAGE_LOCK_ONLY", "true")
                        .env("NPM_CONFIG_WORKSPACE", "other")
                        .env("NPM_CONFIG_WORKSPACES", "false")
                        .env("Npm_Config_Location", "global")
                        .env("npm_CONFIG_if_present", "true")
                        .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
                        .env("npm_config_install_strategy", "nested")
                        .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
                        .env("NPM_CONFIG_IGNORE_SCRIPTS", "true");
                    let mut child = command.spawn().unwrap();
                    child
                        .stdin
                        .take()
                        .unwrap()
                        .write_all(launcher.as_bytes())
                        .unwrap();
                    assert!(child.wait().unwrap().success());

                    let mut expected = vec![
                        "install".to_string(),
                        "--include=dev".into(),
                        "--include=optional".into(),
                        "--include=peer".into(),
                        "--bin-links=true".into(),
                        "--dry-run=false".into(),
                        "--package-lock-only=false".into(),
                        "--package-lock=true".into(),
                        "--global=false".into(),
                        "--location=project".into(),
                        format!("--prefix={}", repo.canonicalize().unwrap().display()),
                        format!("--cpu={}", identity["arch"].as_str().unwrap()),
                        format!("--os={}", identity["platform"].as_str().unwrap()),
                    ];
                    if let Some(libc) = identity["libc"].as_str() {
                        expected.push(format!("--libc={libc}"));
                    }
                    expected.extend([
                        "--workspaces=true".into(),
                        "--include-workspace-root=true".into(),
                    ]);
                    assert_eq!(
                        fs::read_to_string(&install_argv).unwrap(),
                        format!("{}\n", expected.join("\n"))
                    );
                    let environment = fs::read_to_string(&install_env).unwrap();
                    for removed in [
                        "NODE_ENV=",
                        "NPM_CONFIG_OMIT=",
                        "NPM_CONFIG_ONLY=",
                        "NPM_CONFIG_DEV=",
                        "NPM_CONFIG_ALSO=",
                        "Npm_Config_Bin_Links=",
                        "npm_CONFIG_dry_run=",
                        "NPM_CONFIG_PACKAGE_LOCK_ONLY=",
                        "NPM_CONFIG_WORKSPACE=",
                        "NPM_CONFIG_WORKSPACES=",
                        "Npm_Config_Location=",
                        "npm_CONFIG_if_present=",
                    ] {
                        assert!(!environment.lines().any(|line| line.starts_with(removed)));
                    }
                    for preserved in [
                        "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                        "npm_config_install_strategy=nested",
                        "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                        "NPM_CONFIG_IGNORE_SCRIPTS=true",
                    ] {
                        assert!(environment.lines().any(|line| line == preserved));
                    }

                    let mut command = Command::new(node_executable);
                    command
                        .args(["-", "--jig-managed-npm", "run-script", ".", "lint"])
                        .current_dir(&repo)
                        .stdin(Stdio::piped())
                        .env("PATH", &path)
                        .env("INSTALL_ARGV", &install_argv)
                        .env("INSTALL_ENV", &install_env)
                        .env("NODE_ENV", "test")
                        .env("NPM_CONFIG_OMIT", "dev optional peer")
                        .env("NPM_CONFIG_INCLUDE", "prod")
                        .env("NPM_CONFIG_PRODUCTION", "true")
                        .env("NPM_CONFIG_OPTIONAL", "false")
                        .env("NPM_CONFIG_ONLY", "production")
                        .env("NPM_CONFIG_DEV", "false")
                        .env("NPM_CONFIG_ALSO", "production")
                        .env("NPM_CONFIG_GLOBAL", "true")
                        .env("NPM_CONFIG_WORKSPACE", "other")
                        .env("NPM_CONFIG_WORKSPACES", "false")
                        .env("NPM_CONFIG_INCLUDE_WORKSPACE_ROOT", "false")
                        .env("NPM_CONFIG_PREFIX", "/hostile-prefix")
                        .env("Npm_Config_Location", "global")
                        .env("npm_CONFIG_if_present", "true")
                        .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
                        .env(
                            "NPM_CONFIG_//registry.example.invalid/:_authToken",
                            "test-token",
                        )
                        .env("npm_config_install_strategy", "nested")
                        .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
                        .env("NPM_CONFIG_STRICT_PEER_DEPS", "true")
                        .env("NPM_CONFIG_IGNORE_SCRIPTS", "true")
                        .env("NPM_CONFIG_FOREGROUND_SCRIPTS", "true")
                        .env("NPM_CONFIG_SCRIPT_SHELL", "/bin/sh");
                    let mut child = command.spawn().unwrap();
                    child
                        .stdin
                        .take()
                        .unwrap()
                        .write_all(launcher.as_bytes())
                        .unwrap();
                    assert!(child.wait().unwrap().success());

                    let expected = [
                        "--prefix=.",
                        "--workspace=.",
                        "--workspaces=true",
                        "--include-workspace-root=true",
                        "--global=false",
                        "--location=project",
                        "--if-present=false",
                        "--include=dev",
                        "--include=optional",
                        "--include=peer",
                        "run",
                        "lint",
                    ];
                    assert_eq!(
                        fs::read_to_string(&install_argv).unwrap(),
                        format!("{}\n", expected.join("\n"))
                    );
                    let environment = fs::read_to_string(&install_env).unwrap();
                    for removed in [
                        "NPM_CONFIG_OMIT=",
                        "NPM_CONFIG_INCLUDE=",
                        "NPM_CONFIG_PRODUCTION=",
                        "NPM_CONFIG_OPTIONAL=",
                        "NPM_CONFIG_ONLY=",
                        "NPM_CONFIG_DEV=",
                        "NPM_CONFIG_ALSO=",
                        "NPM_CONFIG_GLOBAL=",
                        "NPM_CONFIG_WORKSPACE=",
                        "NPM_CONFIG_WORKSPACES=",
                        "NPM_CONFIG_INCLUDE_WORKSPACE_ROOT=",
                        "NPM_CONFIG_PREFIX=",
                        "Npm_Config_Location=",
                        "npm_CONFIG_if_present=",
                    ] {
                        assert!(!environment.lines().any(|line| line.starts_with(removed)));
                    }
                    for preserved in [
                        "NODE_ENV=test",
                        "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                        "NPM_CONFIG_//registry.example.invalid/:_authToken=test-token",
                        "npm_config_install_strategy=nested",
                        "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                        "NPM_CONFIG_STRICT_PEER_DEPS=true",
                        "NPM_CONFIG_IGNORE_SCRIPTS=true",
                        "NPM_CONFIG_FOREGROUND_SCRIPTS=true",
                        "NPM_CONFIG_SCRIPT_SHELL=/bin/sh",
                    ] {
                        assert!(
                            environment.lines().any(|line| line == preserved),
                            "npm script launcher removed supported input {preserved}:\n{environment}"
                        );
                    }
                }
            }
            assert!(web_check.contains(
                "if [ -f npm-shrinkwrap.json ]; then printf '%s\\n' \"npm-shrinkwrap.json\""
            ));
            assert!(web_check.contains(
                "if [ -f \"$app_dir/npm-shrinkwrap.json\" ]; then printf '%s\\n' \"$app_dir/npm-shrinkwrap.json\""
            ));
            assert_eq!(workflow.matches(r#"- "npm-shrinkwrap.json""#).count(), 2);
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
        }
        if package_manager == "yarn" {
            assert_eq!(workflow.matches(r#"- ".yarnrc""#).count(), 2);
            assert_eq!(workflow.matches(r#"- "**/.yarnrc""#).count(), 2);
            assert_eq!(workflow.matches(r#"- "**/.yarnrc.yml""#).count(), 2);
            for runtime_path in [
                r#"- ".yarn/patches/**""#,
                r#"- ".yarn/plugins/**""#,
                r#"- ".yarn/releases/**""#,
            ] {
                assert_eq!(
                    workflow.matches(runtime_path).count(),
                    2,
                    "missing Yarn workflow trigger {runtime_path}"
                );
            }
            for runtime_path in [
                r#"- "**/.yarn/patches/**""#,
                r#"- "**/.yarn/plugins/**""#,
                r#"- "**/.yarn/releases/**""#,
            ] {
                assert_eq!(
                    workflow.matches(runtime_path).count(),
                    2,
                    "missing recursive Yarn workflow trigger {runtime_path}"
                );
            }
        }
        if package_manager == "pnpm" {
            assert!(
                workflow.contains(r#"scripts/check-webapps.sh package-manager-spec "$APP_DIR""#)
            );
            assert!(!workflow.contains("corepack prepare pnpm@11.13.0 --activate"));
        }
        if package_manager == "yarn" {
            assert!(
                workflow.contains(r#"scripts/check-webapps.sh package-manager-spec "$APP_DIR""#)
            );
            assert!(!workflow.contains("corepack prepare yarn@4.17.1 --activate"));
        }
        if matches!(package_manager, "pnpm" | "yarn") {
            assert!(workflow.contains(
                r#"package_manager_spec="$(scripts/check-webapps.sh package-manager-spec "$APP_DIR")" || exit $?"#
            ));
            assert!(workflow.contains("corepack prepare \"$package_manager_spec\" --activate"));
            assert!(
                !workflow.contains(
                    r#"corepack prepare "$(scripts/check-webapps.sh package-manager-spec"#
                )
            );
            let corepack = workflow.find("corepack enable").unwrap();
            let cache = workflow.find(&format!("cache: {package_manager}")).unwrap();
            assert!(
                corepack < cache,
                "corepack must be enabled before {package_manager} cache setup"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn generated_npm_run_script_selects_exact_app_without_installing() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("npm-run-script");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("npm-run-script".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![
                FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "standalone".into(),
                    dir: "standalone".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("standalone")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root lock\n").unwrap();
    for app_dir in ["apps/web", "standalone"] {
        fs::write(
            repo.join(app_dir).join("package.json"),
            format!(
                r#"{{"name":"{}","scripts":{{"probe":"true"}}}}"#,
                app_dir.replace('/', "-")
            ),
        )
        .unwrap();
    }
    fs::write(
        repo.join("standalone/package-lock.json"),
        "standalone lock\n",
    )
    .unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    : > "$UNEXPECTED_INSTALL"
    exit 91
    ;;
esac
printf '%s\n' "$@" > "$RUN_ARGV"
pwd -P > "$RUN_CWD"
env | LC_ALL=C sort > "$RUN_ENV"
count=0
[ ! -f "$RUN_COUNT" ] || count="$(cat "$RUN_COUNT")"
printf '%s\n' "$((count + 1))" > "$RUN_COUNT"
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let run_count = repo.join("run-count");
    let unexpected_install = repo.join("unexpected-install");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run_script = |app_dir: &str, script_name: &str, suffix: &str| {
        let argv = repo.join(format!("run-{suffix}-argv"));
        let cwd = repo.join(format!("run-{suffix}-cwd"));
        let environment = repo.join(format!("run-{suffix}-env"));
        let node_environment = if suffix == "standalone" {
            "production"
        } else {
            "test"
        };
        let output = std::process::Command::new("bash")
            .args([
                "scripts/check-webapps.sh",
                "run-script",
                app_dir,
                script_name,
            ])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("RUN_ARGV", &argv)
            .env("RUN_CWD", &cwd)
            .env("RUN_ENV", &environment)
            .env("RUN_COUNT", &run_count)
            .env("UNEXPECTED_INSTALL", &unexpected_install)
            .env("NODE_ENV", node_environment)
            .env("NPM_CONFIG_OMIT", "dev optional peer")
            .env("NPM_CONFIG_INCLUDE", "prod")
            .env("NPM_CONFIG_PRODUCTION", "true")
            .env("NPM_CONFIG_OPTIONAL", "false")
            .env("NPM_CONFIG_ONLY", "production")
            .env("NPM_CONFIG_DEV", "false")
            .env("NPM_CONFIG_ALSO", "production")
            .env("Npm_Config_Workspace", "missing")
            .env("npm_CONFIG_workspaces", "false")
            .env("NPM_CONFIG_INCLUDE_WORKSPACE_ROOT", "false")
            .env("npm_config_include-workspace-root", "false")
            .env("NPM_CONFIG_PREFIX", "/hostile-prefix")
            .env("NPM_CONFIG_GLOBAL", "true")
            .env("Npm_Config_Location", "global")
            .env("npm_CONFIG_if_present", "true")
            .env("NPM_CONFIG_IF-PRESENT", "true")
            .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
            .env(
                "NPM_CONFIG_//registry.example.invalid/:_authToken",
                "test-token",
            )
            .env("npm_config_install_strategy", "nested")
            .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
            .env("NPM_CONFIG_STRICT_PEER_DEPS", "true")
            .env("NPM_CONFIG_IGNORE_SCRIPTS", "true")
            .env("NPM_CONFIG_FOREGROUND_SCRIPTS", "true")
            .env("NPM_CONFIG_SCRIPT_SHELL", "/bin/sh")
            .output()
            .unwrap();
        (output, argv, cwd, environment)
    };

    let expected_argv = [
        "--prefix=.",
        "--workspace=.",
        "--workspaces=true",
        "--include-workspace-root=true",
        "--global=false",
        "--location=project",
        "--if-present=false",
        "--include=dev",
        "--include=optional",
        "--include=peer",
        "run",
        "probe",
    ];
    for (app_dir, suffix) in [("apps/web", "workspace"), ("standalone", "standalone")] {
        let (output, argv, cwd, environment) = run_script(app_dir, "probe", suffix);
        assert!(
            output.status.success(),
            "npm run-script failed for {app_dir}:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(argv).unwrap(),
            format!("{}\n", expected_argv.join("\n"))
        );
        assert_eq!(
            fs::read_to_string(cwd).unwrap().trim(),
            fs::canonicalize(repo.join(app_dir))
                .unwrap()
                .display()
                .to_string()
        );
        let environment = fs::read_to_string(environment).unwrap();
        for removed in [
            "NPM_CONFIG_OMIT=",
            "NPM_CONFIG_INCLUDE=",
            "NPM_CONFIG_PRODUCTION=",
            "NPM_CONFIG_OPTIONAL=",
            "NPM_CONFIG_ONLY=",
            "NPM_CONFIG_DEV=",
            "NPM_CONFIG_ALSO=",
            "Npm_Config_Workspace=",
            "npm_CONFIG_workspaces=",
            "NPM_CONFIG_INCLUDE_WORKSPACE_ROOT=",
            "npm_config_include-workspace-root=",
            "NPM_CONFIG_PREFIX=",
            "NPM_CONFIG_GLOBAL=",
            "Npm_Config_Location=",
            "npm_CONFIG_if_present=",
            "NPM_CONFIG_IF-PRESENT=",
        ] {
            assert!(
                !environment.lines().any(|line| line.starts_with(removed)),
                "npm run-script inherited shaping input {removed}:\n{environment}"
            );
        }
        let expected_node_environment = if suffix == "standalone" {
            "NODE_ENV=production"
        } else {
            "NODE_ENV=test"
        };
        assert!(
            environment
                .lines()
                .any(|line| line == expected_node_environment),
            "npm run-script changed explicit {expected_node_environment}:\n{environment}"
        );
        for preserved in [
            "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
            "NPM_CONFIG_//registry.example.invalid/:_authToken=test-token",
            "npm_config_install_strategy=nested",
            "NPM_CONFIG_LEGACY_PEER_DEPS=true",
            "NPM_CONFIG_STRICT_PEER_DEPS=true",
            "NPM_CONFIG_IGNORE_SCRIPTS=true",
            "NPM_CONFIG_FOREGROUND_SCRIPTS=true",
            "NPM_CONFIG_SCRIPT_SHELL=/bin/sh",
        ] {
            assert!(
                environment.lines().any(|line| line == preserved),
                "npm run-script removed supported input {preserved}:\n{environment}"
            );
        }
    }
    assert_eq!(fs::read_to_string(&run_count).unwrap().trim(), "2");
    assert!(!unexpected_install.exists());
    assert!(!repo.join("node_modules").exists());
    assert!(!repo.join("standalone/node_modules").exists());
    assert!(!repo.join(".agent/tmp/web-dependencies").exists());

    let (missing, _, _, _) = run_script("apps/web", "missing", "missing");
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&run_count).unwrap().trim(), "2");

    let unconfigured = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "run-script",
            "unconfigured",
            "probe",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(unconfigured.status.code(), Some(2));
    let wrong_arity = std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", "run-script", "apps/web"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(wrong_arity.status.code(), Some(2));
}

#[test]
fn generated_project_workflows_serialize_dynamic_yaml_scalars_and_shell_branch_values() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("yaml-scalars");
    let default_branch = "release#candidate";
    let runner = "self-hosted # primary";

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("yaml-scalars".into()),
            default_branch: Some(default_branch.into()),
            ci_github_runner: Some(runner.into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "null".into(),
                dir: "null".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    for workflow in [
        "webapp-checks.yml",
        "repo-policy.yml",
        "rust-tests.yml",
        "agent-map-check.yml",
    ] {
        let text = fs::read_to_string(repo.join(".github/workflows").join(workflow)).unwrap();
        let yaml = serde_yaml_ng::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|error| panic!("{workflow} was invalid YAML: {error}\n{text}"));
        assert_eq!(yaml["on"]["push"]["branches"][0], default_branch);
        let jobs = yaml["jobs"].as_object().unwrap();
        for job in jobs.values() {
            assert_eq!(job["runs-on"], runner, "wrong runner in {workflow}");
        }
    }

    let webapp = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    let webapp = serde_yaml_ng::from_str::<serde_json::Value>(&webapp).unwrap();
    let app = &webapp["jobs"]["checks"]["strategy"]["matrix"]["app"][0];
    assert_eq!(app["name"], "null");
    assert_eq!(app["dir"], "null");

    let policy = fs::read_to_string(repo.join(".github/workflows/repo-policy.yml")).unwrap();
    assert!(policy.contains("JIG_DEFAULT_BRANCH:"));
    assert!(policy.contains(r#""origin/$JIG_DEFAULT_BRANCH""#));
    assert!(!policy.contains(&format!("origin/{default_branch}")));
}

#[cfg(unix)]
#[test]
fn generated_node_version_query_rejects_every_present_invalid_authority() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("node-version-authority");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("node-version-authority".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    fs::create_dir(repo.join("web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["web"]}"#,
    )
    .unwrap();
    fs::write(repo.join("web/package.json"), r#"{"private":true}"#).unwrap();

    let query = || {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "node-version-file", "web"])
            .current_dir(&repo)
            .output()
            .unwrap()
    };
    let assert_query_status = |expected: i32, label: &str| {
        let output = query();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{label}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };

    assert_query_status(1, "true absence");

    fs::write(repo.join("web/.node-version"), ">=22\n").unwrap();
    let app_valid = assert_query_status(0, "valid app selector");
    assert_eq!(
        String::from_utf8_lossy(&app_valid.stdout).trim(),
        "web/.node-version"
    );

    fs::write(repo.join(".node-version"), "22.22.2\r\n").unwrap();
    let root_valid = assert_query_status(0, "valid root selector");
    assert_eq!(
        String::from_utf8_lossy(&root_valid.stdout).trim(),
        ".node-version"
    );

    fs::write(repo.join(".node-version"), "invalid selector with spaces\n").unwrap();
    assert_query_status(
        2,
        "invalid root must not fall through to valid app selector",
    );
    fs::write(repo.join(".node-version"), "").unwrap();
    assert_query_status(2, "empty authority");
    fs::write(repo.join(".node-version"), "22.22.2\nsecond\n").unwrap();
    assert_query_status(2, "multiline authority");
    fs::write(repo.join(".node-version"), [0xff]).unwrap();
    assert_query_status(2, "non-UTF-8 authority");
    fs::write(repo.join(".node-version"), b"22.22.2\x7f\n").unwrap();
    assert_query_status(2, "control character authority");
    fs::write(repo.join(".node-version"), vec![b'2'; 129]).unwrap();
    assert_query_status(2, "oversized authority");

    fs::remove_file(repo.join(".node-version")).unwrap();
    fs::create_dir(repo.join(".node-version")).unwrap();
    assert_query_status(2, "directory authority");
    fs::remove_dir(repo.join(".node-version")).unwrap();

    let fifo = repo.join(".node-version");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo_path` is a live, NUL-terminated CString for the duration of
    // the call, and the mode contains only valid permission bits.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
    assert_query_status(2, "FIFO authority");
    fs::remove_file(&fifo).unwrap();

    let outside = temp.path().join("outside-node-version");
    fs::write(&outside, "20.0.0\n").unwrap();
    symlink(&outside, repo.join(".node-version")).unwrap();
    assert_query_status(2, "root symlink authority");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "20.0.0\n");
    fs::remove_file(repo.join(".node-version")).unwrap();

    fs::remove_file(repo.join("web/.node-version")).unwrap();
    symlink(&outside, repo.join("web/.node-version")).unwrap();
    assert_query_status(2, "app symlink authority");
    fs::remove_file(repo.join("web/.node-version")).unwrap();

    fs::rename(repo.join("web"), repo.join("web-real")).unwrap();
    let outside_app = temp.path().join("outside-app");
    fs::create_dir(&outside_app).unwrap();
    fs::write(outside_app.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(outside_app.join(".node-version"), "20.0.0\n").unwrap();
    symlink(&outside_app, repo.join("web")).unwrap();
    assert_query_status(2, "symlinked authority parent");

    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert!(workflow.contains("${RUNNER_TEMP:?GitHub Actions did not provide RUNNER_TEMP}"));
    assert!(workflow.contains("mktemp -d \"$RUNNER_TEMP/jig-node-version.XXXXXX\""));
    assert!(workflow.contains("set -o noclobber"));
    assert!(!workflow.contains("> .node-version"));
    assert_eq!(fs::read_to_string(&outside).unwrap(), "20.0.0\n");
}

#[test]
fn generated_project_workflow_jobs_force_bash_on_windows_runners() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("windows-workflows");

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("windows-workflows".into()),
            ci_github_runner: Some("windows-latest".into()),
            sqlx_enabled: Some(true),
            schema_dump_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    for workflow in [
        "webapp-checks.yml",
        "repo-policy.yml",
        "rust-tests.yml",
        "agent-map-check.yml",
    ] {
        let text = fs::read_to_string(repo.join(".github/workflows").join(workflow)).unwrap();
        let yaml = serde_yaml_ng::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|error| panic!("{workflow} was invalid YAML: {error}\n{text}"));
        for (job_name, job) in yaml["jobs"].as_object().unwrap() {
            assert_eq!(
                job["runs-on"], "windows-latest",
                "wrong Windows runner for {workflow}:{job_name}"
            );
            assert_eq!(
                job["defaults"]["run"]["shell"], "bash",
                "{workflow}:{job_name} must force Bash for generated run steps"
            );
        }
    }

    let source = fs::read_to_string(
        template
            .path()
            .join("templates/project/.github/workflows/webapp-checks.yml.jinja"),
    )
    .unwrap();
    let mut environment = minijinja::Environment::new();
    environment.set_syntax(
        minijinja::syntax::SyntaxConfig::builder()
            .block_delimiters("[%", "%]")
            .variable_delimiters("<<[", "]>>")
            .comment_delimiters("<#", "#>")
            .build()
            .unwrap(),
    );
    environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    let text = environment
        .render_str(
            &source,
            serde_json::json!({
                "frontend_apps": [],
                "ci_github_runner": "windows-latest",
            }),
        )
        .unwrap();
    let yaml = serde_yaml_ng::from_str::<serde_json::Value>(&text).unwrap_or_else(|error| {
        panic!("disabled webapp workflow was invalid YAML: {error}\n{text}")
    });
    assert_eq!(yaml["jobs"]["disabled"]["runs-on"], "windows-latest");
    assert_eq!(yaml["jobs"]["disabled"]["defaults"]["run"]["shell"], "bash");
}

#[cfg(unix)]
#[test]
fn generated_web_checks_keep_app_local_yarn_lock_authoritative() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("app-local-yarn");

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("app-local-yarn".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("yarn".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"yarn@1.22.22"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/package.json"),
        r#"{"private":true,"packageManager":"yarn@1.22.20"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"},"installConfig":{"pnp":true}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/yarn.lock"),
        "# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.\n# yarn lockfile v1\n",
    )
    .unwrap();
    fs::write(repo.join("apps/web/.node-version"), "20.19.1\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_node = fake_bin.join("node");
    fs::write(
        &fake_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  if kill -0 "$3" 2>/dev/null; then printf '%s\n' live; else printf '%s\n' stale; fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-lockfile-kind" ]; then
  lockfile="${3:-}"
  [ -n "$lockfile" ] && [ -f "$lockfile" ] && [ ! -L "$lockfile" ] || exit 1
  if tr -d '\r' < "$lockfile" | grep -Eq '^# yarn lockfile v1$'; then
    printf '%s\n' classic
  else
    printf '%s\n' berry
  fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-authority-preflight" ]; then
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-config" ]; then
  printf '%s\n' 'classic:dGVzdA=='
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-pnp-proof" ]; then
  [ -s "$4" ] || exit 1
  cksum "$4" | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-workspace-metadata" ]; then
  [ "${3:-}" != "contains" ]
  exit $?
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-manifest" ]; then
  manifest="$3"
  if tr '\n' ' ' < "$manifest" | grep -Eq '"installConfig"[[:space:]]*:[[:space:]]*\{[^}]*"pnp"[[:space:]]*:[[:space:]]*true'; then
    printf '%s\n' true
    exit 0
  fi
  exit 1
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-berry-config" ]; then
  linker=pnp
  if [ -f .yarnrc.yml ] && grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*(node-modules|pnpm)' .yarnrc.yml; then
    linker=node-modules
  fi
  config="{\"nodeLinker\":\"$linker\",\"cacheFolder\":\"$(pwd)/.yarn/cache\",\"installStatePath\":\"$(pwd)/.yarn/install-state.gz\",\"pnpUnpluggedFolder\":\"$(pwd)/.yarn/unplugged\",\"pnpEnableInlining\":false,\"pnpEnableEsmLoader\":false}"
  printf '%s' "$config" | base64 | tr -d '\n'
  printf '\n'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-config-value" ]; then
  config="$(printf '%s' "$3" | base64 --decode 2>/dev/null || printf '%s' "$3" | base64 -D)"
  case "$4" in
    nodeLinker) printf '%s\n' "$config" | sed -n 's/.*"nodeLinker":"\([^"]*\)".*/\1/p' ;;
    *) exit 1 ;;
  esac
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-pnp-proof" ]; then
  scope="$3"
  for required in "$4" "$scope/.pnp.data.json" "$scope/.yarn/install-state.gz" "$scope/.yarn/cache/dependency.zip"; do
    [ -s "$required" ] && [ ! -L "$required" ] || exit 1
    cksum "$required"
  done | cksum | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-node-modules-proof" ]; then
  root="$3/node_modules"
  entries="$(find "$root" -mindepth 1 \( -type f -o -type l \) ! -name '.jig-web-dependencies-v3' ! -name '.jig-web-dependencies-v3.tmp.*' ! -path "$root/.cache/*" ! -path "$root/.vite/*" ! -path "$root/.tmp/*" ! -name '.DS_Store' -print | LC_ALL=C sort)"
  [ -n "$entries" ] || exit 1
  printf '%s\n' "$entries" | while IFS= read -r entry; do
    relative="${entry#"$root"/}"
    if [ -L "$entry" ]; then
      printf 'link %s %s\n' "$relative" "$(readlink "$entry")"
    else
      printf 'file %s %s\n' "$relative" "$(wc -c < "$entry" | tr -d ' ')"
      [ "${entry##*/}" != "package.json" ] || cksum "$entry"
    fi
  done | cksum | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ]; then
  shift
  for file in "$@"; do
    if [ -f "$file" ]; then cksum "$file"; fi
    if [ -d "$file" ]; then
      find "$file" -type f -print | LC_ALL=C sort | while IFS= read -r nested; do
        printf '%s\n' "$nested"
        cksum "$nested"
      done
    fi
  done | cksum | awk '{print $1}'
fi
exit 0
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).unwrap();

    let fake_yarn = fake_bin.join("yarn");
    fs::write(
        &fake_yarn,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    printf '%s\n' '1.22.22'
    ;;
  config)
    [ "${2:-}" = "--json" ] || exit 2
    current="$PWD"
    linker=pnp
    while [ "$current" != "${current%/*}" ]; do
      if [ -f "$current/.yarnrc.yml" ] && grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:' "$current/.yarnrc.yml"; then
        linker="$(sed -n 's/^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*\([^[:space:]#]*\).*/\1/p' "$current/.yarnrc.yml" | tail -1)"
        break
      fi
      current="${current%/*}"
    done
    printf '%s\n' "{\"key\":\"nodeLinker\",\"effective\":\"$linker\"}"
    printf '%s\n' "{\"key\":\"cacheFolder\",\"effective\":\"$PWD/.yarn/cache\"}"
    printf '%s\n' "{\"key\":\"installStatePath\",\"effective\":\"$PWD/.yarn/install-state.gz\"}"
    printf '%s\n' "{\"key\":\"pnpUnpluggedFolder\",\"effective\":\"$PWD/.yarn/unplugged\"}"
    printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
    printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":false}'
    ;;
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$INSTALL_COUNT"
    pwd > "$INSTALL_CWD"
    if [ "${FAIL_INSTALL:-0}" = "1" ]; then exit 9; fi
    printf '%s\n' 'generated classic pnp loader' > .pnp.js
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_yarn, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_cwd = repo.join("install-cwd");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let resolve_spec = || {
        let output = std::process::Command::new("bash")
            .args([
                "scripts/check-webapps.sh",
                "package-manager-spec",
                "apps/web",
            ])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "nested Yarn spec resolution failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let run_mode = |mode: &str, fail_install: bool| {
        let output = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("FAIL_INSTALL", if fail_install { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail_install,
            "app-local Yarn web {mode} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    let node_version_file = std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", "node-version-file", "apps/web"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        node_version_file.status.success(),
        "app-local Node version resolution failed: {}",
        String::from_utf8_lossy(&node_version_file.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&node_version_file.stdout).trim(),
        "apps/web/.node-version"
    );
    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert!(workflow.contains("node-version-file: ${{ steps.node-version.outputs.path }}"));
    assert_eq!(resolve_spec(), "yarn@1.22.20");

    run_mode("bootstrap", false);
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(repo.join("apps/web"))
            .unwrap()
            .display()
            .to_string()
    );
    assert!(
        !repo.join("yarn.lock").exists(),
        "bootstrap replaced the adopted app-local dependency scope with a root lock"
    );
    assert!(repo.join("apps/web/.pnp.js").exists());

    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"yarn@1.22.19"}"#,
    )
    .unwrap();
    assert_eq!(resolve_spec(), "yarn@1.22.20");
    run_mode("lint", true);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "3");

    fs::write(
        repo.join("apps/package.json"),
        r#"{"private":true,"packageManager":"yarn@1.22.18"}"#,
    )
    .unwrap();
    assert_eq!(resolve_spec(), "yarn@1.22.18");
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "4");

    fs::create_dir_all(repo.join("apps/.yarn/releases")).unwrap();
    fs::write(repo.join("apps/.yarn/releases/yarn.cjs"), "runtime-v1\n").unwrap();
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "5");
    fs::write(repo.join("apps/.yarn/releases/yarn.cjs"), "runtime-v2\n").unwrap();
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "6");
}

#[cfg(unix)]
#[test]
fn generated_root_yarn_receipt_tracks_nested_app_authority_and_runtime_assets() {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("root-yarn-nested-authority");
    let app_dir = "apps/group/web";

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("root-yarn-nested-authority".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("yarn".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: app_dir.into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join(app_dir)).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"yarn@4.17.1","workspaces":["apps/group/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("yarn.lock"), "__metadata:\n  version: 8\n").unwrap();
    fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
    fs::write(
        repo.join("apps/group/package.json"),
        r#"{"private":true,"packageManager":"yarn@3.8.7"}"#,
    )
    .unwrap();
    fs::write(
        repo.join(app_dir).join("package.json"),
        r#"{"name":"web","private":true,"scripts":{"lint":"true"},"dependencies":{"dependency":"1.0.0"}}"#,
    )
    .unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_yarn = fake_bin.join("yarn");
    fs::write(
        &fake_yarn,
        r#"#!/bin/sh
set -eu
case "$PWD" in
  */apps/group/*)
    if [ -n "${YARN_EXECUTION_MARKER:-}" ] && [ -L "${SYMLINKED_YARN_AUTHORITY:-}" ]; then
      : > "$YARN_EXECUTION_MARKER"
    fi
    if [ -n "${YARN_PATH_PROBE:-}" ] && [ -f "$YARN_PATH_PROBE" ]; then
      "$YARN_PATH_PROBE"
    fi
    ;;
esac
case "${1:-}" in
  --version)
    case "$PWD" in
      */apps/group/web) printf '%s\n' '3.8.7' ;;
      *) printf '%s\n' '4.17.1' ;;
    esac
    ;;
  config)
    [ "${2:-}" = "--json" ] || exit 2
    printf '%s\n' '{"key":"nodeLinker","effective":"node-modules"}'
    printf '{"key":"cacheFolder","effective":"%s/.yarn/cache"}\n' "$PWD"
    printf '{"key":"installStatePath","effective":"%s/.yarn/install-state.gz"}\n' "$PWD"
    printf '{"key":"pnpUnpluggedFolder","effective":"%s/.yarn/unplugged"}\n' "$PWD"
    printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
    printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":false}'
    ;;
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    mkdir -p node_modules/dependency
    printf '%s\n' '{"name":"dependency","version":"1.0.0"}' > node_modules/dependency/package.json
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_yarn, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let yarn_execution_marker = repo.join("unexpected-yarn-execution");
    let symlinked_yarn_authority = repo.join("apps/group/.yarn");
    let yarn_path_probe = repo.join("tools/yarn.cjs");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = |mode: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, app_dir])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("YARN_EXECUTION_MARKER", &yarn_execution_marker)
            .env("SYMLINKED_YARN_AUTHORITY", &symlinked_yarn_authority)
            .env("YARN_PATH_PROBE", &yarn_path_probe)
            .output()
            .unwrap()
    };
    let resolve_spec = || {
        let output = command("package-manager-spec");
        assert!(
            output.status.success(),
            "nested root-workspace Yarn spec failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let assert_install = |label: &str| {
        let output = command("dependencies-install");
        assert!(
            output.status.success(),
            "{label} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    assert_eq!(resolve_spec(), "yarn@3.8.7");
    assert_install("initial nested-authority install");
    assert!(command("dependencies-ready").status.success());
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(
        repo.join("apps/group/package.json"),
        r#"{"private":true,"packageManager":"yarn@3.8.8"}"#,
    )
    .unwrap();
    assert_eq!(resolve_spec(), "yarn@3.8.8");
    assert!(!command("dependencies-ready").status.success());
    assert_install("changed intermediate package authority install");
    assert!(command("dependencies-ready").status.success());

    fs::write(
        repo.join("apps/group/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();
    assert!(!command("dependencies-ready").status.success());
    assert_install("changed intermediate Yarn config install");

    fs::create_dir_all(repo.join("apps/group/.yarn/releases")).unwrap();
    let runtime = repo.join("apps/group/.yarn/releases/yarn.cjs");
    fs::write(&runtime, "runtime-v1\n").unwrap();
    assert!(!command("dependencies-ready").status.success());
    assert_install("added intermediate Yarn runtime install");
    fs::write(&runtime, "runtime-v2\n").unwrap();
    assert!(!command("dependencies-ready").status.success());
    assert_install("changed intermediate Yarn runtime install");
    assert!(command("dependencies-ready").status.success());

    let outside_config_runtime = temp.path().join("outside-yarn-config-runtime");
    fs::create_dir_all(&outside_config_runtime).unwrap();
    fs::write(
        outside_config_runtime.join("yarn.cjs"),
        r#"#!/bin/sh
set -eu
: > "$YARN_EXECUTION_MARKER"
"#,
    )
    .unwrap();
    fs::set_permissions(
        outside_config_runtime.join("yarn.cjs"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    symlink(&outside_config_runtime, repo.join("tools")).unwrap();
    for (label, config) in [
        ("yarnPath", "yarnPath: ../../tools/yarn.cjs\n"),
        (
            "plugin path",
            "plugins:\n  - path: ../../tools/yarn.cjs\n    spec: external-probe\n",
        ),
    ] {
        fs::write(repo.join("apps/group/.yarnrc.yml"), config).unwrap();
        let _ = fs::remove_file(&yarn_execution_marker);
        let output = command("dependencies-ready");
        assert!(!output.status.success(), "external {label} was accepted");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("symbolic link"),
            "external {label} did not fail at the authority boundary: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !yarn_execution_marker.exists(),
            "Yarn executed before external {label} was rejected"
        );
    }
    fs::write(
        repo.join("apps/group/.yarnrc.yml"),
        "{ yarnPath: ../../tools/yarn.cjs }\n",
    )
    .unwrap();
    let _ = fs::remove_file(&yarn_execution_marker);
    let flow_config = command("dependencies-ready");
    assert!(!flow_config.status.success());
    assert!(
        String::from_utf8_lossy(&flow_config.stderr).contains("unsupported top-level YAML"),
        "flow-style Yarn runtime authority did not fail closed: {}",
        String::from_utf8_lossy(&flow_config.stderr)
    );
    assert!(
        !yarn_execution_marker.exists(),
        "Yarn executed before a flow-style runtime authority was rejected"
    );
    fs::write(
        repo.join("apps/group/.yarnrc.yml"),
        "\"yarn\\u0050ath\": ../../tools/yarn.cjs\n",
    )
    .unwrap();
    let _ = fs::remove_file(&yarn_execution_marker);
    let escaped_key = command("dependencies-ready");
    assert!(!escaped_key.status.success());
    assert!(
        String::from_utf8_lossy(&escaped_key.stderr).contains("unsupported top-level YAML"),
        "escaped Yarn runtime key did not fail closed: {}",
        String::from_utf8_lossy(&escaped_key.stderr)
    );
    assert!(
        !yarn_execution_marker.exists(),
        "Yarn executed before an escaped runtime key was rejected"
    );

    fs::write(
        repo.join("apps/group/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();
    for (label, classic_config) in [
        ("canonical", "yarn-path ../../tools/yarn.cjs\n"),
        ("quoted", "\"yarn-path\" \"../../tools/yarn.cjs\"\n"),
    ] {
        fs::write(repo.join("apps/group/.yarnrc"), classic_config).unwrap();
        let _ = fs::remove_file(&yarn_execution_marker);
        let output = command("dependencies-ready");
        assert!(
            !output.status.success(),
            "{label} Classic yarn-path was accepted"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("yarn-path")
                || String::from_utf8_lossy(&output.stderr).contains("symbolic link"),
            "{label} Classic yarn-path rejection was not diagnostic: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !yarn_execution_marker.exists(),
            "Yarn executed before {label} Classic yarn-path was rejected"
        );
    }
    fs::remove_file(repo.join("apps/group/.yarnrc")).unwrap();

    for environment_name in [
        "YARN_RC_FILENAME",
        "YARN_YARN_PATH",
        "YARN_PLUGINS",
        "NPM_CONFIG_YARN_PATH",
    ] {
        let _ = fs::remove_file(&yarn_execution_marker);
        let output = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", app_dir])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("YARN_EXECUTION_MARKER", &yarn_execution_marker)
            .env("SYMLINKED_YARN_AUTHORITY", &symlinked_yarn_authority)
            .env("YARN_PATH_PROBE", &yarn_path_probe)
            .env(environment_name, &yarn_path_probe)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "ambient {environment_name} authority was accepted"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(environment_name),
            "ambient {environment_name} rejection was not diagnostic: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !yarn_execution_marker.exists(),
            "Yarn executed before ambient {environment_name} was rejected"
        );
    }
    fs::remove_file(repo.join("tools")).unwrap();
    fs::write(
        repo.join("apps/group/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();
    assert!(command("dependencies-ready").status.success());

    let authority_failure_env = repo.join("yarn-authority-producer-failure");
    fs::write(
        &authority_failure_env,
        r#"pwd() {
  if [ "${FUNCNAME[1]:-}" = "yarn_scope_authority_paths" ]; then
    return 41
  fi
  builtin pwd "$@"
}
"#,
    )
    .unwrap();
    let authority_failure = |mode: &str| {
        std::process::Command::new("/bin/bash")
            .args(["scripts/check-webapps.sh", mode, app_dir])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("YARN_EXECUTION_MARKER", &yarn_execution_marker)
            .env("SYMLINKED_YARN_AUTHORITY", &symlinked_yarn_authority)
            .env("YARN_PATH_PROBE", &yarn_path_probe)
            .env("BASH_ENV", &authority_failure_env)
            .output()
            .unwrap()
    };
    let invalid_readiness = authority_failure("dependencies-ready");
    assert!(
        invalid_readiness
            .status
            .code()
            .is_some_and(|status| status >= 2),
        "Yarn authority producer failure was downgraded to stale readiness: {:?}\n{}",
        invalid_readiness.status.code(),
        String::from_utf8_lossy(&invalid_readiness.stderr)
    );

    let stamp = repo.join(".agent/tmp/web-dependencies/root.sha256");
    fs::remove_file(&stamp).unwrap();
    let install_count_before_failure = fs::read_to_string(&install_count)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let invalid_install = authority_failure("dependencies-install");
    assert!(
        invalid_install
            .status
            .code()
            .is_some_and(|status| status >= 2),
        "Yarn install published an incomplete authority fingerprint: {:?}\n{}",
        invalid_install.status.code(),
        String::from_utf8_lossy(&invalid_install.stderr)
    );
    assert!(!stamp.exists());
    assert!(!repo.join("node_modules/.jig-web-dependencies-v3").exists());
    assert_eq!(
        fs::read_to_string(&install_count)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap(),
        install_count_before_failure + 1
    );
    assert_install("restore after Yarn authority producer failure");
    assert!(command("dependencies-ready").status.success());

    let outside_yarn = temp.path().join("outside-yarn-authority");
    fs::create_dir_all(outside_yarn.join("releases")).unwrap();
    fs::write(outside_yarn.join("releases/yarn.cjs"), "runtime-v2\n").unwrap();
    fs::remove_dir_all(repo.join("apps/group/.yarn")).unwrap();
    symlink(&outside_yarn, repo.join("apps/group/.yarn")).unwrap();
    let symlinked_authority = command("dependencies-ready");
    assert!(!symlinked_authority.status.success());
    assert!(
        String::from_utf8_lossy(&symlinked_authority.stderr).contains("symbolic link"),
        "Yarn fingerprint followed an out-of-repository authority symlink: {}",
        String::from_utf8_lossy(&symlinked_authority.stderr)
    );
    assert!(
        !yarn_execution_marker.exists(),
        "Yarn executed before inherited authority symlinks were rejected"
    );

    fs::remove_file(repo.join("apps/group/.yarn")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/group/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("apps/group/package.json"), r#"{"private":true}"#).unwrap();
    let outside_lock = temp.path().join("outside-yarn.lock");
    fs::write(&outside_lock, "# yarn lockfile v1\n").unwrap();
    fs::remove_file(repo.join("yarn.lock")).unwrap();
    symlink(&outside_lock, repo.join("yarn.lock")).unwrap();
    let symlinked_lock = command("package-manager-spec");
    assert_eq!(symlinked_lock.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&symlinked_lock.stderr).contains("symbolic link"),
        "Yarn package-manager resolution followed an out-of-repository lock symlink: {}",
        String::from_utf8_lossy(&symlinked_lock.stderr)
    );
    let _ = fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies"));
    let unready_symlinked_lock = command("dependencies-ready");
    assert_eq!(
        unready_symlinked_lock.status.code(),
        Some(2),
        "an absent receipt must not downgrade malformed Yarn lock authority"
    );
    let install_count_before_hard_failure = fs::read_to_string(&install_count).unwrap();
    let install_with_symlinked_lock = command("dependencies-install");
    assert_eq!(install_with_symlinked_lock.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(&install_count).unwrap(),
        install_count_before_hard_failure,
        "a hard readiness failure must not invoke Yarn install"
    );
    let node_version_with_symlinked_lock = command("node-version-file");
    assert_eq!(
        node_version_with_symlinked_lock.status.code(),
        Some(2),
        "root Node-version fallback must not mask malformed dependency scope authority"
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_preserve_and_override_inherited_yarn_berry_linker() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("yarn-linker-inheritance");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("yarn-linker-inheritance".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("yarn".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"yarn@4.17.1","workspaces":["tools/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","private":true,"packageManager":"yarn@4.17.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/yarn.lock"),
        "__metadata:\n  version: 8\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/.yarnrc.yml"),
        "checksumBehavior: reset\n",
    )
    .unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_yarn = fake_bin.join("yarn");
    fs::write(
        &fake_yarn,
r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    printf '%s\n' '4.17.1'
    ;;
  config)
    [ "${2:-}" = "--json" ] || exit 2
    current="$PWD"
    linker=pnp
    while [ "$current" != "${current%/*}" ]; do
      if [ -f "$current/.yarnrc.yml" ] && grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:' "$current/.yarnrc.yml"; then
        linker="$(sed -n 's/^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*\([^[:space:]#]*\).*/\1/p' "$current/.yarnrc.yml" | tail -1)"
        break
      fi
      current="${current%/*}"
    done
    printf '%s\n' "{\"key\":\"nodeLinker\",\"effective\":\"$linker\"}"
    printf '%s\n' "{\"key\":\"cacheFolder\",\"effective\":\"$PWD/.yarn/cache\"}"
    printf '%s\n' "{\"key\":\"installStatePath\",\"effective\":\"$PWD/.yarn/install-state.gz\"}"
    printf '%s\n' "{\"key\":\"pnpUnpluggedFolder\",\"effective\":\"$PWD/.yarn/unplugged\"}"
    printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
    printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":false}'
    ;;
  install)
    if grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*pnp' .yarnrc.yml; then
      if [ -d node_modules ]; then rm -rf node_modules; fi
      printf '%s\n' 'const RAW_RUNTIME_STATE = '\''{"packageRegistryData":[]}'\'';' > .pnp.cjs
      mkdir -p .yarn
      printf '%s\n' state > .yarn/install-state.gz
    else
      rm -f .pnp.cjs .pnp.js
      mkdir -p node_modules/test-package
      printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_yarn, fs::Permissions::from_mode(0o755)).unwrap();
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = |mode: &str| {
        let mut command = std::process::Command::new("bash");
        command.args(["scripts/check-webapps.sh", mode]);
        if mode == "dependencies-ready" {
            command.arg("apps/web");
        }
        command
            .current_dir(&repo)
            .env("PATH", &path)
            .output()
            .unwrap()
    };

    let inherited = command("bootstrap");
    assert!(
        inherited.status.success(),
        "inherited Yarn linker bootstrap failed: {}",
        String::from_utf8_lossy(&inherited.stderr)
    );
    assert!(repo.join("apps/web/node_modules").is_dir());
    assert!(command("dependencies-ready").status.success());

    fs::write(
        repo.join("apps/web/.yarnrc.yml"),
        "nodeLinker: pnp\nchecksumBehavior: reset\n",
    )
    .unwrap();
    assert!(!command("dependencies-ready").status.success());
    let overridden = command("bootstrap");
    assert!(
        overridden.status.success(),
        "app Yarn linker override failed: {}",
        String::from_utf8_lossy(&overridden.stderr)
    );
    assert!(repo.join("apps/web/.pnp.cjs").is_file());
    assert!(command("dependencies-ready").status.success());
}

#[cfg(unix)]
#[test]
fn generated_yarn_berry_accepts_historical_inline_loader_formats() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for (format, loader) in [("object", ".pnp.js"), ("json-string", ".pnp.cjs")] {
        let repo = temp.path().join(format!("yarn-berry-{format}"));
        run_init(InitOpts {
            path: repo.clone(),
            scaffold: ScaffoldOpts::default(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some(format!("yarn-berry-{format}")),
                sqlx_enabled: Some(false),
                web_package_manager: Some("yarn".into()),
                frontend_apps: vec![FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let app = repo.join("apps/web");
        fs::create_dir_all(&app).unwrap();
        fs::write(
            app.join("package.json"),
            r#"{"name":"web","private":true,"packageManager":"yarn@2.4.3","dependencies":{"dependency":"1.0.0"}}"#,
        )
        .unwrap();
        fs::write(app.join("yarn.lock"), "__metadata:\n  version: 4\n").unwrap();

        let fake_bin = repo.join("fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_yarn = fake_bin.join("yarn");
        fs::write(
            &fake_yarn,
            r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    printf '%s\n' '2.4.3'
    ;;
  config)
    [ "${2:-}" = "--json" ] || exit 2
    printf '%s\n' '{"key":"nodeLinker","effective":"pnp"}'
    printf '%s\n' "{\"key\":\"cacheFolder\",\"effective\":\"$PWD/.yarn/cache\"}"
    printf '%s\n' "{\"key\":\"installStatePath\",\"effective\":\"$PWD/.yarn/install-state.gz\"}"
    printf '%s\n' "{\"key\":\"pnpUnpluggedFolder\",\"effective\":\"$PWD/.yarn/unplugged\"}"
    printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
    if [ "${TEST_ESM:-0}" = "1" ]; then
      printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":true}'
    fi
    ;;
  install)
    mkdir -p .yarn/cache
    printf '%s\n' archive > .yarn/cache/dependency.zip
    printf '%s\n' state > .yarn/install-state.gz
    if [ "${TEST_ESM:-0}" = "1" ]; then
      printf '%s\n' esm-loader > .pnp.loader.mjs
    else
      rm -f .pnp.loader.mjs
    fi
    if [ "$TEST_FORMAT" = object ]; then
      rm -f .pnp.cjs
      {
        printf '%s\n' 'function $$SETUP_STATE(hydrateRuntimeState, basePath) {'
        printf '%s\n' '  return hydrateRuntimeState({'
        printf '%s\n' '    "packageRegistryData": [[null, [[null, {'
        printf '%s\n' '      "packageLocation": "./.yarn/cache/dependency.zip/node_modules/dependency/"'
        printf '%s\n' '    }]]]]'
        printf '%s\n' '  }, {basePath: basePath || __dirname});'
        printf '%s\n' '}'
      } > .pnp.js
    else
      rm -f .pnp.js
      printf '%s\n' 'function $$SETUP_STATE(hydrateRuntimeState, basePath) {' > .pnp.cjs
      printf '%s\n' '  return hydrateRuntimeState(JSON.parse('\''{"packageRegistryData":[[null,[[null,{"packageLocation":"./.yarn/cache/dependency.zip/node_modules/dependency/"}]]]]}'\''), {basePath: basePath || __dirname});' >> .pnp.cjs
      printf '%s\n' '}' >> .pnp.cjs
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&fake_yarn, fs::Permissions::from_mode(0o755)).unwrap();
        let mut path = OsString::from(fake_bin.as_os_str());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let command = |mode: &str, esm: bool| {
            std::process::Command::new("bash")
                .args(["scripts/check-webapps.sh", mode, "apps/web"])
                .current_dir(&repo)
                .env("PATH", &path)
                .env("TEST_FORMAT", format)
                .env("TEST_ESM", if esm { "1" } else { "0" })
                .output()
                .unwrap()
        };

        let installed = command("dependencies-install", false);
        assert!(
            installed.status.success(),
            "{format} install failed: {}",
            String::from_utf8_lossy(&installed.stderr)
        );
        assert!(app.join(loader).is_file());
        assert!(command("dependencies-ready", false).status.success());

        fs::rename(
            app.join(".yarn/cache/dependency.zip"),
            app.join(".yarn/cache/dependency.zip.missing"),
        )
        .unwrap();
        assert!(!command("dependencies-ready", false).status.success());
        fs::rename(
            app.join(".yarn/cache/dependency.zip.missing"),
            app.join(".yarn/cache/dependency.zip"),
        )
        .unwrap();
        fs::write(app.join(".yarn/cache/unrelated.zip"), "unrelated\n").unwrap();
        assert!(command("dependencies-ready", false).status.success());

        let loader_path = app.join(loader);
        let loader_length = fs::metadata(&loader_path).unwrap().len();
        let oversized_loader = fs::OpenOptions::new()
            .write(true)
            .open(&loader_path)
            .unwrap();
        oversized_loader.set_len(64 * 1024 * 1024 + 1).unwrap();
        drop(oversized_loader);
        assert!(
            !command("dependencies-ready", false).status.success(),
            "{format} accepted an oversized parsed loader"
        );
        let restored_loader = fs::OpenOptions::new()
            .write(true)
            .open(&loader_path)
            .unwrap();
        restored_loader.set_len(loader_length).unwrap();
        drop(restored_loader);
        assert!(command("dependencies-ready", false).status.success());

        if format == "object" {
            assert!(!command("dependencies-ready", true).status.success());
            assert!(command("dependencies-install", true).status.success());
            assert!(command("dependencies-ready", true).status.success());
            fs::write(app.join(".pnp.loader.mjs"), "changed\n").unwrap();
            assert!(
                !command("dependencies-ready", true).status.success(),
                "an explicit pnpEnableEsmLoader=true did not attest the ESM loader"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn generated_yarn_classic_stamps_actual_artifacts_and_proves_referenced_packages() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("yarn-classic-artifacts");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("yarn-classic-artifacts".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("yarn".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    let app_manifest = repo.join("apps/web/package.json");
    fs::write(
        &app_manifest,
        r#"{"name":"web","private":true,"packageManager":"yarn@1.22.22","workspaces":["packages/*"],"dependencies":{"dependency":"1.0.0","workspace":"1.0.0"},"installConfig":{"pnp":true}}"#,
    )
    .unwrap();
    fs::write(repo.join("apps/web/yarn.lock"), "# yarn lockfile v1\n").unwrap();
    let app = repo.join("apps/web");
    fs::write(app.join(".node-version"), "22.22.2\n").unwrap();
    fs::create_dir_all(app.join("packages/workspace")).unwrap();
    fs::write(
        app.join("packages/workspace/package.json"),
        r#"{"name":"workspace","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(app.join("packages/workspace/source.js"), "source-v1\n").unwrap();

    let cache = repo.join("classic-cache");

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_yarn = fake_bin.join("yarn");
    fs::write(
        &fake_yarn,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    printf '%s\n' '1.22.22'
    ;;
  cache)
    [ "${2:-}" = "dir" ] || exit 2
    printf '%s/v6\n' "$TEST_CACHE"
    ;;
  config)
    [ "${2:-}" = "list" ] && [ "${3:-}" = "--json" ] || exit 2
    printf '%s\n' '{"type":"info","data":"yarn config"}'
    printf '%s\n' "{\"type\":\"inspect\",\"data\":{\"--pnp\":true,\"cache-folder\":\"$TEST_CACHE\"}}"
    ;;
  install)
    mkdir -p "$TEST_CACHE/v6/dependency/node_modules/dependency"
    printf '%s\n' '{"name":"dependency","version":"1.0.0"}' > "$TEST_CACHE/v6/dependency/node_modules/dependency/package.json"
    if [ "${TEST_PNP:-0}" = "1" ]; then
      rm -rf node_modules
      {
        printf '%s\n' 'const path = require("path");'
        printf '%s\n' 'let packageInformationStores = new Map(['
        printf '%s\n' '  ["dependency", new Map([["1.0.0", {'
        printf '    packageLocation: path.resolve(__dirname, "%s/v6/dependency/node_modules/dependency/"),\n' "$TEST_CACHE"
        printf '%s\n' '  }]])],'
        printf '%s\n' '  ["workspace", new Map([["1.0.0", {'
        printf '%s\n' '    packageLocation: path.resolve(__dirname, "./packages/workspace/"),'
        printf '%s\n' '  }]])],'
        printf '%s\n' '  [null, new Map([[null, {'
        printf '%s\n' '    packageLocation: path.resolve(__dirname, "./"),'
        printf '%s\n' '  }]])],'
        printf '%s\n' ']);'
        printf '%s\n' 'let locatorsByLocations = new Map(['
        printf '%s\n' ']);'
      } > .pnp.js
    else
      rm -f .pnp.js .pnp.cjs
      mkdir -p node_modules/dependency
      printf '%s\n' '{"name":"dependency"}' > node_modules/dependency/package.json
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_yarn, fs::Permissions::from_mode(0o755)).unwrap();
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = |mode: &str, pnp: bool, override_value: Option<&str>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("TEST_PNP", if pnp { "1" } else { "0" })
            .env("TEST_CACHE", &cache);
        if let Some(value) = override_value {
            command.env("YARN_PLUGNPLAY_OVERRIDE", value);
        }
        command.output().unwrap()
    };
    let assert_success = |output: std::process::Output, label: &str| {
        assert!(
            output.status.success(),
            "{label} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    assert_success(
        command("dependencies-install", true, None),
        "Classic PnP artifact install",
    );
    assert_success(
        command("dependencies-ready", true, None),
        "Classic PnP artifact readiness",
    );

    fs::write(app.join("packages/workspace/source.js"), "source-v2\n").unwrap();
    assert_success(
        command("dependencies-ready", true, None),
        "workspace source edit should not invalidate dependencies",
    );

    fs::write(app.join(".yarnrc"), "--pnp false\n").unwrap();
    assert!(
        !command("dependencies-ready", true, None).status.success(),
        "a Classic RC change did not invalidate readiness"
    );
    assert_success(
        command("dependencies-install", true, None),
        "actual PnP artifact after RC change",
    );
    assert!(
        !command("dependencies-ready", true, Some("0"))
            .status
            .success()
    );
    assert_success(
        command("dependencies-install", true, Some("0")),
        "actual PnP artifact with override change",
    );

    let workspace_manifest = app.join("packages/workspace/package.json");
    let workspace_manifest_contents = fs::read_to_string(&workspace_manifest).unwrap();
    fs::remove_file(&workspace_manifest).unwrap();
    assert!(
        !command("dependencies-ready", true, Some("0"))
            .status
            .success(),
        "a missing referenced workspace manifest did not invalidate readiness"
    );
    fs::write(&workspace_manifest, workspace_manifest_contents).unwrap();
    assert_success(
        command("dependencies-ready", true, Some("0")),
        "restored workspace manifest",
    );

    let cache_package = cache.join("v6/dependency");
    let missing_cache_package = cache.join("v6/dependency.missing");
    fs::rename(&cache_package, &missing_cache_package).unwrap();
    assert!(
        !command("dependencies-ready", true, Some("0"))
            .status
            .success(),
        "a missing referenced Classic cache package did not invalidate readiness"
    );
    fs::rename(&missing_cache_package, &cache_package).unwrap();
    fs::create_dir_all(cache.join("v6/unrelated/node_modules/unrelated")).unwrap();
    fs::write(
        cache.join("v6/unrelated/node_modules/unrelated/package.json"),
        "{}\n",
    )
    .unwrap();
    assert_success(
        command("dependencies-ready", true, Some("0")),
        "unrelated Classic cache addition",
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_use_ignore_workspace_and_track_parent_config_for_standalone_pnpm() {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("app-local-pnpm");

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("app-local-pnpm".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("pnpm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.12.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.11.0"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), "packages:\n  - tools/*\n").unwrap();
    fs::write(
        repo.join(".npmrc"),
        "shared-workspace-lockfile=false\nregistry=https://registry.npmjs.org/\n",
    )
    .unwrap();
    fs::write(repo.join(".pnpmfile.cjs"), "module.exports = {}\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_node = fake_bin.join("fake-node");
    fs::write(
        &fake_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  if kill -0 "$3" 2>/dev/null; then printf '%s\n' live; else printf '%s\n' stale; fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-workspace-metadata" ]; then
  [ "${3:-}" != "contains" ]
  exit $?
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-node-modules-proof" ]; then
  root="$3/node_modules"
  entries="$(find "$root" -mindepth 1 \( -type f -o -type l \) ! -name '.jig-web-dependencies-v3' ! -name '.jig-web-dependencies-v3.tmp.*' ! -path "$root/.cache/*" ! -path "$root/.vite/*" ! -path "$root/.tmp/*" ! -name '.DS_Store' -print | LC_ALL=C sort)"
  [ -n "$entries" ] || exit 1
  printf '%s\n' "$entries" | while IFS= read -r entry; do
    relative="${entry#"$root"/}"
    if [ -L "$entry" ]; then
      printf 'link %s %s\n' "$relative" "$(readlink "$entry")"
    else
      printf 'file %s %s\n' "$relative" "$(wc -c < "$entry" | tr -d ' ')"
      [ "${entry##*/}" != "package.json" ] || cksum "$entry"
    fi
  done | cksum | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ]; then
  shift
  for file in "$@"; do
    if [ -f "$file" ]; then cksum "$file"; fi
    if [ -d "$file" ]; then
      find "$file" -type f -print | LC_ALL=C sort | while IFS= read -r nested; do
        printf '%s\n' "$nested"
        cksum "$nested"
      done
    fi
  done | cksum | awk '{print $1}'
fi
exit 0
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).unwrap();

    let fake_pnpm = fake_bin.join("pnpm");
    fs::write(
        &fake_pnpm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    if [ -n "${PNPM_VERSION_FILE:-}" ] && [ -f "$PNPM_VERSION_FILE" ]; then
      cat "$PNPM_VERSION_FILE"
    else
      printf '%s\n' "${PNPM_VERSION:-10.12.1}"
    fi
    ;;
  config)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
    if [ -n "${PNPM_CONFIG_JSON:-}" ]; then printf '%s\n' "$PNPM_CONFIG_JSON"; exit 0; fi
    shared="${PNPM_SHARED_WORKSPACE_LOCKFILE:-true}"
    global="${PNPM_ENABLE_GLOBAL_VIRTUAL_STORE:-false}"
    case "$shared:$global" in
      true:true|true:false|false:true|false:false)
        printf '{"sharedWorkspaceLockfile":%s,"enableGlobalVirtualStore":%s}\n' "$shared" "$global"
        ;;
      true:undefined|false:undefined)
        printf '{"sharedWorkspaceLockfile":%s}\n' "$shared"
        ;;
      undefined:true|undefined:false)
        printf '{"enableGlobalVirtualStore":%s}\n' "$global"
        ;;
      undefined:undefined) printf '%s\n' '{}' ;;
      *) exit 2 ;;
    esac
    ;;
  pkg)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    if [ -n "${PNPM_PKG_JSON:-}" ]; then printf '%s\n' "$PNPM_PKG_JSON"; else printf '%s\n' '{}'; fi
    ;;
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$INSTALL_COUNT"
    pwd > "$INSTALL_CWD"
    if [ "${FAIL_INSTALL:-0}" = "1" ]; then exit 9; fi
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    printf '%s\n' 'layout-v1' > node_modules/.modules.yaml
    if [ -n "${PNPM_DRIFT_TO:-}" ] && [ -n "${PNPM_VERSION_FILE:-}" ]; then
      printf '%s\n' "$PNPM_DRIFT_TO" > "$PNPM_VERSION_FILE"
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_pnpm, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_cwd = repo.join("install-cwd");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let resolved_spec = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "package-manager-spec",
            "apps/web",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(resolved_spec.status.success());
    assert_eq!(
        String::from_utf8(resolved_spec.stdout).unwrap().trim(),
        "pnpm@10.11.0"
    );
    let run_mode = |mode: &str, fail_install: bool| {
        let output = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("FAIL_INSTALL", if fail_install { "1" } else { "0" })
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail_install,
            "app-local pnpm web {mode} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run_mode("bootstrap", false);
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(repo.join("apps/web"))
            .unwrap()
            .display()
            .to_string()
    );
    assert!(!repo.join("pnpm-lock.yaml").exists());
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    assert!(web_check.contains("pnpm install --ignore-workspace"));
    run_mode("lint", false);
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let dependencies_ready = || {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .status()
            .unwrap()
            .success()
    };
    let node_modules = repo.join("apps/web/node_modules");
    for workspace_state_name in [
        ".pnpm-workspace-state.json",
        ".pnpm-workspace-state-v1.json",
    ] {
        let workspace_state = node_modules.join(workspace_state_name);
        fs::write(
            &workspace_state,
            r#"{"lastValidatedTimestamp":1,"settings":{"dedupeInjectedDeps":true}}"#,
        )
        .unwrap();
        assert!(
            dependencies_ready(),
            "pnpm's root validation cache {workspace_state_name} invalidated an otherwise ready install"
        );
        fs::write(
            &workspace_state,
            r#"{"lastValidatedTimestamp":1784300000000,"settings":{"dedupeInjectedDeps":true}}"#,
        )
        .unwrap();
        assert!(
            dependencies_ready(),
            "a {workspace_state_name} timestamp/size rewrite invalidated readiness"
        );

        let nested_workspace_state = node_modules.join("test-package").join(workspace_state_name);
        fs::write(&nested_workspace_state, "nested package-owned state\n").unwrap();
        assert!(
            !dependencies_ready(),
            "nested {workspace_state_name} escaped the structural proof"
        );
        fs::remove_file(&nested_workspace_state).unwrap();
        assert!(dependencies_ready());

        fs::remove_file(&workspace_state).unwrap();
        assert!(
            dependencies_ready(),
            "deleting pnpm's volatile root cache {workspace_state_name} invalidated readiness"
        );
        fs::create_dir(&workspace_state).unwrap();
        assert!(
            !dependencies_ready(),
            "a directory replacing {workspace_state_name} escaped the structural proof"
        );
        fs::remove_dir(&workspace_state).unwrap();
        assert!(dependencies_ready());

        symlink("test-package/package.json", &workspace_state).unwrap();
        assert!(
            !dependencies_ready(),
            "a symlink replacing {workspace_state_name} escaped the structural proof"
        );
        fs::remove_file(&workspace_state).unwrap();
        assert!(dependencies_ready());
    }

    let bin_dir = node_modules.join(".bin");
    fs::create_dir(&bin_dir).unwrap();
    fs::write(bin_dir.join("test-package"), "shim\n").unwrap();
    assert!(
        !dependencies_ready(),
        "pnpm's .bin layout escaped the structural proof"
    );
    fs::remove_dir_all(&bin_dir).unwrap();
    assert!(dependencies_ready());

    let modules_metadata = node_modules.join(".modules.yaml");
    fs::write(&modules_metadata, "layout-v2\n").unwrap();
    assert!(
        !dependencies_ready(),
        "same-sized semantic pnpm metadata mutation escaped the structural proof"
    );
    fs::write(&modules_metadata, "layout-v1\n").unwrap();
    assert!(dependencies_ready());

    fs::write(repo.join("apps/web/local.patch"), "patch contents\n").unwrap();
    fs::write(
        repo.join("apps/web/pnpm-workspace.yaml"),
        "patchedDependencies:\n  dependency@1: local.patch\n",
    )
    .unwrap();
    for version in ["10.33.4", "11.13.1"] {
        let rejected = std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "lint"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("FAIL_INSTALL", "0")
            .env("PNPM_VERSION", version)
            .output()
            .unwrap();
        assert!(
            !rejected.status.success(),
            "pnpm {version} accepted scope-local YAML patches that --ignore-workspace would ignore"
        );
        let stderr = String::from_utf8_lossy(&rejected.stderr);
        assert!(stderr.contains("apps/web/pnpm-workspace.yaml"), "{stderr}");
        assert!(stderr.contains("--ignore-workspace"), "{stderr}");
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            "1",
            "pnpm {version} reached install before rejecting inactive local patches"
        );
    }
    fs::remove_file(repo.join("apps/web/pnpm-workspace.yaml")).unwrap();
    fs::remove_file(repo.join("apps/web/local.patch")).unwrap();
    run_mode("lint", false);

    for (path, contents) in [
        (
            ".npmrc",
            "shared-workspace-lockfile=false\nregistry=https://registry.example/\n",
        ),
        (
            "pnpm-workspace.yaml",
            "packages:\n  - tools/*\n  - packages/*\n",
        ),
        (
            ".pnpmfile.cjs",
            "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n",
        ),
    ] {
        fs::write(repo.join(path), contents).unwrap();
        run_mode("lint", true);
        let failed_count = fs::read_to_string(&install_count)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        run_mode("lint", false);
        assert_eq!(
            fs::read_to_string(&install_count)
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap(),
            failed_count + 1
        );
    }

    let workflow = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert_eq!(workflow.matches(r#"- ".pnpmfile.cjs""#).count(), 2);
    assert_eq!(workflow.matches(r#"- "pnpmfile.cjs""#).count(), 2);
}

#[cfg(unix)]
#[test]
fn generated_web_checks_recover_interrupted_and_contended_stale_install_locks() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("stale-lock-contention");

    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("stale-lock-contention".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/web"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "{\"lockfileVersion\":3}\n").unwrap();
    fs::write(repo.join(".node-version"), "22.22.2\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_node = fake_bin.join("node");
    fs::write(
        &fake_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  if kill -0 "$3" 2>/dev/null; then printf '%s\n' live; else printf '%s\n' stale; fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-managed-npm" ]; then
  launcher_operation="$3"
  app_dir="$4"
  operation_argument="$5"
  cd "$app_dir"
  case "$launcher_operation:$operation_argument" in
    install:frozen) exec npm ci ;;
    install:bootstrap) exec npm install ;;
    run-script:*) exec npm --prefix=. --workspace=. --workspaces=true --include-workspace-root=true --global=false --location=project --if-present=false --include=dev --include=optional --include=peer run "$operation_argument" ;;
    *) exit 2 ;;
  esac
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-lockfile-kind" ]; then
  lockfile="${3:-}"
  [ -n "$lockfile" ] && [ -f "$lockfile" ] && [ ! -L "$lockfile" ] || exit 1
  if tr -d '\r' < "$lockfile" | grep -Eq '^# yarn lockfile v1$'; then
    printf '%s\n' classic
  else
    printf '%s\n' berry
  fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-authority-preflight" ]; then
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-config" ]; then
  printf '%s\n' 'classic:dGVzdA=='
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-pnp-proof" ]; then
  [ -s "$4" ] || exit 1
  cksum "$4" | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-manifest" ]; then
  manifest="$3"
  if tr '\n' ' ' < "$manifest" | grep -Eq '"installConfig"[[:space:]]*:[[:space:]]*\{[^}]*"pnp"[[:space:]]*:[[:space:]]*true'; then
    printf '%s\n' true
    exit 0
  fi
  exit 1
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-berry-config" ]; then
  linker=pnp
  if [ -f .yarnrc.yml ] && grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*(node-modules|pnpm)' .yarnrc.yml; then
    linker=node-modules
  fi
  config="{\"nodeLinker\":\"$linker\",\"cacheFolder\":\"$(pwd)/.yarn/cache\",\"installStatePath\":\"$(pwd)/.yarn/install-state.gz\",\"pnpUnpluggedFolder\":\"$(pwd)/.yarn/unplugged\",\"pnpEnableInlining\":false,\"pnpEnableEsmLoader\":false}"
  printf '%s' "$config" | base64 | tr -d '\n'
  printf '\n'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-config-value" ]; then
  config="$(printf '%s' "$3" | base64 --decode 2>/dev/null || printf '%s' "$3" | base64 -D)"
  case "$4" in
    nodeLinker) printf '%s\n' "$config" | sed -n 's/.*"nodeLinker":"\([^"]*\)".*/\1/p' ;;
    *) exit 1 ;;
  esac
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-pnp-proof" ]; then
  scope="$3"
  for required in "$4" "$scope/.pnp.data.json" "$scope/.yarn/install-state.gz" "$scope/.yarn/cache/dependency.zip"; do
    [ -s "$required" ] && [ ! -L "$required" ] || exit 1
    cksum "$required"
  done | cksum | awk '{print $1}'
  exit 0
fi

if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-node-modules-proof" ]; then
  root="$3/node_modules"
  entries="$(find "$root" -mindepth 1 \( -type f -o -type l \) ! -name '.jig-web-dependencies-v3' ! -name '.jig-web-dependencies-v3.tmp.*' ! -path "$root/.cache/*" ! -path "$root/.vite/*" ! -path "$root/.tmp/*" ! -name '.DS_Store' -print | LC_ALL=C sort)"
  [ -n "$entries" ] || exit 1
  printf '%s\n' "$entries" | while IFS= read -r entry; do
    relative="${entry#"$root"/}"
    if [ -L "$entry" ]; then
      printf 'link %s %s\n' "$relative" "$(readlink "$entry")"
    else
      printf 'file %s %s\n' "$relative" "$(wc -c < "$entry" | tr -d ' ')"
      [ "${entry##*/}" != "package.json" ] || cksum "$entry"
    fi
  done | cksum | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ]; then
  shift
  for file in "$@"; do
    if [ -f "$file" ]; then cksum "$file"; fi
    if [ -d "$file" ]; then
      find "$file" -type f -print | LC_ALL=C sort | while IFS= read -r nested; do
        printf '%s\n' "$nested"
        cksum "$nested"
      done
    fi
  done | cksum | awk '{print $1}'
fi
exit 0
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).unwrap();

    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    if ! mkdir "$INSTALL_ACTIVE" 2>/dev/null; then
      : > "$INSTALL_OVERLAP"
      exit 11
    fi
    trap 'rmdir "$INSTALL_ACTIVE"' EXIT
    printf '%s\n' "$$" >> "$INSTALL_LOG"
    sleep 0.2
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let recovery_barrier = repo.join("recovery-barrier");
    let recovery_moves = repo.join("recovery-moves");
    let bash_env = repo.join("bash-env");
    let paused_bash_env = repo.join("paused-bash-env");
    let pid_reuse_bash_env = repo.join("pid-reuse-bash-env");
    let claim_ready = repo.join("claim-ready");
    fs::write(&recovery_barrier, "").unwrap();
    fs::write(
        &bash_env,
        r#"kill() {
  if [ "${1:-}" = "-0" ] && [ "${2:-}" = "999999999" ]; then
    printf '%s\n' "$$" >> "$RECOVERY_BARRIER"
    attempt=0
    while [ "$(wc -l < "$RECOVERY_BARRIER")" -lt 2 ]; do
      attempt=$((attempt + 1))
      if [ "$attempt" -ge 1000 ]; then return 1; fi
      sleep 0.01
    done
    return 1
  fi
  builtin kill "$@"
}

mv() {
  if [ "${1:-}" = ".agent/tmp/web-dependencies.lock" ]; then
    printf '%s\n' "$$" >> "$RECOVERY_MOVES"
  fi
  command mv "$@"
}
"#,
    )
    .unwrap();
    fs::write(
        &paused_bash_env,
        r#"ln() {
  command ln "$@"
  status=$?
  if [ "$status" -eq 0 ]; then
    destination=
    for argument in "$@"; do destination="$argument"; done
    case "${PAUSE_AFTER_LINK:-}:$destination" in
      candidate:.agent/tmp/web-dependencies.lock|claim:*.recover.*)
        printf '%s\n' "$$" > "$CLAIM_READY"
        while :; do sleep 1; done
        ;;
    esac
  fi
  return "$status"
}
"#,
    )
    .unwrap();
    fs::write(
        &pid_reuse_bash_env,
        r#"ps() {
  for argument in "$@"; do
    if [ "$argument" = "$REUSED_PID" ]; then
      printf '%s\n' 'Thu Jan  1 00:00:00 2099'
      return 0
    fi
  done
  command ps "$@"
}
"#,
    )
    .unwrap();

    let install_lock = repo.join(".agent/tmp/web-dependencies.lock");
    fs::create_dir_all(install_lock.parent().unwrap()).unwrap();

    let install_active = repo.join("install-active");
    let install_overlap = repo.join("install-overlap");
    let install_log = repo.join("install-log");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = |bash_env: Option<&std::path::Path>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", "bootstrap"])
            .current_dir(&repo)
            .env("NODE", &fake_node)
            .env("PATH", &path)
            .env("RECOVERY_BARRIER", &recovery_barrier)
            .env("RECOVERY_MOVES", &recovery_moves)
            .env("CLAIM_READY", &claim_ready)
            .env("REUSED_PID", std::process::id().to_string())
            .env("INSTALL_ACTIVE", &install_active)
            .env("INSTALL_OVERLAP", &install_overlap)
            .env("INSTALL_LOG", &install_log)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(bash_env) = bash_env {
            command.env("BASH_ENV", bash_env);
        }
        command
    };

    let interrupt_after_link = |kind: &str| {
        if claim_ready.exists() {
            fs::remove_file(&claim_ready).unwrap();
        }
        let mut interrupted = command(Some(&paused_bash_env));
        interrupted.env("PAUSE_AFTER_LINK", kind);
        let mut interrupted = interrupted.spawn().unwrap();
        for _ in 0..500 {
            if claim_ready.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            claim_ready.exists(),
            "interrupted {kind} transition never reached its pause point"
        );
        assert_eq!(
            fs::read_to_string(&claim_ready).unwrap().trim(),
            interrupted.id().to_string()
        );
        interrupted.kill().unwrap();
        let interrupted = interrupted.wait_with_output().unwrap();
        assert!(!interrupted.status.success());
    };
    let assert_no_lock_sidecars = || {
        assert!(
            fs::read_dir(install_lock.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("web-dependencies.lock.")),
            "stale-lock recovery left a sidecar behind"
        );
    };
    let reset_dependency_state = || {
        for directory in [
            repo.join("node_modules"),
            repo.join(".agent/tmp/web-dependencies"),
        ] {
            if directory.exists() {
                fs::remove_dir_all(directory).unwrap();
            }
        }
        for file in [
            &install_log,
            &install_overlap,
            &recovery_moves,
            &claim_ready,
        ] {
            if file.exists() {
                fs::remove_file(file).unwrap();
            }
        }
        fs::write(&recovery_barrier, "").unwrap();
    };

    interrupt_after_link("candidate");
    assert!(install_lock.exists());
    let lock_metadata = fs::read_to_string(&install_lock).unwrap();
    let lock_fields = lock_metadata.split_whitespace().collect::<Vec<_>>();
    assert_eq!(lock_fields.len(), 3);
    assert_ne!(lock_fields[2], "unknown");
    assert!(
        fs::read_dir(install_lock.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("web-dependencies.lock.candidate.")),
        "candidate hardlink was not retained at the simulated kill point"
    );
    let recovered = command(None).output().unwrap();
    assert!(
        recovered.status.success(),
        "bootstrap did not reclaim an interrupted lock creation:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&recovered.stdout),
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(fs::read_to_string(&install_log).unwrap().lines().count(), 1);
    assert!(!install_lock.exists());
    assert_no_lock_sidecars();

    reset_dependency_state();
    fs::write(&install_lock, "999999999\n").unwrap();
    interrupt_after_link("claim");
    assert!(install_lock.exists());
    let recovered = command(None).output().unwrap();
    assert!(
        recovered.status.success(),
        "bootstrap did not reclaim an interrupted recovery claim:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&recovered.stdout),
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(fs::read_to_string(&install_log).unwrap().lines().count(), 1);
    assert!(!install_lock.exists());
    assert_no_lock_sidecars();

    reset_dependency_state();
    fs::write(
        &install_lock,
        format!("{} reused-token DefinitelyOldStart\n", std::process::id()),
    )
    .unwrap();
    let recovered = command(Some(&pid_reuse_bash_env)).output().unwrap();
    assert!(
        recovered.status.success(),
        "bootstrap did not detect a reused lock-owner PID:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&recovered.stdout),
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(fs::read_to_string(&install_log).unwrap().lines().count(), 1);
    assert!(!install_lock.exists());
    assert_no_lock_sidecars();

    reset_dependency_state();
    fs::write(&install_lock, "999999999\n").unwrap();

    let first = command(Some(&bash_env)).spawn().unwrap();
    let second = command(Some(&bash_env)).spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    for (name, output) in [("first", first), ("second", second)] {
        assert!(
            output.status.success(),
            "{name} bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(
        fs::read_to_string(&recovery_moves).unwrap().lines().count(),
        1,
        "more than one process moved the stale lock generation"
    );
    assert_eq!(
        fs::read_to_string(&install_log).unwrap().lines().count(),
        1,
        "dependency installation ran more than once"
    );
    assert!(
        !install_overlap.exists(),
        "dependency installation overlapped under stale-lock contention"
    );
    assert!(!install_lock.exists());
    assert!(repo.join("node_modules").is_dir());
    assert_no_lock_sidecars();
}

#[cfg(unix)]
#[test]
fn generated_web_checks_wait_for_live_installs_and_never_suggest_removing_their_lock() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("live-install-lock");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("live-install-lock".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root-lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    : > "$INSTALL_ACTIVE"
    sleep 0.25
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    rm -f "$INSTALL_ACTIVE"
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let install_count = repo.join("install-count");
    let install_active = repo.join("install-active");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = || {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", "bootstrap"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_ACTIVE", &install_active)
            .env("JIG_WEB_INSTALL_LOCK_UNRESOLVED_ATTEMPTS", "1")
            .env("JIG_WEB_INSTALL_LOCK_POLL_SECONDS", "0.01")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command
    };

    let mut first_command = command();
    first_command.env("TZ", "Europe/Prague");
    let first = first_command.spawn().unwrap();
    for _ in 0..200 {
        if install_active.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(install_active.exists(), "first install never became active");
    let mut second_command = command();
    second_command.env("TZ", "America/Los_Angeles");
    let second = second_command.spawn().unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    for (name, output) in [("first", first), ("second", second)] {
        assert!(
            output.status.success(),
            "{name} live-lock bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    assert!(
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web",])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    fs::remove_dir_all(repo.join("node_modules")).unwrap();
    fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies")).unwrap();
    let install_lock = repo.join(".agent/tmp/web-dependencies.lock");
    fs::write(&install_lock, "malformed lock metadata\n").unwrap();
    let malformed = command().output().unwrap();
    assert!(!malformed.status.success());
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(stderr.contains("could not be validated or recovered"));
    assert!(!stderr.to_ascii_lowercase().contains("remove"));

    fs::write(&install_lock, "999999999 absent-token RecordedStart\n").unwrap();
    let absent = command().output().unwrap();
    assert!(
        absent.status.success(),
        "an ESRCH owner was not recovered:\n{}",
        String::from_utf8_lossy(&absent.stderr)
    );
    assert!(!install_lock.exists());
    fs::remove_dir_all(repo.join("node_modules")).unwrap();
    fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies")).unwrap();

    let kill_failure_env = repo.join("kill-failure-env");
    fs::write(
        &kill_failure_env,
        "kill() { if [ \"${1:-}\" = \"-0\" ]; then return 1; fi; builtin kill \"$@\"; }\n",
    )
    .unwrap();
    let probe_node = repo.join("probe-node");
    fs::write(
        &probe_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  case "${PROBE_BEHAVIOR:-}" in
    eperm) printf '%s\n' unverified; exit 0 ;;
    tool-failure) exit 19 ;;
  esac
fi
exec node "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&probe_node, fs::Permissions::from_mode(0o755)).unwrap();
    let simulated_owner = format!(
        "{} simulated-permission-owner RecordedStart\n",
        std::process::id()
    );
    for behavior in ["eperm", "tool-failure"] {
        fs::write(&install_lock, &simulated_owner).unwrap();
        let mut simulated = command();
        simulated
            .env("BASH_ENV", &kill_failure_env)
            .env("NODE", &probe_node)
            .env("PROBE_BEHAVIOR", behavior);
        let simulated = simulated.output().unwrap();
        assert!(
            !simulated.status.success(),
            "{behavior} process probe was treated as a stale owner"
        );
        assert_eq!(fs::read_to_string(&install_lock).unwrap(), simulated_owner);
    }

    let unknown_live = format!("{} legacy-token unknown\n", std::process::id());
    fs::write(&install_lock, &unknown_live).unwrap();
    let unverified = command().output().unwrap();
    assert!(!unverified.status.success());
    assert_eq!(fs::read_to_string(&install_lock).unwrap(), unknown_live);
    assert!(
        String::from_utf8_lossy(&unverified.stderr).contains("could not be validated or recovered")
    );

    let ps_failure_env = repo.join("ps-failure-env");
    fs::write(&ps_failure_env, "ps() { return 1; }\n").unwrap();
    fs::write(
        &install_lock,
        format!("{} known-token RecordedStart\n", std::process::id()),
    )
    .unwrap();
    let mut unreadable_identity = command();
    unreadable_identity.env("BASH_ENV", &ps_failure_env);
    let unreadable_identity = unreadable_identity.output().unwrap();
    assert!(!unreadable_identity.status.success());
    assert!(install_lock.exists(), "unverified live lock was removed");
}

#[cfg(unix)]
#[test]
fn generated_web_install_worker_survives_parent_sigkill_without_overlapping_install() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("sigkill-install-worker");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("sigkill-install-worker".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let start_worker = checker
        .split_once("start_install_worker() {")
        .unwrap()
        .1
        .split_once("run_dependency_install() {")
        .unwrap()
        .0;
    assert!(
        start_worker
            .find("trap 'forward_install_worker_signal HUP' HUP")
            .unwrap()
            < start_worker.find("\"$bash_bin\" \"$0\"").unwrap(),
        "worker signal forwarding must be armed before the worker is spawned"
    );
    assert!(checker.contains("trap 'preserve_install_lock_for_group_recovery' EXIT"));
    assert!(!checker.contains("trap 'release_install_lock' EXIT\n          break"));
    let handoff_wait = checker
        .split_once("dependency_install_worker() {")
        .unwrap()
        .1
        .split_once("scope=\"$(dependency_scope")
        .unwrap()
        .0;
    assert!(handoff_wait.contains("while :; do"));
    assert!(handoff_wait.contains("unresolved_handoff_attempts=0"));
    assert!(handoff_wait.contains("max_unresolved_handoff_attempts=600"));
    assert!(!handoff_wait.contains("attempt -lt 500"));
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
trap '' TERM
case "${1:-}" in
  ci|install)
    if ! mkdir "$INSTALL_ACTIVE" 2>/dev/null; then
      : > "$INSTALL_OVERLAP"
      exit 11
    fi
    trap 'rmdir "$INSTALL_ACTIVE" 2>/dev/null || true' EXIT
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    : > "$INSTALL_STARTED"
    while [ ! -f "$INSTALL_RELEASE" ]; do sleep 0.01; done
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let install_active = repo.join("install-active");
    let install_overlap = repo.join("install-overlap");
    let install_count = repo.join("install-count");
    let install_started = repo.join("install-started");
    let install_release = repo.join("install-release");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let command = || {
        let mut command = std::process::Command::new("bash");
        command
            .args([
                "scripts/check-webapps.sh",
                "dependencies-install",
                "apps/web",
            ])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_ACTIVE", &install_active)
            .env("INSTALL_OVERLAP", &install_overlap)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_STARTED", &install_started)
            .env("INSTALL_RELEASE", &install_release)
            .env("JIG_WEB_INSTALL_LOCK_POLL_SECONDS", "0.01")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };

    let mut first = command().spawn().unwrap();
    for _ in 0..500 {
        if install_started.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let started = install_started.exists();
    if started {
        unsafe {
            libc::kill(first.id() as i32, libc::SIGKILL);
        }
    }
    let _ = first.wait();
    let mut second = command().spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let overlap_before_release = install_overlap.exists();
    fs::write(&install_release, "release\n").unwrap();

    let mut second_status = None;
    for _ in 0..500 {
        second_status = second.try_wait().unwrap();
        if second_status.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if second_status.is_none() {
        let _ = second.kill();
        let _ = second.wait();
    }

    assert!(started, "installer worker never started");
    assert!(
        !overlap_before_release,
        "second install overlapped the orphaned worker"
    );
    assert!(
        second_status.unwrap().success(),
        "waiting install wrapper failed"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    assert!(!repo.join(".agent/tmp/web-dependencies.lock").exists());
    assert!(
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web",])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    fs::remove_dir_all(repo.join("node_modules")).unwrap();
    fs::remove_dir_all(repo.join(".agent/tmp/web-dependencies")).unwrap();
    for file in [
        &install_overlap,
        &install_count,
        &install_started,
        &install_release,
    ] {
        if file.exists() {
            fs::remove_file(file).unwrap();
        }
    }
    if install_active.exists() {
        fs::remove_dir_all(&install_active).unwrap();
    }

    let mut interrupted = command().spawn().unwrap();
    for _ in 0..500 {
        if install_started.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        install_started.exists(),
        "signal-interrupted installer worker never started"
    );
    assert_eq!(
        unsafe { libc::kill(interrupted.id() as i32, libc::SIGTERM) },
        0,
        "could not signal dependency-install coordinator"
    );
    let mut interrupted_status = None;
    for _ in 0..500 {
        interrupted_status = interrupted.try_wait().unwrap();
        if interrupted_status.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        interrupted_status.is_some_and(|status| !status.success()),
        "signal-interrupted coordinator did not exit through its forwarding path"
    );

    let mut waiting = command().spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        waiting.try_wait().unwrap().is_none(),
        "a waiter stopped honoring the interrupted worker generation"
    );
    assert!(
        !install_overlap.exists(),
        "a second install overlapped a signal-surviving package-manager descendant"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
    fs::write(&install_release, "release\n").unwrap();

    let mut waiting_status = None;
    for _ in 0..500 {
        waiting_status = waiting.try_wait().unwrap();
        if waiting_status.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if waiting_status.is_none() {
        let _ = waiting.kill();
        let _ = waiting.wait();
    }
    assert!(
        waiting_status.unwrap().success(),
        "waiter failed after the interrupted install group exited"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
    assert!(!install_overlap.exists());
    assert!(!repo.join(".agent/tmp/web-dependencies.lock").exists());
}

#[cfg(unix)]
#[test]
fn generated_web_install_worker_preserves_status_after_wait_without_pid_probe() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("install-worker-status");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("install-worker-status".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/web"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    : > "$INSTALL_FINISHED"
    exit 42
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();

    let bash_env = repo.join("simulate-post-wait-pid-reuse");
    fs::write(
        &bash_env,
        r#"kill() {
  if [ "${1:-}" = "-0" ] && [ -f "$INSTALL_FINISHED" ] && [ ! -f "$PID_REUSE_PROBED" ]; then
    : > "$PID_REUSE_PROBED"
    return 0
  fi
  builtin kill "$@"
}
"#,
    )
    .unwrap();
    let install_finished = repo.join("install-finished");
    let pid_reuse_probed = repo.join("pid-reuse-probed");
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let output = std::process::Command::new("/bin/bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-install",
            "apps/web",
        ])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("BASH_ENV", &bash_env)
        .env("INSTALL_FINISHED", &install_finished)
        .env("PID_REUSE_PROBED", &pid_reuse_probed)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(42),
        "worker status was clobbered:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(install_finished.exists());
    assert!(
        !pid_reuse_probed.exists(),
        "coordinator probed a reaped worker PID"
    );
    assert!(!repo.join(".agent/tmp/web-dependencies.lock").exists());
    assert!(
        !repo
            .join(".agent/tmp/web-dependencies/root.sha256")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn generated_web_checks_track_lockfiles_and_yarn_pnp_install_state() {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for (case_name, package_manager, lockfile, artifact, initial_lock, classic_pnp, yarn_config) in [
        (
            "npm",
            "npm",
            "package-lock.json",
            "node_modules",
            "lock-v1\n",
            false,
            None,
        ),
        (
            "npm-shrinkwrap",
            "npm",
            "npm-shrinkwrap.json",
            "node_modules",
            "shrinkwrap-v1\n",
            false,
            None,
        ),
        (
            "yarn-modern",
            "yarn",
            "yarn.lock",
            ".pnp.cjs",
            "__metadata:\n  version: 8\n",
            false,
            None,
        ),
        (
            "yarn-classic",
            "yarn",
            "yarn.lock",
            ".pnp.js",
            "# yarn lockfile v1\n",
            true,
            None,
        ),
        (
            "yarn-node-modules",
            "yarn",
            "yarn.lock",
            "node_modules",
            "__metadata:\n  version: 8\n",
            false,
            Some("nodeLinker: node-modules\n"),
        ),
    ] {
        let repo = temp.path().join(case_name);
        run_init(InitOpts {
            path: repo.clone(),
            scaffold: ScaffoldOpts::default(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some(format!("sentinel-{case_name}")),
                sqlx_enabled: Some(false),
                web_package_manager: Some(package_manager.into()),
                frontend_apps: vec![FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        fs::create_dir_all(repo.join("apps/web")).unwrap();
        let package_json = r#"{"private":true,"workspaces":["apps/web"]}"#;
        fs::write(repo.join("package.json"), package_json).unwrap();
        fs::write(
            repo.join("apps/web/package.json"),
            r#"{"name":"web","scripts":{"lint":"true"}}"#,
        )
        .unwrap();
        fs::write(repo.join(".node-version"), "22.22.2\n").unwrap();
        fs::write(repo.join(lockfile), initial_lock).unwrap();
        if package_manager == "yarn" {
            fs::write(
                repo.join(".yarnrc"),
                if classic_pnp {
                    "--install.pure-lockfile false\n--pnp true\n"
                } else {
                    "--install.pure-lockfile false\n"
                },
            )
            .unwrap();
            if let Some(config) = yarn_config {
                fs::write(repo.join(".yarnrc.yml"), config).unwrap();
            }
            for runtime_file in [
                ".yarn/patches/dependency.patch",
                ".yarn/plugins/plugin.cjs",
                ".yarn/releases/yarn.cjs",
            ] {
                let path = repo.join(runtime_file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "runtime-v1\n").unwrap();
            }
        }

        let fake_bin = repo.join("fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_node = fake_bin.join("node");
        fs::write(
        &fake_node,
        r#"#!/bin/sh
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-process-probe" ]; then
  if kill -0 "$3" 2>/dev/null; then printf '%s\n' live; else printf '%s\n' stale; fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-managed-npm" ]; then
  launcher_operation="$3"
  app_dir="$4"
  operation_argument="$5"
  cd "$app_dir"
  case "$launcher_operation" in
    install)
      case "$operation_argument" in
        frozen) operation=ci ;;
        bootstrap) operation=install ;;
        *) exit 2 ;;
      esac
      unset NODE_ENV NPM_CONFIG_OMIT NPM_CONFIG_INCLUDE NPM_CONFIG_PRODUCTION NPM_CONFIG_OPTIONAL
      unset NPM_CONFIG_ONLY NPM_CONFIG_DEV NPM_CONFIG_ALSO
      unset Npm_Config_Bin_Links npm_CONFIG_dry_run NPM_CONFIG_PACKAGE_LOCK_ONLY
      unset NPM_CONFIG_PACKAGE_LOCK NPM_CONFIG_GLOBAL NPM_CONFIG_WORKSPACE NPM_CONFIG_WORKSPACES
      unset NPM_CONFIG_INCLUDE_WORKSPACE_ROOT NPM_CONFIG_PREFIX NPM_CONFIG_LOCATION NPM_CONFIG_IF_PRESENT
      unset NPM_CONFIG_CPU NPM_CONFIG_OS NPM_CONFIG_LIBC
      set -- npm "$operation" \
        --include=dev --include=optional --include=peer \
        --bin-links=true --dry-run=false --package-lock-only=false \
        --package-lock=true --global=false --location=project \
        "--prefix=$(pwd -P)" --cpu=test-cpu --os=test-platform
      if [ "$app_dir" = "." ]; then
        set -- "$@" --workspaces=true --include-workspace-root=true
      else
        set -- "$@" --workspaces=false
      fi
      exec "$@"
      ;;
    run-script)
      unset NPM_CONFIG_OMIT NPM_CONFIG_INCLUDE NPM_CONFIG_PRODUCTION NPM_CONFIG_OPTIONAL
      unset NPM_CONFIG_ONLY NPM_CONFIG_DEV NPM_CONFIG_ALSO
      unset NPM_CONFIG_GLOBAL NPM_CONFIG_WORKSPACE NPM_CONFIG_WORKSPACES
      unset NPM_CONFIG_INCLUDE_WORKSPACE_ROOT NPM_CONFIG_PREFIX NPM_CONFIG_LOCATION NPM_CONFIG_IF_PRESENT
      exec npm --prefix=. --workspace=. --workspaces=true --include-workspace-root=true \
        --global=false --location=project --if-present=false \
        --include=dev --include=optional --include=peer run "$operation_argument"
      ;;
    *) exit 2 ;;
  esac
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-lockfile-kind" ]; then
  lockfile="${3:-}"
  [ -n "$lockfile" ] && [ -f "$lockfile" ] && [ ! -L "$lockfile" ] || exit 1
  if tr -d '\r' < "$lockfile" | grep -Eq '^# yarn lockfile v1$'; then
    printf '%s\n' classic
  else
    printf '%s\n' berry
  fi
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-authority-preflight" ]; then
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-config" ]; then
  printf '%s\n' 'classic:dGVzdA=='
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-pnp-proof" ]; then
  [ -s "$4" ] || exit 1
  cksum "$4" | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-classic-manifest" ]; then
  manifest="$3"
  if tr '\n' ' ' < "$manifest" | grep -Eq '"installConfig"[[:space:]]*:[[:space:]]*\{[^}]*"pnp"[[:space:]]*:[[:space:]]*true'; then
    printf '%s\n' true
    exit 0
  fi
  exit 1
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-berry-config" ]; then
  linker=pnp
  if [ -f .yarnrc.yml ] && grep -Eq '^[[:space:]]*nodeLinker[[:space:]]*:[[:space:]]*(node-modules|pnpm)' .yarnrc.yml; then
    linker=node-modules
  fi
  config="{\"nodeLinker\":\"$linker\",\"cacheFolder\":\"$(pwd)/.yarn/cache\",\"installStatePath\":\"$(pwd)/.yarn/install-state.gz\",\"pnpUnpluggedFolder\":\"$(pwd)/.yarn/unplugged\",\"pnpEnableInlining\":false,\"pnpEnableEsmLoader\":false}"
  printf '%s' "$config" | base64 | tr -d '\n'
  printf '\n'
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-config-value" ]; then
  config="$(printf '%s' "$3" | base64 --decode 2>/dev/null || printf '%s' "$3" | base64 -D)"
  case "$4" in
    nodeLinker) printf '%s\n' "$config" | sed -n 's/.*"nodeLinker":"\([^"]*\)".*/\1/p' ;;
    *) exit 1 ;;
  esac
  exit 0
fi
if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-yarn-pnp-proof" ]; then
  scope="$3"
  for required in "$4" "$scope/.pnp.data.json" "$scope/.yarn/install-state.gz" "$scope/.yarn/cache/dependency.zip"; do
    [ -s "$required" ] && [ ! -L "$required" ] || exit 1
    cksum "$required"
  done | cksum | awk '{print $1}'
  exit 0
fi

if [ "${1:-}" = "-" ] && [ "${2:-}" = "--jig-node-modules-proof" ]; then
  root="$3/node_modules"
  entries="$(find "$root" -mindepth 1 \( -type f -o -type l \) ! -name '.jig-web-dependencies-v3' ! -name '.jig-web-dependencies-v3.tmp.*' ! -path "$root/.cache/*" ! -path "$root/.vite/*" ! -path "$root/.tmp/*" ! -name '.DS_Store' -print | LC_ALL=C sort)"
  [ -n "$entries" ] || exit 1
  printf '%s\n' "$entries" | while IFS= read -r entry; do
    relative="${entry#"$root"/}"
    if [ -L "$entry" ]; then
      printf 'link %s %s\n' "$relative" "$(readlink "$entry")"
    else
      printf 'file %s %s\n' "$relative" "$(wc -c < "$entry" | tr -d ' ')"
      [ "${entry##*/}" != "package.json" ] || cksum "$entry"
    fi
  done | cksum | awk '{print $1}'
  exit 0
fi
if [ "${1:-}" = "-" ]; then
  shift
  for file in "$@"; do
    if [ -f "$file" ]; then cksum "$file"; fi
    if [ -d "$file" ]; then
      find "$file" -type f -print | LC_ALL=C sort | while IFS= read -r nested; do
        printf '%s\n' "$nested"
        cksum "$nested"
      done
    fi
  done | cksum | awk '{print $1}'
fi
exit 0
"#,
        )
        .unwrap();
        fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).unwrap();

        let fake_manager = fake_bin.join(package_manager);
        fs::write(
            &fake_manager,
            r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    case "$(basename "$0")" in
      pnpm) printf '%s\n' '10.12.1' ;;
      yarn) printf '%s\n' '4.17.1' ;;
      *) exit 2 ;;
    esac
    ;;
  config)
    [ "$(basename "$0")" = pnpm ] && [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
    printf '%s\n' '{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false}'
    ;;
  pkg)
    [ "$(basename "$0")" = pnpm ] && [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 2
    printf '%s\n' '{}'
    ;;
  ci|install)
    printf '%s\n' "$@" > "$INSTALL_ARGV"
    if [ "$(basename "$0")" = npm ]; then env | LC_ALL=C sort > "$INSTALL_ENV"; fi
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    count=$((count + 1))
    printf '%s\n' "$count" > "$INSTALL_COUNT"
    if [ "${FAIL_INSTALL:-0}" = "1" ]; then exit 9; fi
    if [ ! -f "$TEST_LOCKFILE" ] && { [ "$1" != ci ] || [ "$(basename "$0")" != npm ]; }; then
      printf '%s\n' "lock-v1" > "$TEST_LOCKFILE"
    fi
    if [ "$TEST_ARTIFACT" = "node_modules" ]; then
      mkdir -p node_modules/test-package
      printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    elif [ "$TEST_PACKAGE_MANAGER" = "yarn" ]; then
      if [ "$TEST_ARTIFACT" = ".pnp.cjs" ]; then
        printf '%s\n' 'generated pnp loader using .pnp.data.json' > "$TEST_ARTIFACT"
        printf '%s\n' '{"dependencyTreeRoots":[]}' > .pnp.data.json
        mkdir -p .yarn/cache
        printf '%s\n' archive > .yarn/cache/dependency.zip
        printf '%s\n' state > .yarn/install-state.gz
      else
        printf '%s\n' 'generated pnp loader' > "$TEST_ARTIFACT"
      fi
    else
      exit 3
    fi
    ;;
  --prefix=.)
    [ "$#" -eq 12 ]
    [ "$2" = --workspace=. ]
    [ "$3" = --workspaces=true ]
    [ "$4" = --include-workspace-root=true ]
    [ "$5" = --global=false ]
    [ "$6" = --location=project ]
    [ "$7" = --if-present=false ]
    [ "$8" = --include=dev ]
    [ "$9" = --include=optional ]
    [ "${10}" = --include=peer ]
    [ "${11}" = run ]
    [ "${12}" = lint ]
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();

        let install_count = repo.join("install-count");
        let install_argv = repo.join("install-argv");
        let install_env = repo.join("install-env");
        let mut path = OsString::from(fake_bin.as_os_str());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let run_mode = |mode: &str, fail_install: bool| {
            let output = std::process::Command::new("bash")
                .args(["scripts/check-webapps.sh", mode])
                .current_dir(&repo)
                .env("NODE", &fake_node)
                .env("PATH", &path)
                .env("INSTALL_COUNT", &install_count)
                .env("INSTALL_ARGV", &install_argv)
                .env("INSTALL_ENV", &install_env)
                .env("TEST_LOCKFILE", lockfile)
                .env("TEST_PACKAGE_MANAGER", package_manager)
                .env("TEST_ARTIFACT", artifact)
                .env("FAIL_INSTALL", if fail_install { "1" } else { "0" })
                .env("NODE_ENV", "production")
                .env("NPM_CONFIG_OMIT", "dev optional peer")
                .env("NPM_CONFIG_INCLUDE", "prod")
                .env("NPM_CONFIG_PRODUCTION", "true")
                .env("NPM_CONFIG_OPTIONAL", "false")
                .env("NPM_CONFIG_ONLY", "production")
                .env("NPM_CONFIG_DEV", "false")
                .env("NPM_CONFIG_ALSO", "production")
                .env("Npm_Config_Bin_Links", "false")
                .env("npm_CONFIG_dry_run", "true")
                .env("NPM_CONFIG_PACKAGE_LOCK_ONLY", "true")
                .env("NPM_CONFIG_PACKAGE_LOCK", "false")
                .env("NPM_CONFIG_GLOBAL", "true")
                .env("NPM_CONFIG_WORKSPACE", "other")
                .env("NPM_CONFIG_WORKSPACES", "false")
                .env("NPM_CONFIG_INCLUDE_WORKSPACE_ROOT", "false")
                .env("NPM_CONFIG_PREFIX", "/hostile-prefix")
                .env("NPM_CONFIG_LOCATION", "global")
                .env("NPM_CONFIG_IF_PRESENT", "true")
                .env("NPM_CONFIG_CPU", "hostile-cpu")
                .env("NPM_CONFIG_OS", "hostile-os")
                .env("NPM_CONFIG_LIBC", "hostile-libc")
                .env("NPM_CONFIG_REGISTRY", "https://registry.example.invalid/")
                .env("npm_config_install_strategy", "nested")
                .env("NPM_CONFIG_LEGACY_PEER_DEPS", "true")
                .env("NPM_CONFIG_IGNORE_SCRIPTS", "true")
                .output()
                .unwrap();
            assert_eq!(
                output.status.success(),
                !fail_install,
                "{package_manager} web {mode} failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };
        let dependencies_ready = || {
            std::process::Command::new("bash")
                .args(["scripts/check-webapps.sh", "dependencies-ready", "apps/web"])
                .current_dir(&repo)
                .env("NODE", &fake_node)
                .env("PATH", &path)
                .status()
                .unwrap()
                .success()
        };

        let install_lock = repo.join(".agent/tmp/web-dependencies.lock");
        fs::create_dir_all(install_lock.parent().unwrap()).unwrap();
        fs::write(&install_lock, "999999999\n").unwrap();
        run_mode("bootstrap", false);
        assert!(
            !install_lock.exists(),
            "stale dependency lock was not recovered"
        );
        assert_eq!(
            fs::read_to_string(repo.join(lockfile)).unwrap(),
            initial_lock
        );
        assert!(
            dependencies_ready(),
            "{package_manager} dependency receipt was not reusable immediately after publication"
        );
        let dependency_stamp = repo.join(".agent/tmp/web-dependencies/root.sha256");
        let current_stamp = fs::read_to_string(&dependency_stamp).unwrap();
        assert!(
            current_stamp.starts_with("v5 "),
            "{package_manager} did not publish a v5 dependency receipt"
        );
        fs::write(&dependency_stamp, current_stamp.replacen("v5 ", "v4 ", 1)).unwrap();
        assert!(
            !dependencies_ready(),
            "{package_manager} accepted a stale v4 dependency receipt"
        );
        fs::write(&dependency_stamp, current_stamp).unwrap();
        assert!(dependencies_ready());
        run_mode("lint", false);
        assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");
        assert!(dependencies_ready());

        let mut expected_install_count = 1;
        if package_manager == "npm" {
            let npm_install_argv = format!(
                "{{operation}}\n--include=dev\n--include=optional\n--include=peer\n--bin-links=true\n--dry-run=false\n--package-lock-only=false\n--package-lock=true\n--global=false\n--location=project\n--prefix={}\n--cpu=test-cpu\n--os=test-platform\n--workspaces=true\n--include-workspace-root=true\n",
                repo.canonicalize().unwrap().display()
            );
            assert_eq!(
                fs::read_to_string(&install_argv).unwrap(),
                npm_install_argv.replace("{operation}", "install"),
                "npm bootstrap did not freeze install-shaping inputs"
            );
            let environment = fs::read_to_string(&install_env).unwrap();
            for removed in [
                "NODE_ENV=",
                "NPM_CONFIG_OMIT=",
                "NPM_CONFIG_INCLUDE=",
                "NPM_CONFIG_PRODUCTION=",
                "NPM_CONFIG_OPTIONAL=",
                "NPM_CONFIG_ONLY=",
                "NPM_CONFIG_DEV=",
                "NPM_CONFIG_ALSO=",
                "Npm_Config_Bin_Links=",
                "npm_CONFIG_dry_run=",
                "NPM_CONFIG_PACKAGE_LOCK_ONLY=",
                "NPM_CONFIG_PACKAGE_LOCK=",
                "NPM_CONFIG_GLOBAL=",
                "NPM_CONFIG_WORKSPACE=",
                "NPM_CONFIG_WORKSPACES=",
                "NPM_CONFIG_INCLUDE_WORKSPACE_ROOT=",
                "NPM_CONFIG_PREFIX=",
                "NPM_CONFIG_LOCATION=",
                "NPM_CONFIG_IF_PRESENT=",
                "NPM_CONFIG_CPU=",
                "NPM_CONFIG_OS=",
                "NPM_CONFIG_LIBC=",
            ] {
                assert!(
                    !environment.lines().any(|line| line.starts_with(removed)),
                    "npm install inherited shaping input {removed}:\n{environment}"
                );
            }
            for preserved in [
                "NPM_CONFIG_REGISTRY=https://registry.example.invalid/",
                "npm_config_install_strategy=nested",
                "NPM_CONFIG_LEGACY_PEER_DEPS=true",
                "NPM_CONFIG_IGNORE_SCRIPTS=true",
            ] {
                assert!(
                    environment.lines().any(|line| line == preserved),
                    "npm install removed supported input {preserved}:\n{environment}"
                );
            }
            fs::write(
                repo.join("package.json"),
                r#"{"private":true,"version":"2","workspaces":["apps/web"]}"#,
            )
            .unwrap();
            run_mode("lint", false);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_argv).unwrap(),
                npm_install_argv.replace("{operation}", "ci"),
                "npm frozen install did not freeze install-shaping inputs"
            );
            assert!(dependencies_ready());
        }
        if package_manager == "yarn" {
            let opposite = if artifact == "node_modules" {
                repo.join(".pnp.cjs")
            } else {
                repo.join("node_modules")
            };
            if artifact == "node_modules" {
                fs::write(&opposite, "unexpected pnp loader\n").unwrap();
            } else {
                fs::create_dir_all(&opposite).unwrap();
            }
            assert!(
                !dependencies_ready(),
                "Yarn accepted artifacts for two effective linkers in {case_name}"
            );
            if opposite.is_dir() {
                fs::remove_dir_all(&opposite).unwrap();
            } else {
                fs::remove_file(&opposite).unwrap();
            }
            assert!(dependencies_ready());
            if artifact == ".pnp.cjs" {
                fs::write(
                    repo.join(".yarn/cache/unrelated.zip"),
                    "unrelated archive\n",
                )
                .unwrap();
                assert!(
                    dependencies_ready(),
                    "an unrelated shared-cache addition invalidated Yarn Berry readiness"
                );
                for companion in [
                    ".pnp.data.json",
                    ".yarn/cache/dependency.zip",
                    ".yarn/install-state.gz",
                ] {
                    fs::write(repo.join(companion), "changed companion\n").unwrap();
                    assert!(
                        !dependencies_ready(),
                        "Yarn Berry ignored changed PnP companion {companion}"
                    );
                    run_mode("lint", false);
                    expected_install_count += 1;
                    assert_eq!(
                        fs::read_to_string(&install_count).unwrap().trim(),
                        expected_install_count.to_string()
                    );
                }
            }
            if classic_pnp {
                for flag in ["--install.pnp true", "--enable-pnp true"] {
                    fs::write(
                        repo.join(".yarnrc"),
                        format!("--install.pure-lockfile false\n{flag}\n"),
                    )
                    .unwrap();
                    assert!(!dependencies_ready());
                    run_mode("lint", false);
                    expected_install_count += 1;
                    assert!(dependencies_ready(), "Yarn Classic ignored {flag}");
                }
            }
        }

        if lockfile == "npm-shrinkwrap.json" {
            fs::write(repo.join("package-lock.json"), "inactive-lock-v1\n").unwrap();
            assert!(
                dependencies_ready(),
                "adding an inactive package-lock invalidated the shrinkwrap receipt"
            );
            fs::write(repo.join("package-lock.json"), "inactive-lock-v2\n").unwrap();
            assert!(
                dependencies_ready(),
                "changing an inactive package-lock invalidated the shrinkwrap receipt"
            );
            fs::remove_file(repo.join("npm-shrinkwrap.json")).unwrap();
            assert!(
                !dependencies_ready(),
                "removing the authoritative shrinkwrap reused its receipt for package-lock"
            );
            run_mode("lint", false);
            expected_install_count += 1;
            assert!(dependencies_ready());
        }

        if package_manager == "yarn" {
            for runtime_file in [
                ".yarn/patches/dependency.patch",
                ".yarn/plugins/plugin.cjs",
                ".yarn/releases/yarn.cjs",
            ] {
                fs::write(repo.join(runtime_file), "runtime-v2\n").unwrap();
                run_mode("lint", true);
                expected_install_count += 1;
                assert_eq!(
                    fs::read_to_string(&install_count).unwrap().trim(),
                    expected_install_count.to_string()
                );
                run_mode("lint", false);
                expected_install_count += 1;
                assert_eq!(
                    fs::read_to_string(&install_count).unwrap().trim(),
                    expected_install_count.to_string()
                );
            }
            for config_file in [".yarnrc", ".yarnrc.yml"] {
                let config = match (classic_pnp, artifact, config_file) {
                    (true, _, ".yarnrc") => "--pnp true\n--install.pure-lockfile false\n",
                    (_, "node_modules", ".yarnrc.yml") => {
                        "nodeLinker: node-modules\nchecksumBehavior: reset\n"
                    }
                    _ => "config-v2\n",
                };
                fs::write(repo.join(config_file), config).unwrap();
                run_mode("lint", true);
                expected_install_count += 1;
                assert_eq!(
                    fs::read_to_string(&install_count).unwrap().trim(),
                    expected_install_count.to_string()
                );
                run_mode("lint", false);
                expected_install_count += 1;
                assert_eq!(
                    fs::read_to_string(&install_count).unwrap().trim(),
                    expected_install_count.to_string()
                );
            }
        }

        fs::write(repo.join(".node-version"), "22.22.3\n").unwrap();
        run_mode("lint", true);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );

        fs::write(repo.join(lockfile), "lock-v2\n").unwrap();
        run_mode("lint", true);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );

        let artifact_path = repo.join(artifact);
        if artifact_path.is_dir() {
            fs::remove_dir_all(&artifact_path).unwrap();
            fs::create_dir_all(&artifact_path).unwrap();
        } else {
            fs::write(&artifact_path, "").unwrap();
        }
        assert!(
            !dependencies_ready(),
            "replacement or truncation of {artifact} was accepted"
        );
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );

        if artifact != "node_modules" {
            fs::write(&artifact_path, "different nonempty loader\n").unwrap();
            assert!(
                !dependencies_ready(),
                "a changed nonempty PnP loader was not detected"
            );
            run_mode("lint", false);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_count).unwrap().trim(),
                expected_install_count.to_string()
            );
        }

        let symlink_target = repo.join(format!("{case_name}-replacement-artifact"));
        if artifact == "node_modules" {
            fs::remove_dir_all(&artifact_path).unwrap();
            fs::create_dir_all(&symlink_target).unwrap();
        } else {
            fs::remove_file(&artifact_path).unwrap();
            fs::write(&symlink_target, "linked loader\n").unwrap();
        }
        symlink(&symlink_target, &artifact_path).unwrap();
        assert!(
            !dependencies_ready(),
            "a symlinked dependency artifact was accepted"
        );
        fs::remove_file(&artifact_path).unwrap();
        if symlink_target.is_dir() {
            fs::remove_dir_all(&symlink_target).unwrap();
        } else {
            fs::remove_file(&symlink_target).unwrap();
        }
        run_mode("lint", false);
        expected_install_count += 1;
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            expected_install_count.to_string()
        );

        if artifact == "node_modules" {
            let installed_manifest = artifact_path.join("test-package/package.json");
            fs::write(&installed_manifest, "").unwrap();
            assert!(
                !dependencies_ready(),
                "corrupt installed entries were accepted with an unchanged receipt"
            );
            run_mode("lint", false);
            expected_install_count += 1;

            let receipt_path = artifact_path.join(".jig-web-dependencies-v3");
            let copied_receipt = fs::read_to_string(&receipt_path).unwrap();
            fs::remove_dir_all(&artifact_path).unwrap();
            fs::create_dir_all(&artifact_path).unwrap();
            fs::write(
                artifact_path.join(".jig-web-dependencies-v3"),
                copied_receipt,
            )
            .unwrap();
            assert!(
                !dependencies_ready(),
                "an empty node_modules tree with a copied receipt was accepted"
            );
            run_mode("lint", false);
            expected_install_count += 1;
            assert_eq!(
                fs::read_to_string(&install_count).unwrap().trim(),
                expected_install_count.to_string()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn generated_web_dependency_scope_requires_workspace_membership_and_honors_app_locks() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for (case_name, package_manager, lockfile) in [
        ("bun", "bun", "bun.lock"),
        ("npm-package-lock", "npm", "package-lock.json"),
        ("npm-shrinkwrap", "npm", "npm-shrinkwrap.json"),
        ("pnpm", "pnpm", "pnpm-lock.yaml"),
        ("yarn", "yarn", "yarn.lock"),
    ] {
        for workspace_member in [false, true] {
            let case = if workspace_member {
                "workspace"
            } else {
                "standalone"
            };
            let repo = temp.path().join(format!("{case_name}-{case}"));
            run_init(InitOpts {
                path: repo.clone(),
                scaffold: ScaffoldOpts::default(),
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force: false,
                defaults: true,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some(format!("scope-{case_name}-{case}")),
                    sqlx_enabled: Some(false),
                    web_package_manager: Some(package_manager.into()),
                    frontend_apps: vec![FrontendApp {
                        name: "web".into(),
                        dir: "apps/web".into(),
                        coverage_threshold: 80,
                        kind: "vite".into(),
                        role: "spa".into(),
                    }],
                    ..AnswerOpts::default()
                },
            })
            .unwrap();

            fs::create_dir_all(repo.join("apps/web")).unwrap();
            fs::write(
                repo.join("apps/web/package.json"),
                r#"{"name":"web","scripts":{"lint":"true"}}"#,
            )
            .unwrap();
            if workspace_member {
                fs::write(
                    repo.join("package.json"),
                    r#"{"private":true,"workspaces":["apps/*"]}"#,
                )
                .unwrap();
            } else {
                fs::write(
                    repo.join("package.json"),
                    r#"{"private":true,"workspaces":["tools/*","!apps/web"]}"#,
                )
                .unwrap();
                fs::write(
                    repo.join(lockfile),
                    if package_manager == "yarn" {
                        "__metadata:\n  version: 8\n"
                    } else {
                        "unrelated-root-lock\n"
                    },
                )
                .unwrap();
                fs::create_dir_all(repo.join("node_modules")).unwrap();
            }
            if package_manager == "pnpm" {
                fs::write(
                    repo.join("pnpm-workspace.yaml"),
                    if workspace_member {
                        "packages:\n  - 'apps/**'\n"
                    } else {
                        "packages: ['apps/**', '!apps/web']\n"
                    },
                )
                .unwrap();
            }
            if package_manager == "yarn" {
                fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
            }

            let fake_bin = repo.join("fake-bin");
            fs::create_dir_all(&fake_bin).unwrap();
            let fake_manager = fake_bin.join(package_manager);
            fs::write(
                &fake_manager,
                r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    case "$(basename "$0")" in
      pnpm) printf '%s\n' '10.12.1' ;;
      yarn) printf '%s\n' '4.17.1' ;;
      *) exit 2 ;;
    esac
    ;;
  install|ci)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    pwd > "$INSTALL_CWD"
    if [ "$(basename "$0")" = yarn ]; then
      [ -f "$LOCK_NAME" ] || printf '%s\n' '__metadata:' '  version: 8' > "$LOCK_NAME"
    else
      printf '%s\n' 'installed-lock' > "$LOCK_NAME"
    fi
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  config)
    if [ "$(basename "$0")" = pnpm ]; then
      [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
      printf '%s\n' '{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false}'
      exit 0
    fi
    [ "$(basename "$0")" = yarn ] && [ "${2:-}" = --json ] || exit 2
    scope="$(pwd -P)"
    printf '%s\n' '{"key":"nodeLinker","effective":"node-modules"}'
    printf '{"key":"cacheFolder","effective":"%s/.yarn/cache"}\n' "$scope"
    printf '{"key":"installStatePath","effective":"%s/.yarn/install-state.gz"}\n' "$scope"
    printf '{"key":"pnpUnpluggedFolder","effective":"%s/.yarn/unplugged"}\n' "$scope"
    printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
    printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":false}'
    ;;
  pkg)
    [ "$(basename "$0")" = pnpm ] && [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 2
    printf '%s\n' '{}'
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
            )
            .unwrap();
            fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();

            let install_count = repo.join("install-count");
            let install_cwd = repo.join("install-cwd");
            let mut path = OsString::from(fake_bin.as_os_str());
            path.push(":");
            path.push(std::env::var_os("PATH").unwrap_or_default());
            let run = |mode: &str| {
                std::process::Command::new("/bin/bash")
                    .args(["scripts/check-webapps.sh", mode, "apps/web"])
                    .current_dir(&repo)
                    .env("PATH", &path)
                    .env("INSTALL_COUNT", &install_count)
                    .env("INSTALL_CWD", &install_cwd)
                    .env("LOCK_NAME", lockfile)
                    .output()
                    .unwrap()
            };

            let before = run("dependencies-ready");
            assert!(
                !before.status.success(),
                "artifact-only {package_manager} {case} state was accepted"
            );
            let bootstrap = std::process::Command::new("/bin/bash")
                .args(["scripts/check-webapps.sh", "bootstrap"])
                .current_dir(&repo)
                .env("PATH", &path)
                .env("INSTALL_COUNT", &install_count)
                .env("INSTALL_CWD", &install_cwd)
                .env("LOCK_NAME", lockfile)
                .output()
                .unwrap();
            assert!(
                bootstrap.status.success(),
                "{package_manager} {case} bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&bootstrap.stdout),
                String::from_utf8_lossy(&bootstrap.stderr)
            );
            let expected_cwd = if workspace_member {
                fs::canonicalize(&repo).unwrap()
            } else {
                fs::canonicalize(repo.join("apps/web")).unwrap()
            };
            assert_eq!(
                fs::read_to_string(&install_cwd).unwrap().trim(),
                expected_cwd.display().to_string(),
                "wrong {package_manager} {case} install scope"
            );
            assert!(run("dependencies-ready").status.success());
            assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

            let installed_node_modules = if workspace_member {
                repo.join("node_modules")
            } else {
                repo.join("apps/web/node_modules")
            };
            let dependency_marker = installed_node_modules.join(".jig-web-dependencies-v3");
            let dependency_stamp = if workspace_member {
                repo.join(".agent/tmp/web-dependencies/root.sha256")
            } else {
                repo.join(".agent/tmp/web-dependencies/apps/apps/web.sha256")
            };
            let marker_before_runtime_caches = fs::read(&dependency_marker).unwrap();
            let stamp_before_runtime_caches = fs::read(&dependency_stamp).unwrap();
            assert!(marker_before_runtime_caches.starts_with(b"v2 "));
            assert!(stamp_before_runtime_caches.starts_with(b"v5 "));

            let runtime_node_modules = repo.join("apps/web/node_modules");
            if workspace_member {
                assert!(
                    !runtime_node_modules.exists(),
                    "fake {package_manager} root install unexpectedly populated its workspace member"
                );
            }
            for cache_name in [".cache", ".vite", ".vite-temp", ".tmp"] {
                let cache = runtime_node_modules.join(cache_name);
                fs::create_dir_all(cache.join("nested")).unwrap();
                fs::write(
                    cache.join("nested/runtime-state"),
                    "generated runtime state\n",
                )
                .unwrap();
            }
            fs::write(runtime_node_modules.join(".DS_Store"), "finder state\n").unwrap();
            let cached_ready = run("dependencies-ready");
            assert!(
                cached_ready.status.success(),
                "top-level runtime caches invalidated {package_manager} {case} readiness:\n{}",
                String::from_utf8_lossy(&cached_ready.stderr)
            );
            assert_eq!(
                fs::read(&dependency_marker).unwrap(),
                marker_before_runtime_caches,
                "runtime caches rewrote the {package_manager} {case} dependency marker"
            );
            assert_eq!(
                fs::read(&dependency_stamp).unwrap(),
                stamp_before_runtime_caches,
                "runtime caches rewrote the {package_manager} {case} dependency stamp"
            );

            if workspace_member {
                for cache_name in [".cache", ".vite", ".vite-temp", ".tmp"] {
                    fs::remove_dir_all(runtime_node_modules.join(cache_name)).unwrap();
                }
                fs::remove_file(runtime_node_modules.join(".DS_Store")).unwrap();
                assert!(
                    run("dependencies-ready").status.success(),
                    "an empty runtime-created member node_modules invalidated {package_manager} readiness"
                );

                if case_name == "npm-package-lock" {
                    let unknown_empty = runtime_node_modules.join("unknown-empty-directory");
                    fs::create_dir(&unknown_empty).unwrap();
                    assert!(
                        !run("dependencies-ready").status.success(),
                        "an unknown empty workspace-member directory was normalized away"
                    );
                    fs::remove_dir(&unknown_empty).unwrap();
                    assert!(run("dependencies-ready").status.success());
                }
            }

            if !workspace_member && lockfile == "npm-shrinkwrap.json" {
                let inactive_lock = repo.join("apps/web/package-lock.json");
                fs::write(&inactive_lock, "inactive-app-lock-v1\n").unwrap();
                assert!(
                    run("dependencies-ready").status.success(),
                    "an app package-lock replaced its authoritative npm shrinkwrap"
                );
                fs::write(&inactive_lock, "inactive-app-lock-v2\n").unwrap();
                assert!(
                    run("dependencies-ready").status.success(),
                    "app receipt fingerprint included inactive package-lock content"
                );
            }

            fs::write(
                repo.join("apps/web/package.json"),
                r#"{"name":"web","version":"2","scripts":{"lint":"true"}}"#,
            )
            .unwrap();
            assert!(
                !run("dependencies-ready").status.success(),
                "stale {package_manager} {case} stamp was accepted"
            );
            let install = run("dependencies-install");
            assert!(
                install.status.success(),
                "{package_manager} {case} frozen install failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&install.stdout),
                String::from_utf8_lossy(&install.stderr)
            );
            assert!(run("dependencies-ready").status.success());
            assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

            if workspace_member {
                let app_lock = repo.join("apps/web").join(lockfile);
                if package_manager == "yarn" {
                    fs::write(&app_lock, "# yarn lockfile v1\n").unwrap();
                    assert!(
                        run("dependencies-ready").status.success(),
                        "a Yarn Classic member lock incorrectly replaced its root workspace"
                    );

                    fs::write(&app_lock, "__metadata:\n  version: 8\n").unwrap();
                    assert!(
                        !run("dependencies-ready").status.success(),
                        "a nested Yarn Berry project reused root artifacts"
                    );
                    let app_bootstrap = std::process::Command::new("/bin/bash")
                        .args(["scripts/check-webapps.sh", "bootstrap"])
                        .current_dir(&repo)
                        .env("PATH", &path)
                        .env("INSTALL_COUNT", &install_count)
                        .env("INSTALL_CWD", &install_cwd)
                        .env("LOCK_NAME", lockfile)
                        .output()
                        .unwrap();
                    assert!(
                        app_bootstrap.status.success(),
                        "Yarn Berry nested-project bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&app_bootstrap.stdout),
                        String::from_utf8_lossy(&app_bootstrap.stderr)
                    );
                    assert_eq!(
                        fs::read_to_string(&install_cwd).unwrap().trim(),
                        fs::canonicalize(repo.join("apps/web"))
                            .unwrap()
                            .display()
                            .to_string()
                    );
                    assert!(run("dependencies-ready").status.success());
                    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "3");
                } else {
                    fs::write(&app_lock, "ignored-member-lock\n").unwrap();
                    assert!(
                        run("dependencies-ready").status.success(),
                        "{package_manager} let a nested member lock replace the root workspace"
                    );
                    let bootstrap = run("bootstrap");
                    assert!(bootstrap.status.success());
                    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
                    assert_eq!(
                        fs::read_to_string(&install_cwd).unwrap().trim(),
                        fs::canonicalize(&repo).unwrap().display().to_string()
                    );
                }
            }
        }
    }
}

#[cfg(unix)]
fn init_pnpm_dependency_checker_fixture(
    repo: &Path,
    template: &Path,
) -> (std::ffi::OsString, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    run_init(InitOpts {
        path: repo.to_path_buf(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("pnpm-checker-fixture".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("pnpm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_pnpm = fake_bin.join("pnpm");
    fs::write(
        &fake_pnpm,
        r#"#!/bin/sh
set -eu
if [ -n "${PNPM_QUERY_MARKER:-}" ]; then printf '%s\n' "${1:-}" >> "$PNPM_QUERY_MARKER"; fi
case "${1:-}" in
  --version)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    if [ -n "${PNPM_VERSION_FILE:-}" ] && [ -f "$PNPM_VERSION_FILE" ]; then
      cat "$PNPM_VERSION_FILE"
    else
      printf '%s\n' "${PNPM_VERSION:-10.12.1}"
    fi
    ;;
  config)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
    if [ -n "${PNPM_CONFIG_JSON:-}" ]; then printf '%s\n' "$PNPM_CONFIG_JSON"; exit 0; fi
    shared="${PNPM_SHARED_WORKSPACE_LOCKFILE:-true}"
    global="${PNPM_ENABLE_GLOBAL_VIRTUAL_STORE:-false}"
    case "$shared:$global" in
      true:true|true:false|false:true|false:false)
        printf '{"sharedWorkspaceLockfile":%s,"enableGlobalVirtualStore":%s}\n' "$shared" "$global"
        ;;
      true:undefined|false:undefined)
        printf '{"sharedWorkspaceLockfile":%s}\n' "$shared"
        ;;
      undefined:true|undefined:false)
        printf '{"enableGlobalVirtualStore":%s}\n' "$global"
        ;;
      undefined:undefined) printf '%s\n' '{}' ;;
      *) exit 2 ;;
    esac
    ;;
  pkg)
    [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 8
    if [ -n "${PNPM_PKG_JSON:-}" ]; then printf '%s\n' "$PNPM_PKG_JSON"; else printf '%s\n' '{}'; fi
    ;;
  install)
    count=0
    if [ -f "$INSTALL_COUNT" ]; then count="$(cat "$INSTALL_COUNT")"; fi
    printf '%s\n' "$((count + 1))" > "$INSTALL_COUNT"
    pwd -P > "$INSTALL_CWD"
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    if [ -n "${PNPM_DRIFT_TO:-}" ] && [ -n "${PNPM_VERSION_FILE:-}" ]; then
      printf '%s\n' "$PNPM_DRIFT_TO" > "$PNPM_VERSION_FILE"
    fi
    if [ "${PNPM_DRIFT_LOCK:-0}" = 1 ]; then
      printf '%s\n' drift >> pnpm-lock.yaml
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_pnpm, fs::Permissions::from_mode(0o755)).unwrap();

    let fake_corepack = fake_bin.join("corepack");
    fs::write(
        &fake_corepack,
        r#"#!/bin/sh
set -eu
[ "${1:-}" = pnpm@11.13.0 ] || exit 21
[ "${COREPACK_ENABLE_PROJECT_SPEC:-}" = 0 ] || exit 22
[ "${COREPACK_ENABLE_AUTO_PIN:-}" = 0 ] || exit 23
[ "${COREPACK_ENABLE_DOWNLOAD_PROMPT:-}" = 0 ] || exit 24
[ "${COREPACK_ENV_FILE:-}" = 0 ] || exit 25
[ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] || exit 26
[ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] || exit 27
[ "${NPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS:-}" = false ] || exit 28
[ "${PNPM_CONFIG_MANAGE_PACKAGE_MANAGER_VERSIONS:-}" = false ] || exit 29
[ "${NPM_CONFIG_PM_ON_FAIL:-}" = ignore ] || exit 30
[ "${PNPM_CONFIG_PM_ON_FAIL:-}" = ignore ] || exit 31
[ "${NPM_CONFIG_RUNTIME_ON_FAIL:-}" = ignore ] || exit 32
[ "${PNPM_CONFIG_RUNTIME_ON_FAIL:-}" = ignore ] || exit 33
if [ -n "${COREPACK_QUERY_MARKER:-}" ]; then printf '%s\n' "$*" >> "$COREPACK_QUERY_MARKER"; fi
shift
exec "$(dirname "$0")/pnpm" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_corepack, fs::Permissions::from_mode(0o755)).unwrap();

    let mut path = std::ffi::OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    (path, repo.join("install-count"), repo.join("install-cwd"))
}

#[cfg(unix)]
fn run_pnpm_dependency_checker(
    repo: &Path,
    path: &std::ffi::OsStr,
    install_count: &Path,
    install_cwd: &Path,
    mode: &str,
) -> std::process::Output {
    std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", mode, "apps/web"])
        .current_dir(repo)
        .env("PATH", path)
        .env("INSTALL_COUNT", install_count)
        .env("INSTALL_CWD", install_cwd)
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn generated_pnpm_receipts_bind_the_runtime_and_reject_install_drift() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-runtime-contract");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.12.1+sha224.deadbeef"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"},"dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['apps/*']\n").unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let version_file = repo.join("fake-pnpm-version");
    fs::write(&version_file, "10.12.1\n").unwrap();

    let resolved_spec = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "package-manager-spec",
            "apps/web",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(resolved_spec.status.success());
    assert_eq!(
        String::from_utf8(resolved_spec.stdout).unwrap().trim(),
        "pnpm@10.12.1+sha224.deadbeef"
    );

    let run = |mode: &str, drift_to: Option<&str>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION_FILE", &version_file);
        if let Some(version) = drift_to {
            command.env("PNPM_DRIFT_TO", version);
        }
        command.output().unwrap()
    };

    let install = run("dependencies-install", None);
    assert!(
        install.status.success(),
        "runtime-bound install failed:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(run("dependencies-ready", None).status.success());

    let fake_pnpm = repo.join("fake-bin/pnpm");
    let original_fake_pnpm = fs::read(&fake_pnpm).unwrap();
    let mut changed_fake_pnpm = original_fake_pnpm.clone();
    changed_fake_pnpm.extend_from_slice(b"\n# executable identity changed\n");
    fs::write(&fake_pnpm, changed_fake_pnpm).unwrap();
    assert!(
        !run("dependencies-ready", None).status.success(),
        "changing pnpm executable content with the same version reused the receipt"
    );
    fs::write(&fake_pnpm, &original_fake_pnpm).unwrap();
    assert!(run("dependencies-ready", None).status.success());

    fs::write(&version_file, "11.13.1\n").unwrap();
    assert!(
        !run("dependencies-ready", None).status.success(),
        "changing only the effective pnpm runtime left the receipt ready"
    );
    fs::write(&version_file, "10.12.1\n").unwrap();
    assert!(run("dependencies-ready", None).status.success());

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","version":"2","scripts":{"lint":"true"},"dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    let drifted = run("dependencies-install", Some("11.13.1"));
    assert!(
        !drifted.status.success(),
        "runtime drift during install was accepted"
    );
    assert!(String::from_utf8_lossy(&drifted.stderr).contains("changed during installation"));
    assert!(
        !repo
            .join(".agent/tmp/web-dependencies/root.sha256")
            .exists()
    );
    assert!(!repo.join("node_modules/.jig-web-dependencies-v3").exists());

    fs::write(&version_file, "10.12.1\n").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","version":"3","scripts":{"lint":"true"},"dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    let lock_drift = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-install",
            "apps/web",
        ])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("INSTALL_COUNT", &install_count)
        .env("INSTALL_CWD", &install_cwd)
        .env("PNPM_VERSION_FILE", &version_file)
        .env("PNPM_DRIFT_LOCK", "1")
        .output()
        .unwrap();
    assert!(
        !lock_drift.status.success(),
        "frozen lockfile drift was accepted"
    );
    assert!(String::from_utf8_lossy(&lock_drift.stderr).contains("frozen install changed"));
    assert!(
        !repo
            .join(".agent/tmp/web-dependencies/root.sha256")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn generated_pnpm_rejects_global_virtual_store_layouts_and_environment_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-global-virtual-store-contract");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@10.12.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\nenableGlobalVirtualStore: false\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let run = |version: &str,
               global_virtual_store: &str,
               override_key: Option<&str>,
               query_marker: Option<&Path>| {
        let mut command = std::process::Command::new("bash");
        command
            .args([
                "scripts/check-webapps.sh",
                "dependencies-install",
                "apps/web",
            ])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION", version)
            .env("PNPM_ENABLE_GLOBAL_VIRTUAL_STORE", global_virtual_store);
        if let Some(key) = override_key {
            command.env(key, "false");
        }
        if let Some(marker) = query_marker {
            command.env("PNPM_QUERY_MARKER", marker);
        }
        command.output().unwrap()
    };

    for (version, setting) in [
        ("10.12.1", "false"),
        ("10.12.1", "undefined"),
        ("11.13.1", "false"),
    ] {
        let output = run(version, setting, None, None);
        assert!(
            output.status.success(),
            "pnpm {version} with enable-global-virtual-store={setting} was rejected:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        "2",
        "pnpm 10 undefined did not normalize to the same false contract"
    );

    for (version, setting) in [
        ("10.12.1", "true"),
        ("11.13.1", "true"),
        ("11.13.1", "undefined"),
    ] {
        let output = run(version, setting, None, None);
        assert!(
            !output.status.success(),
            "pnpm {version} with enable-global-virtual-store={setting} was accepted"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("enableGlobalVirtualStore: false"),
            "{stderr}"
        );
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            "2",
            "rejected pnpm layout reached install"
        );
    }

    for override_key in [
        "NpM_cOnFiG_EnAbLe_GlObAl_ViRtUaL_StOrE",
        "pNpM-cOnFiG-eNaBlE-gLoBaL-vIrTuAl-sToRe",
    ] {
        let query_marker = repo.join("pnpm-query-marker");
        if query_marker.exists() {
            fs::remove_file(&query_marker).unwrap();
        }
        let output = run("10.12.1", "false", Some(override_key), Some(&query_marker));
        assert!(
            !output.status.success(),
            "inherited {override_key} was accepted"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("environment override"), "{stderr}");
        assert!(
            stderr.contains("enableGlobalVirtualStore: false"),
            "{stderr}"
        );
        assert!(
            !query_marker.exists(),
            "pnpm metadata was queried before rejecting {override_key}"
        );
        assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");
    }
}

#[cfg(unix)]
#[test]
fn generated_pnpm_binds_supported_layout_and_proves_repository_local_virtual_store() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-layout-contract");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@11.13.1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\nenableGlobalVirtualStore: false\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let base_config = r#"{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false,"allowBuilds":{"esbuild":true},"supportedArchitectures":{"cpu":["current","x64"]},"dedupeInjectedDeps":true,"nodeLinker":"isolated"}"#;
    let run = |mode: &str, config: &str, environment: Option<(&str, &str)>| {
        let mut command = std::process::Command::new("bash");
        command
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION", "11.13.1")
            .env("PNPM_CONFIG_JSON", config);
        if let Some((key, value)) = environment {
            command.env(key, value);
        }
        command.output().unwrap()
    };

    assert!(
        run("dependencies-install", base_config, None)
            .status
            .success()
    );
    assert!(
        run("dependencies-ready", base_config, None)
            .status
            .success()
    );
    let changed_build_policy = base_config.replace("\"esbuild\":true", "\"esbuild\":false");
    assert!(
        !run("dependencies-ready", &changed_build_policy, None)
            .status
            .success(),
        "a bounded object-valued build/layout setting was omitted from the contract"
    );
    let changed_architectures = base_config.replace("\"x64\"", "\"arm64\"");
    assert!(
        !run("dependencies-ready", &changed_architectures, None)
            .status
            .success(),
        "supported pnpm architecture selection was omitted from the contract"
    );
    let changed_injected_dedupe = base_config.replace(
        "\"dedupeInjectedDeps\":true",
        "\"dedupeInjectedDeps\":false",
    );
    assert!(
        !run("dependencies-ready", &changed_injected_dedupe, None)
            .status
            .success(),
        "pnpm injected-dependency layout selection was omitted from the contract"
    );

    for config in [
        r#"{"enableGlobalVirtualStore":false,"nodeLinker":"pnp"}"#,
        r#"{"enableGlobalVirtualStore":false,"symlink":false}"#,
        r#"{"enableGlobalVirtualStore":false,"modulesDir":".mods"}"#,
        r#"{"enableGlobalVirtualStore":false,"virtualStoreOnly":true}"#,
        r#"{"enableGlobalVirtualStore":false,"nodeExperimentalPackageMap":true}"#,
        r#"{"enableGlobalVirtualStore":false,"nodePackageMapType":"mount"}"#,
        r#"{"enableGlobalVirtualStore":false,"virtualStoreDir":"../../outside"}"#,
    ] {
        assert!(
            !run("dependencies-install", config, None).status.success(),
            "unsupported pnpm layout was accepted: {config}"
        );
    }

    fs::create_dir_all(repo.join(".custom-store/pkg")).unwrap();
    fs::write(
        repo.join(".custom-store/pkg/package.json"),
        r#"{"name":"custom"}"#,
    )
    .unwrap();
    let custom_config = r#"{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false,"virtualStoreDir":".custom-store"}"#;
    assert!(
        run("dependencies-install", custom_config, None)
            .status
            .success()
    );
    assert!(
        run("dependencies-ready", custom_config, None)
            .status
            .success()
    );
    fs::write(
        repo.join(".custom-store/pkg/package.json"),
        r#"{"name":"changed"}"#,
    )
    .unwrap();
    assert!(
        !run("dependencies-ready", custom_config, None)
            .status
            .success(),
        "repository-local custom virtual store was not structurally proved"
    );

    let outside = temp.path().join("outside-virtual-store");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, repo.join("linked-store")).unwrap();
    let linked_config =
        r#"{"enableGlobalVirtualStore":false,"virtualStoreDir":"linked-store/content"}"#;
    assert!(
        !run("dependencies-install", linked_config, None)
            .status
            .success(),
        "symlink-mediated custom virtual store escaped repository ownership"
    );

    assert!(
        run(
            "dependencies-install",
            base_config,
            Some(("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN", "false")),
        )
        .status
        .success()
    );
    assert!(
        run(
            "dependencies-ready",
            base_config,
            Some(("pnpm_config_verify_deps_before_run", "false")),
        )
        .status
        .success(),
        "case-insensitive raw layout override was not normalized consistently"
    );
    assert!(
        !run("dependencies-ready", base_config, None)
            .status
            .success(),
        "removing a bound ambient layout override left the receipt ready"
    );
    let ignored_hook = run(
        "dependencies-install",
        base_config,
        Some(("NpM_cOnFiG_iGnOrE_pNpMfIlE", "false")),
    );
    assert!(!ignored_hook.status.success());
    assert!(String::from_utf8_lossy(&ignored_hook.stderr).contains("metadata-hook"));
}

#[cfg(unix)]
#[test]
fn generated_pnpm_patch_sources_follow_runtime_major_and_shared_lock_mode() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-patch-source-matrix");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("custom")).unwrap();
    fs::write(repo.join("custom/root.patch"), "root\n").unwrap();
    fs::write(repo.join("custom/yaml.patch"), "yaml\n").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":{"root@1":"custom/root.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"},"pnpm":{"patchedDependencies":{"member@1":"missing-member.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\npatchedDependencies:\n  yaml@1: custom/missing-yaml.patch\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    let run = |mode: &str, version: &str, shared: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env("PNPM_VERSION", version)
            .env("PNPM_SHARED_WORKSPACE_LOCKFILE", shared)
            .output()
            .unwrap()
    };

    assert!(
        run("dependencies-install", "10.12.1", "true")
            .status
            .success(),
        "pnpm 10 shared install did not prefer root legacy metadata"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let member_active = run("dependencies-install", "10.12.1", "false");
    assert!(!member_active.status.success());
    assert!(String::from_utf8_lossy(&member_active.stderr).contains("missing-member.patch"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    assert!(
        run("dependencies-install", "10.12.1", "false")
            .status
            .success(),
        "pnpm 10 unshared project did not inherit the root legacy map"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    let yaml_active = run("dependencies-install", "11.13.1", "true");
    assert!(!yaml_active.status.success());
    assert!(String::from_utf8_lossy(&yaml_active.stderr).contains("missing-yaml.patch"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":{"ignored@1":"missing-root.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*']\npatchedDependencies:\n  yaml@1: custom/yaml.patch\n",
    )
    .unwrap();
    assert!(
        run("dependencies-install", "11.13.1", "true")
            .status
            .success(),
        "pnpm 11 root install did not use only workspace YAML patches"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "3");

    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"},"pnpm":{"patchedDependencies":{"app@1":"missing-app.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['tools/*']\npatchedDependencies:\n  parent@1: missing-parent.patch\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    assert!(
        run("dependencies-install", "11.13.1", "true")
            .status
            .success(),
        "pnpm 11 standalone install activated ignored legacy/YAML patches"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "4");
    let v10_standalone = run("dependencies-install", "10.12.1", "true");
    assert!(!v10_standalone.status.success());
    assert!(String::from_utf8_lossy(&v10_standalone.stderr).contains("missing-app.patch"));
    assert!(!String::from_utf8_lossy(&v10_standalone.stderr).contains("missing-parent.patch"));
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "4");
}

#[cfg(unix)]
#[test]
fn generated_pnpm_reads_alternate_manifests_without_loading_pnpmfile_hooks() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-alternate-manifests");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/yaml")).unwrap();
    fs::create_dir_all(repo.join("apps/json5")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(
        repo.join("apps/yaml/package.yaml"),
        "name: yaml-app\ndependencies:\n  dep: '1'\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/json5/package.json5"),
        "{ name: 'json5-app', dependencies: { dep: '1' } }\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['apps/*']\n").unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    let hook_marker = repo.join("pnpmfile-ran");
    fs::write(
        repo.join(".pnpmfile.cjs"),
        format!(
            "require('node:fs').writeFileSync({}, 'ran')\nmodule.exports = {{}}\n",
            serde_json::to_string(&hook_marker).unwrap()
        ),
    )
    .unwrap();

    let run = |mode: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/yaml"])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("INSTALL_COUNT", &install_count)
            .env("INSTALL_CWD", &install_cwd)
            .env(
                "PNPM_PKG_JSON",
                r#"{"name":"alternate","dependencies":{"dep":"1"}}"#,
            )
            .output()
            .unwrap()
    };

    assert_eq!(run("dependencies-ready").status.code(), Some(1));
    let install = run("dependencies-install");
    assert!(
        install.status.success(),
        "alternate-manifest install failed:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(
        !hook_marker.exists(),
        "pnpmfile hook ran during manifest inspection"
    );
    assert!(run("dependencies-ready").status.success());

    for (relative, changed) in [
        (
            "apps/yaml/package.yaml",
            "name: yaml-app\nversion: '2'\ndependencies:\n  dep: '1'\n",
        ),
        (
            "apps/json5/package.json5",
            "{ name: 'json5-app', version: '2', dependencies: { dep: '1' } }\n",
        ),
    ] {
        let manifest = repo.join(relative);
        let original = fs::read(&manifest).unwrap();
        fs::write(&manifest, changed).unwrap();
        let stale = run("dependencies-ready");
        assert!(
            !stale.status.success(),
            "selected alternate manifest {relative} was omitted from the fingerprint"
        );
        assert_eq!(stale.status.code(), Some(1));
        fs::write(&manifest, original).unwrap();
        assert!(run("dependencies-ready").status.success());
    }
    assert!(!hook_marker.exists());

    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['tools/*']\n").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"packageManager":"pnpm@9.9.9"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/yaml/package.yaml"),
        "name: yaml-app\npackageManager: pnpm@10.99.0\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/yaml/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    let corepack_marker = repo.join("corepack-query");
    let resolve_alternate = |app: &str, payload: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "package-manager-spec", app])
            .current_dir(&repo)
            .env("PATH", &path)
            .env("PNPM_PKG_JSON", payload)
            .env("COREPACK_QUERY_MARKER", &corepack_marker)
            .env("npm_config_ignore_pnpmfile", "hostile")
            .env("pnpm_config_ignore_pnpmfile", "hostile")
            .env("npm_config_pm_on_fail", "error")
            .env("pnpm_config_runtime_on_fail", "error")
            .output()
            .unwrap()
    };

    let top_level = resolve_alternate("apps/yaml", r#"{"packageManager":"pnpm@10.99.0"}"#);
    assert_eq!(top_level.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(top_level.stdout).unwrap().trim(),
        "pnpm@10.99.0"
    );
    assert_eq!(
        fs::read_to_string(&corepack_marker).unwrap().trim(),
        "pnpm@11.13.0 pkg get packageManager devEngines.packageManager --json --ignore-workspace"
    );

    fs::write(
        repo.join("apps/yaml/package.yaml"),
        "name: yaml-app\ndevEngines:\n  packageManager:\n    name: pnpm\n    version: 10.98.0\n",
    )
    .unwrap();
    let dev_engine = resolve_alternate(
        "apps/yaml",
        r#"{"devEngines.packageManager":{"name":"pnpm","version":"10.98.0"}}"#,
    );
    assert_eq!(dev_engine.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(dev_engine.stdout).unwrap().trim(),
        "pnpm@10.98.0"
    );

    fs::write(
        repo.join("apps/json5/package.json5"),
        "{ name: 'json5-app', packageManager: 'pnpm@10.97.0' }\n",
    )
    .unwrap();
    fs::write(
        repo.join("apps/json5/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    let json5 = resolve_alternate("apps/json5", r#"{"packageManager":"pnpm@10.97.0"}"#);
    assert_eq!(json5.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(json5.stdout).unwrap().trim(),
        "pnpm@10.97.0"
    );

    fs::write(
        repo.join("apps/yaml/package.json"),
        r#"{"name":"json-wins"}"#,
    )
    .unwrap();
    let precedence = resolve_alternate("apps/yaml", r#"{"packageManager":"pnpm@10.97.0"}"#);
    assert_eq!(precedence.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(precedence.stdout).unwrap().trim(),
        "pnpm@9.9.9",
        "a lower-priority package.yaml must not contribute authority beside package.json"
    );
    fs::remove_file(repo.join("apps/yaml/package.json")).unwrap();

    let wrong_manager = resolve_alternate("apps/yaml", r#"{"packageManager":"yarn@4.17.1"}"#);
    assert_eq!(wrong_manager.status.code(), Some(2));
    let invalid_readiness = std::process::Command::new("bash")
        .args([
            "scripts/check-webapps.sh",
            "dependencies-ready",
            "apps/yaml",
        ])
        .current_dir(&repo)
        .env("PATH", &path)
        .env("PNPM_PKG_JSON", r#"{"packageManager":"yarn@4.17.1"}"#)
        .output()
        .unwrap();
    assert_eq!(
        invalid_readiness.status.code(),
        Some(2),
        "an absent receipt must not downgrade invalid pnpm manifest authority"
    );
    let malformed = resolve_alternate("apps/yaml", "{}\n{}");
    assert_eq!(malformed.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn generated_pnpm_workspace_globstars_include_zero_segment_members() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-zero-segment-globstar");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(
        repo.join("apps/package.json"),
        r#"{"name":"apps-root","version":"1"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();

    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*/**']\n",
    )
    .unwrap();
    let immediate = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        immediate.status.success(),
        "apps/*/** did not include the immediate apps/web member:\n{}",
        String::from_utf8_lossy(&immediate.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(&repo).unwrap().display().to_string()
    );

    fs::write(repo.join("pnpm-workspace.yaml"), "packages: [' apps/*']\n").unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-install",
        )
        .status
        .success(),
        "quoted workspace-pattern whitespace was incorrectly trimmed"
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(repo.join("pnpm-workspace.yaml"), "packages: ['apps/**']\n").unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-install",
        )
        .status
        .success()
    );
    fs::write(
        repo.join("apps/package.json"),
        r#"{"name":"apps-root","version":"2"}"#,
    )
    .unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "apps/** omitted the zero-segment apps/package.json member"
    );
}

#[cfg(unix)]
#[test]
fn generated_pnpm_workspace_walk_is_bounded_to_relevant_branches() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-bounded-workspace-walk");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web/public")).unwrap();
    fs::create_dir_all(repo.join("shared-assets")).unwrap();
    fs::create_dir_all(repo.join("vendor")).unwrap();
    fs::create_dir_all(repo.join("apps/excluded")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"dep":"1"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*', '!apps/excluded']\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    for index in 0..10_050 {
        fs::write(repo.join(format!("vendor/unrelated-{index}")), b"").unwrap();
        fs::write(repo.join(format!("apps/excluded/unrelated-{index}")), b"").unwrap();
    }
    symlink(
        "../../../shared-assets",
        repo.join("apps/web/public/assets"),
    )
    .unwrap();

    let run =
        |mode: &str| run_pnpm_dependency_checker(&repo, &path, &install_count, &install_cwd, mode);
    let install = run("dependencies-install");
    assert!(
        install.status.success(),
        "unrelated/excluded trees or a terminal-member symlink broke discovery:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );

    fs::remove_file(repo.join("apps/web/public/assets")).unwrap();
    fs::create_dir_all(repo.join("bower_components")).unwrap();
    for index in 0..10_050 {
        fs::write(repo.join(format!("bower_components/ignored-{index}")), b"").unwrap();
    }
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['**', '!vendor/**', '!apps/excluded/**']\n",
    )
    .unwrap();
    let broad_install = run("dependencies-install");
    assert!(
        broad_install.status.success(),
        "bower_components affected a broad pnpm workspace walk:\n{}",
        String::from_utf8_lossy(&broad_install.stderr)
    );
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*', '!apps/excluded']\n",
    )
    .unwrap();
    assert!(run("dependencies-install").status.success());

    symlink("../shared-assets", repo.join("apps/selected-link")).unwrap();
    let selected_link = run("dependencies-ready");
    assert!(!selected_link.status.success());
    assert!(String::from_utf8_lossy(&selected_link.stderr).contains("symbolic link"));
    fs::remove_file(repo.join("apps/selected-link")).unwrap();
    assert!(run("dependencies-ready").status.success());

    for index in 0..10_050 {
        fs::create_dir(repo.join(format!("apps/relevant-{index}"))).unwrap();
    }
    let capped = run("dependencies-ready");
    assert!(!capped.status.success());
    assert!(String::from_utf8_lossy(&capped.stderr).contains("relevant filesystem entries"));
}

#[cfg(unix)]
#[test]
fn generated_pnpm_workspace_metadata_uses_root_keys_and_keeps_selected_build_names() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-workspace-metadata");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":{"inactive-parent@1":"missing-parent-legacy.patch"}}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","scripts":{"lint":"true"},"pnpm":{"patchedDependencies":{"active@1":"patches/active.patch"}}}"#,
    )
    .unwrap();
    fs::create_dir_all(repo.join("apps/web/patches")).unwrap();
    fs::write(repo.join("apps/web/patches/active.patch"), "active-v1\n").unwrap();
    fs::write(
        repo.join("apps/web/pnpm-workspace.yaml"),
        "patchedDependencies:\n  inactive-app-workspace@1: missing-app-workspace.patch\n",
    )
    .unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    fs::write(
        repo.join("apps/web/pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .unwrap();
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "  \"packages\":\n    - 'tools/*'\n  'patchedDependencies':\n    inactive@1: missing-parent.patch\n  catalogs:\n    default:\n      packages: '^1.0.0'\n",
    )
    .unwrap();

    let inactive_local_yaml = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        !inactive_local_yaml.status.success(),
        "scope-local YAML patches that --ignore-workspace cannot apply were accepted"
    );
    let stderr = String::from_utf8_lossy(&inactive_local_yaml.stderr);
    assert!(stderr.contains("apps/web/pnpm-workspace.yaml"), "{stderr}");
    assert!(stderr.contains("--ignore-workspace"), "{stderr}");
    assert!(!install_count.exists());

    fs::remove_file(repo.join("apps/web/pnpm-workspace.yaml")).unwrap();
    let standalone = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        standalone.status.success(),
        "standalone legacy patches failed after removing inactive local YAML:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&standalone.stdout),
        String::from_utf8_lossy(&standalone.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(repo.join("apps/web"))
            .unwrap()
            .display()
            .to_string()
    );

    fs::write(repo.join("apps/web/patches/active.patch"), "active-v2\n").unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "the active app-local legacy patch was omitted from the fingerprint"
    );
    fs::write(repo.join("apps/web/patches/active.patch"), "active-v1\n").unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring the app-local legacy patch did not restore readiness"
    );

    let app_scope = repo.join("apps/web");
    let real_app_scope = repo.join("apps/web-real");
    fs::rename(&app_scope, &real_app_scope).unwrap();
    symlink(&real_app_scope, &app_scope).unwrap();
    let linked_scope = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_scope.status.success());
    assert!(
        String::from_utf8_lossy(&linked_scope.stderr).contains("symbolic links"),
        "symlinked app-scope ancestry omitted its diagnostic:\n{}",
        String::from_utf8_lossy(&linked_scope.stderr)
    );
    fs::remove_file(&app_scope).unwrap();
    fs::rename(&real_app_scope, &app_scope).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring real app-scope ancestry did not restore readiness"
    );

    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();

    let mut selected_manifests = Vec::new();
    for relative in [
        "apps/dist/package.json",
        "apps/coverage/package.json",
        "apps/target/package.json",
        "target/worker/package.json",
    ] {
        let manifest = repo.join(relative);
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, r#"{"name":"selected","version":"1"}"#).unwrap();
        selected_manifests.push(manifest);
    }
    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "\u{feff}---\n  \"packages\":\n    - 'apps/*'\n    - 'target/*'\n    - \"!apps/excluded\"\n  catalogs:\n    default:\n      packages: '^1.0.0'\n      patchedDependencies: missing.patch\n",
    )
    .unwrap();

    let valid_root_workspace = fs::read_to_string(repo.join("pnpm-workspace.yaml")).unwrap();

    let root_install = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        root_install.status.success(),
        "root pnpm install failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&root_install.stdout),
        String::from_utf8_lossy(&root_install.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_cwd).unwrap().trim(),
        fs::canonicalize(&repo).unwrap().display().to_string()
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "2");

    let apps = repo.join("apps");
    let real_apps = repo.join("apps-real");
    fs::rename(&apps, &real_apps).unwrap();
    symlink(&real_apps, &apps).unwrap();
    let linked_workspace_ancestor = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_workspace_ancestor.status.success());
    assert!(
        String::from_utf8_lossy(&linked_workspace_ancestor.stderr).contains("symbolic link"),
        "selected workspace-member ancestor symlink omitted its diagnostic:\n{}",
        String::from_utf8_lossy(&linked_workspace_ancestor.stderr)
    );
    fs::remove_file(&apps).unwrap();
    fs::rename(&real_apps, &apps).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring the real workspace-member ancestor did not restore readiness"
    );

    for manifest in selected_manifests {
        fs::write(&manifest, r#"{"name":"selected","version":"2"}"#).unwrap();
        assert!(
            !run_pnpm_dependency_checker(
                &repo,
                &path,
                &install_count,
                &install_cwd,
                "dependencies-ready",
            )
            .status
            .success(),
            "selected build-name manifest {} was omitted",
            manifest.display()
        );
        fs::write(&manifest, r#"{"name":"selected","version":"1"}"#).unwrap();
        assert!(
            run_pnpm_dependency_checker(
                &repo,
                &path,
                &install_count,
                &install_cwd,
                "dependencies-ready",
            )
            .status
            .success(),
            "restoring {} did not restore readiness",
            manifest.display()
        );
    }

    fs::write(
        repo.join("pnpm-workspace.yaml"),
        "packages: ['apps/*', 'target/*',]\n",
    )
    .unwrap();
    let trailing_comma = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "package-manager-spec",
    );
    assert!(
        trailing_comma.status.success(),
        "valid YAML flow trailing comma was rejected:\n{}",
        String::from_utf8_lossy(&trailing_comma.stderr)
    );
    fs::write(repo.join("pnpm-workspace.yaml"), &valid_root_workspace).unwrap();

    let invalid_workspaces = [
        (
            "packages:\n  - 'apps/*'\n\"packages\":\n  - 'target/*'\n",
            "declares packages more than once",
        ),
        ("{packages: ['apps/*']}\n", "root flow mappings"),
        ("- packages: ['apps/*']\n", "root block sequences"),
        ("? packages\n: ['apps/*']\n", "explicit mapping keys"),
        ("packages: [{x: y}]\n", "unsupported YAML collection syntax"),
        (
            "packages:\n  - key: value\n",
            "unsupported YAML collection syntax",
        ),
        (
            "packages:\n  - - nested\n",
            "unsupported YAML collection syntax",
        ),
        ("packages:\n  -apps/*\n", "non-string sequence entry"),
        (
            "packages: [, 'apps/*']\n",
            "packages contains an empty glob",
        ),
        (
            "packages: ['apps/*',,]\n",
            "packages contains an empty glob",
        ),
        (
            "packages:\n  - &web 'apps/*'\n",
            "unsupported YAML node properties",
        ),
        ("packages:\n  - *web\n", "unsupported YAML node properties"),
        (
            "packages:\n  - !!str 'apps/*'\n",
            "unsupported YAML node properties",
        ),
        (
            "packages:\n  - |-\n      apps/*\n",
            "unsupported YAML node properties",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies:\n  dep@1: &patch custom.patch\n",
            "unsupported YAML node properties",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies:\n  &selector dep@1: custom.patch\n",
            "unsupported YAML node properties",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies:\n  dep@1: custom.patch\n  \"dep@1\": custom.patch\n",
            "declares patchedDependencies selector \"dep@1\" more than once",
        ),
        (
            "packages: ['apps/*']\npatchedDependencies: {dep@1: custom.patch}\n",
            "patchedDependencies must be a block mapping",
        ),
        (
            "packages: ['apps/*']\n---\npatchedDependencies:\n  dep@1: custom.patch\n",
            "multiple YAML documents",
        ),
        ("packages: ['apps/*']\n...\n", "document end markers"),
    ];
    for (workspace, diagnostic) in invalid_workspaces {
        fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();
        let invalid = run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        );
        assert!(
            !invalid.status.success(),
            "invalid YAML was accepted: {workspace}"
        );
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains(diagnostic),
            "invalid YAML omitted {diagnostic:?}:\n{}",
            String::from_utf8_lossy(&invalid.stderr)
        );
    }
    fs::write(repo.join("pnpm-workspace.yaml"), valid_root_workspace).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring valid indented and quoted pnpm workspace YAML did not restore readiness"
    );
}

#[cfg(unix)]
#[test]
fn generated_pnpm_custom_patch_fingerprints_are_required_and_repository_bounded() {
    use std::os::unix::fs::symlink;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("pnpm-custom-patches");
    let (path, install_count, install_cwd) =
        init_pnpm_dependency_checker_fixture(&repo, template.path());

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("custom")).unwrap();
    fs::create_dir_all(repo.join("linked-parent")).unwrap();
    let root_manifest =
        r#"{"private":true,"pnpm":{"patchedDependencies":{"legacy@1":"custom/root.patch"}}}"#;
    let member_manifest = r#"{"name":"web","scripts":{"lint":"true"},"pnpm":{"patchedDependencies":{"member@1":"../../custom/member.patch"}}}"#;
    fs::write(repo.join("package.json"), root_manifest).unwrap();
    fs::write(repo.join("apps/web/package.json"), member_manifest).unwrap();
    fs::write(repo.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    for (relative, contents) in [
        ("custom/yaml.patch", "yaml-v1\n"),
        ("custom/root.patch", "root-v1\n"),
        ("custom/member.patch", "member-v1\n"),
        ("linked-parent/parent.patch", "parent-v1\n"),
        ("custom/unreferenced.patch", "unreferenced-v1\n"),
    ] {
        fs::write(repo.join(relative), contents).unwrap();
    }
    let workspace = "packages:\n  - 'apps/*'\npatchedDependencies:\n  yaml@1: custom/yaml.patch\n  parent@1: linked-parent/parent.patch\n";
    fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();

    let install = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        install.status.success(),
        "custom-patch install failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    let active_patch = "custom/root.patch";
    let patch = repo.join(active_patch);
    let original = fs::read(&patch).unwrap();
    fs::write(&patch, "changed patch\n").unwrap();
    assert!(
        !run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "configured patch {active_patch} was omitted from the fingerprint"
    );
    fs::write(&patch, original).unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "restoring configured patch {active_patch} did not restore readiness"
    );

    for relative in [
        "custom/yaml.patch",
        "custom/member.patch",
        "linked-parent/parent.patch",
    ] {
        let patch = repo.join(relative);
        let original = fs::read(&patch).unwrap();
        fs::write(&patch, "inactive patch changed\n").unwrap();
        assert!(
            run_pnpm_dependency_checker(
                &repo,
                &path,
                &install_count,
                &install_cwd,
                "dependencies-ready",
            )
            .status
            .success(),
            "inactive pnpm 10 patch source {relative} changed readiness"
        );
        fs::write(&patch, original).unwrap();
    }

    fs::write(repo.join("custom/unreferenced.patch"), "unreferenced-v2\n").unwrap();
    assert!(
        run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-ready",
        )
        .status
        .success(),
        "an unreferenced custom patch changed the fingerprint"
    );

    let root_patch = repo.join("custom/root.patch");
    let root_contents = fs::read(&root_patch).unwrap();
    fs::remove_file(&root_patch).unwrap();
    let missing = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!missing.status.success());
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
    fs::write(&root_patch, &root_contents).unwrap();

    let symlink_target = repo.join("custom/root-real.patch");
    fs::rename(&root_patch, &symlink_target).unwrap();
    symlink(&symlink_target, &root_patch).unwrap();
    let linked_file = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_file.status.success());
    assert_eq!(linked_file.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&linked_file.stderr).contains("symbolic links"));
    fs::remove_file(&root_patch).unwrap();
    fs::rename(&symlink_target, &root_patch).unwrap();

    let linked_parent = repo.join("custom");
    let real_parent = repo.join("custom-real");
    fs::rename(&linked_parent, &real_parent).unwrap();
    symlink(&real_parent, &linked_parent).unwrap();
    let linked_directory = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert!(!linked_directory.status.success());
    assert_eq!(linked_directory.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&linked_directory.stderr).contains("symbolic links"));
    fs::remove_file(&linked_parent).unwrap();
    fs::rename(&real_parent, &linked_parent).unwrap();

    let outside_patch = temp.path().join("outside.patch");
    fs::write(&outside_patch, "outside\n").unwrap();
    // Without root legacy metadata, pnpm 10 falls back to the workspace YAML.
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    let invalid_paths = [
        ("../outside.patch".to_owned(), "escapes the repository"),
        (
            format!("'{}'", outside_patch.display()),
            "must be repository-relative",
        ),
        (
            r"'C:\outside.patch'".to_owned(),
            "must be repository-relative",
        ),
        (
            r"'\\server\share\outside.patch'".to_owned(),
            "must be repository-relative",
        ),
        ("custom".to_owned(), "must be a file"),
        ("false".to_owned(), "must be a string"),
    ];
    for (configured_path, diagnostic) in invalid_paths {
        fs::write(
            repo.join("pnpm-workspace.yaml"),
            format!("packages:\n  - 'apps/*'\npatchedDependencies:\n  yaml@1: {configured_path}\n"),
        )
        .unwrap();
        let invalid = run_pnpm_dependency_checker(
            &repo,
            &path,
            &install_count,
            &install_cwd,
            "dependencies-install",
        );
        assert!(
            !invalid.status.success(),
            "invalid patch path {configured_path} was accepted"
        );
        assert_eq!(
            invalid.status.code(),
            Some(2),
            "invalid patch path {configured_path} was not a hard authority error"
        );
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains(diagnostic),
            "invalid patch path {configured_path} omitted {diagnostic:?}:\n{}",
            String::from_utf8_lossy(&invalid.stderr)
        );
        assert_eq!(
            fs::read_to_string(&install_count).unwrap().trim(),
            "1",
            "the installer ran for invalid patch path {configured_path}"
        );
    }

    fs::write(repo.join("pnpm-workspace.yaml"), workspace).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"pnpm":{"patchedDependencies":[]}}"#,
    )
    .unwrap();
    let malformed_manifest = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(!malformed_manifest.status.success());
    assert_eq!(malformed_manifest.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&malformed_manifest.stderr)
            .contains("pnpm.patchedDependencies must be an object")
    );
    assert_eq!(fs::read_to_string(&install_count).unwrap().trim(), "1");

    fs::write(repo.join("package.json"), root_manifest).unwrap();
    let restored_readiness = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-ready",
    );
    assert_eq!(
        restored_readiness.status.code(),
        Some(0),
        "restoring exact patch authority did not reattest the existing receipt:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&restored_readiness.stdout),
        String::from_utf8_lossy(&restored_readiness.stderr)
    );
    let restored_install = run_pnpm_dependency_checker(
        &repo,
        &path,
        &install_count,
        &install_cwd,
        "dependencies-install",
    );
    assert!(
        restored_install.status.success(),
        "restoring valid patch metadata did not reuse the existing receipt:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&restored_install.stdout),
        String::from_utf8_lossy(&restored_install.stderr)
    );
    assert_eq!(
        fs::read_to_string(&install_count).unwrap().trim(),
        "1",
        "restoring exact authority unnecessarily reran the package manager"
    );
}

#[cfg(unix)]
#[test]
fn generated_web_dependency_scope_and_fingerprints_use_only_selected_manager_metadata() {
    use std::ffi::OsString;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let cases = [
        (
            "npm-package-wins",
            "npm",
            r#"{"private":true,"workspaces":["apps/*"]}"#,
            "packages:\n  - 'tools/*'\n  - '!apps/web'\n",
            true,
        ),
        (
            "npm-brace-workspace",
            "npm",
            r#"{"private":true,"workspaces":["apps/{web,admin}"]}"#,
            "packages:\n  - 'tools/*'\n",
            true,
        ),
        (
            "npm-ignores-yarn-object",
            "npm",
            r#"{"private":true,"workspaces":{"packages":["apps/*"]}}"#,
            "packages:\n  - 'tools/*'\n",
            false,
        ),
        (
            "bun-ignores-pnpm",
            "bun",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages:\n  - 'apps/*'\n",
            false,
        ),
        (
            "bun-character-class-workspace",
            "bun",
            r#"{"private":true,"workspaces":["apps/[w]eb"]}"#,
            "packages:\n  - 'tools/*'\n",
            true,
        ),
        (
            "yarn-object-wins",
            "yarn",
            r#"{"private":true,"workspaces":{"packages":["apps/*"]}}"#,
            "packages:\n  - '!apps/web'\n",
            true,
        ),
        (
            "pnpm-ignores-package",
            "pnpm",
            r#"{"private":true,"workspaces":["apps/*"]}"#,
            "packages: ['tools/*', 'tools/hash#workspace'] # app excluded\n",
            false,
        ),
        (
            "pnpm-workspace-wins",
            "pnpm",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages:\n  - 'apps/*' # web application\n  - tools/hash#workspace\n",
            true,
        ),
        (
            "pnpm-flow-comment-wins",
            "pnpm",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages: ['apps/*', 'tools/hash#workspace'] # web application\n",
            true,
        ),
        (
            "pnpm-brace-workspace",
            "pnpm",
            r#"{"private":true,"workspaces":["tools/*"]}"#,
            "packages: ['apps/{web,admin}']\n",
            true,
        ),
    ];

    for (case_name, package_manager, package_json, pnpm_workspace, root_scope) in cases {
        let repo = temp.path().join(case_name);
        run_init(InitOpts {
            path: repo.clone(),
            scaffold: ScaffoldOpts::default(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some(case_name.into()),
                sqlx_enabled: Some(false),
                web_package_manager: Some(package_manager.into()),
                frontend_apps: vec![FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        fs::create_dir_all(repo.join("apps/web")).unwrap();
        fs::write(repo.join("package.json"), package_json).unwrap();
        fs::write(
            repo.join("apps/web/package.json"),
            r#"{"name":"web","scripts":{"lint":"true"}}"#,
        )
        .unwrap();
        fs::write(repo.join("pnpm-workspace.yaml"), pnpm_workspace).unwrap();
        if package_manager == "yarn" {
            fs::write(repo.join(".yarnrc.yml"), "nodeLinker: node-modules\n").unwrap();
        }

        let lockfile = match package_manager {
            "bun" => "bun.lock",
            "npm" => "package-lock.json",
            "pnpm" => "pnpm-lock.yaml",
            "yarn" => "yarn.lock",
            _ => unreachable!(),
        };
        fs::write(
            repo.join(lockfile),
            if package_manager == "yarn" {
                "__metadata:\n  version: 8\n"
            } else {
                "unrelated root lock\n"
            },
        )
        .unwrap();
        if package_manager == "pnpm" && !root_scope {
            fs::write(
                repo.join("apps/web/pnpm-lock.yaml"),
                "lockfileVersion: '9.0'\n",
            )
            .unwrap();
        }
        if package_manager == "npm" && !root_scope {
            fs::write(
                repo.join("apps/web/package-lock.json"),
                "standalone app lock\n",
            )
            .unwrap();
        }
        let fake_bin = repo.join("fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_manager = fake_bin.join(package_manager);
        fs::write(
            &fake_manager,
            r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    case "$(basename "$0")" in
      pnpm) printf '%s\n' '10.12.1' ;;
      yarn) printf '%s\n' '4.17.1' ;;
      *) exit 2 ;;
    esac
    ;;
  ci|install)
    pwd > "$INSTALL_CWD"
    if [ "$(basename "$0")" = yarn ]; then
      [ -f "$LOCK_NAME" ] || printf '%s\n' '__metadata:' '  version: 8' > "$LOCK_NAME"
    else
      [ -f "$LOCK_NAME" ] || printf '%s\n' lock > "$LOCK_NAME"
    fi
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  config)
    if [ "$(basename "$0")" = pnpm ]; then
      [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
      printf '%s\n' '{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false}'
      exit 0
    fi
    [ "$(basename "$0")" = yarn ] && [ "${2:-}" = --json ] || exit 2
    scope="$(pwd -P)"
    printf '%s\n' '{"key":"nodeLinker","effective":"node-modules"}'
    printf '{"key":"cacheFolder","effective":"%s/.yarn/cache"}\n' "$scope"
    printf '{"key":"installStatePath","effective":"%s/.yarn/install-state.gz"}\n' "$scope"
    printf '{"key":"pnpUnpluggedFolder","effective":"%s/.yarn/unplugged"}\n' "$scope"
    printf '%s\n' '{"key":"pnpEnableInlining","effective":true}'
    printf '%s\n' '{"key":"pnpEnableEsmLoader","effective":false}'
    ;;
  pkg)
    [ "$(basename "$0")" = pnpm ] && [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 2
    printf '%s\n' '{}'
    ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();
        let install_cwd = repo.join("install-cwd");
        let mut path = OsString::from(fake_bin.as_os_str());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let run = |mode: &str| {
            std::process::Command::new("bash")
                .args(["scripts/check-webapps.sh", mode, "apps/web"])
                .current_dir(&repo)
                .env("PATH", &path)
                .env("INSTALL_CWD", &install_cwd)
                .env("LOCK_NAME", lockfile)
                .output()
                .unwrap()
        };

        let install = run("dependencies-install");
        assert!(
            install.status.success(),
            "{case_name} install failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&install.stdout),
            String::from_utf8_lossy(&install.stderr)
        );
        let expected_cwd = if root_scope {
            fs::canonicalize(&repo).unwrap()
        } else {
            fs::canonicalize(repo.join("apps/web")).unwrap()
        };
        assert_eq!(
            fs::read_to_string(&install_cwd).unwrap().trim(),
            expected_cwd.display().to_string(),
            "{case_name} chose the wrong dependency scope"
        );
        assert!(run("dependencies-ready").status.success());

        if case_name == "npm-package-wins" {
            fs::create_dir_all(repo.join("apps/worker")).unwrap();
            fs::write(
                repo.join("apps/worker/package.json"),
                r#"{"name":"worker","version":"1"}"#,
            )
            .unwrap();
            assert!(
                !run("dependencies-ready").status.success(),
                "a newly discovered authoritative workspace manifest did not stale readiness"
            );
            assert!(run("dependencies-install").status.success());
            fs::write(
                repo.join("apps/worker/package.json"),
                r#"{"name":"worker","version":"2"}"#,
            )
            .unwrap();
            assert!(
                !run("dependencies-ready").status.success(),
                "an unconfigured workspace manifest was omitted from the root fingerprint"
            );
            assert!(run("dependencies-install").status.success());
        }

        if matches!(
            case_name,
            "bun-character-class-workspace" | "pnpm-workspace-wins"
        ) {
            fs::create_dir_all(repo.join("patches")).unwrap();
            fs::write(repo.join("patches/dependency.patch"), "patch-v1\n").unwrap();
            assert!(
                !run("dependencies-ready").status.success(),
                "{package_manager} root patch inputs were omitted from the fingerprint"
            );
            assert!(run("dependencies-install").status.success());
        }

        let irrelevant = match package_manager {
            "npm" => "bunfig.toml",
            "pnpm" => ".yarnrc",
            "bun" => ".pnpmfile.cjs",
            "yarn" => "bunfig.toml",
            _ => unreachable!(),
        };
        fs::write(repo.join(irrelevant), "irrelevant manager config\n").unwrap();
        assert!(
            run("dependencies-ready").status.success(),
            "{case_name} fingerprint included irrelevant manager config"
        );

        if case_name == "pnpm-workspace-wins" {
            let workspace = repo.join("pnpm-workspace.yaml");
            let original = fs::read_to_string(&workspace).unwrap();
            fs::write(&workspace, "packages: invalid-scalar\n").unwrap();
            let malformed = run("dependencies-ready");
            assert!(!malformed.status.success());
            assert!(
                String::from_utf8_lossy(&malformed.stderr)
                    .contains("packages must be a block or flow sequence")
            );
            fs::write(&workspace, original).unwrap();
            assert!(run("dependencies-ready").status.success());
        }

        let relevant = match package_manager {
            "npm" => ".npmrc",
            "pnpm" => ".pnpmfile.cjs",
            "bun" => "bunfig.toml",
            "yarn" => ".yarnrc",
            _ => unreachable!(),
        };
        let relevant_path = repo.join(relevant);
        let original_relevant = fs::read(&relevant_path).ok();
        if relevant_path.exists() {
            fs::remove_file(&relevant_path).unwrap();
        }
        let relevant_target = repo.join("selected-manager-config-target");
        fs::write(&relevant_target, "selected manager config\n").unwrap();
        symlink(&relevant_target, &relevant_path).unwrap();
        assert!(
            !run("dependencies-ready").status.success(),
            "{case_name} accepted a symlinked manager config input"
        );
        fs::remove_file(&relevant_path).unwrap();
        fs::remove_file(&relevant_target).unwrap();
        if let Some(original) = original_relevant {
            fs::write(&relevant_path, original).unwrap();
        }
        assert!(run("dependencies-ready").status.success());

        if case_name == "npm-package-wins" {
            let manifest = repo.join("package.json");
            let manifest_target = repo.join("package-target.json");
            fs::rename(&manifest, &manifest_target).unwrap();
            symlink(&manifest_target, &manifest).unwrap();
            assert!(
                !run("dependencies-ready").status.success(),
                "a symlinked authoritative package manifest was accepted"
            );
            fs::remove_file(&manifest).unwrap();
            fs::rename(&manifest_target, &manifest).unwrap();
            assert!(run("dependencies-ready").status.success());
        }

        fs::write(repo.join(relevant), "selected manager config changed\n").unwrap();
        assert!(
            !run("dependencies-ready").status.success(),
            "{case_name} fingerprint ignored selected manager config"
        );
    }
}

#[cfg(unix)]
#[test]
fn generated_web_dependency_fingerprints_isolate_mixed_root_and_app_scopes() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("mixed-web-scopes");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("mixed-web-scopes".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![
                FrontendApp {
                    name: "root-web".into(),
                    dir: "apps/root-web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
                FrontendApp {
                    name: "legacy-web".into(),
                    dir: "legacy-web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                },
            ],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/root-web")).unwrap();
    fs::create_dir_all(repo.join("legacy-web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "root-lock\n").unwrap();
    fs::write(
        repo.join("apps/root-web/package.json"),
        r#"{"name":"root-web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(
        repo.join("legacy-web/package.json"),
        r#"{"name":"legacy-web","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    fs::write(repo.join("legacy-web/package-lock.json"), "app-lock\n").unwrap();

    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  install|ci)
    mkdir -p node_modules/test-package
    printf '%s\n' '{"name":"test-package"}' > node_modules/test-package/package.json
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let output = std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", "bootstrap"])
        .current_dir(&repo)
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "mixed-scope bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let ready = |app_dir: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", "dependencies-ready", app_dir])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    };
    assert!(ready("apps/root-web"));
    assert!(ready("legacy-web"));

    fs::write(
        repo.join("legacy-web/package.json"),
        r#"{"name":"legacy-web","version":"2","scripts":{"lint":"true"}}"#,
    )
    .unwrap();
    assert!(
        ready("apps/root-web"),
        "app-local package changes must not stale the root workspace receipt"
    );
    assert!(!ready("legacy-web"));
}

#[cfg(unix)]
#[test]
fn generated_root_receipt_attests_workspace_member_node_modules_and_launcher_bytes_and_mode() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("member-node-modules-receipt");
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("member-node-modules-receipt".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{"name":"web","dependencies":{"tool":"1"}}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), r#"{"lockfileVersion":3}"#).unwrap();
    let fake_bin = repo.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_npm = fake_bin.join("npm");
    fs::write(
        &fake_npm,
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  ci|install)
    if [ "$1" = install ]; then
      : > .bootstrap-install-ran
    fi
    mkdir -p node_modules/tool apps/web/node_modules/.bin apps/web/node_modules/runtime-owner
    printf '%s\n' '{"name":"tool"}' > node_modules/tool/package.json
    printf '%s\n' '{"name":"runtime-owner","v":1}' > apps/web/node_modules/runtime-owner/package.json
    printf '%s\n' 'layout-v1' > apps/web/node_modules/.modules.yaml
    printf '%s\n' '#!/bin/sh' 'exit 0' > apps/web/node_modules/.bin/tool
    chmod 755 apps/web/node_modules/.bin/tool
    ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_npm, fs::Permissions::from_mode(0o755)).unwrap();
    let mut path = OsString::from(fake_bin.as_os_str());
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let run = |mode: &str| {
        std::process::Command::new("bash")
            .args(["scripts/check-webapps.sh", mode, "apps/web"])
            .current_dir(&repo)
            .env("PATH", &path)
            .output()
            .unwrap()
    };

    let install = run("dependencies-install");
    assert!(
        install.status.success(),
        "workspace install failed:\n{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(run("dependencies-ready").status.success());

    let member_modules = repo.join("apps/web/node_modules");
    for node_modules in [repo.join("node_modules"), member_modules.clone()] {
        for cache_name in [".cache", ".vite", ".vite-temp", ".tmp"] {
            let cache = node_modules.join(cache_name);
            fs::create_dir(&cache).unwrap();
            fs::write(cache.join("runtime-state"), "first\n").unwrap();
            assert!(
                run("dependencies-ready").status.success(),
                "top-level runtime cache {cache_name} invalidated readiness"
            );
            fs::write(cache.join("runtime-state"), "rewritten runtime state\n").unwrap();
            assert!(
                run("dependencies-ready").status.success(),
                "rewriting top-level runtime cache {cache_name} invalidated readiness"
            );
            fs::remove_dir_all(&cache).unwrap();
            assert!(run("dependencies-ready").status.success());
        }

        let finder_metadata = node_modules.join(".DS_Store");
        fs::write(&finder_metadata, "first\n").unwrap();
        assert!(run("dependencies-ready").status.success());
        fs::write(&finder_metadata, "rewritten Finder state\n").unwrap();
        assert!(run("dependencies-ready").status.success());
        fs::remove_file(&finder_metadata).unwrap();
        assert!(run("dependencies-ready").status.success());
    }

    let cache_type_replacement = member_modules.join(".vite");
    fs::write(&cache_type_replacement, "not a cache directory\n").unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "a file replacing a member runtime-cache directory escaped the receipt"
    );
    fs::remove_file(&cache_type_replacement).unwrap();
    assert!(run("dependencies-ready").status.success());
    std::os::unix::fs::symlink("runtime-owner", &cache_type_replacement).unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "a symlink replacing a member runtime-cache directory escaped the receipt"
    );
    fs::remove_file(&cache_type_replacement).unwrap();
    assert!(run("dependencies-ready").status.success());

    let nested_cache = member_modules.join("runtime-owner/.vite");
    fs::create_dir(&nested_cache).unwrap();
    fs::write(nested_cache.join("runtime-state"), "nested\n").unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "a cache-like directory below a package entry escaped the receipt"
    );
    fs::remove_dir_all(&nested_cache).unwrap();
    assert!(run("dependencies-ready").status.success());

    let package_metadata = member_modules.join("runtime-owner/package.json");
    fs::write(&package_metadata, "{\"name\":\"runtime-owner\",\"v\":2}\n").unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "same-size member package metadata mutation escaped the receipt"
    );
    fs::write(&package_metadata, "{\"name\":\"runtime-owner\",\"v\":1}\n").unwrap();
    assert!(run("dependencies-ready").status.success());

    let modules_metadata = member_modules.join(".modules.yaml");
    fs::write(&modules_metadata, "layout-v2\n").unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "same-size member .modules.yaml mutation escaped the receipt"
    );
    fs::write(&modules_metadata, "layout-v1\n").unwrap();
    assert!(run("dependencies-ready").status.success());

    for receipt_like_name in [
        ".jig-web-dependencies-v3",
        ".jig-web-dependencies-v3.tmp.untrusted",
    ] {
        let receipt_like = member_modules.join(receipt_like_name);
        fs::write(&receipt_like, "untrusted\n").unwrap();
        assert!(
            !run("dependencies-ready").status.success(),
            "member receipt-like file {receipt_like_name} escaped the receipt"
        );
        fs::remove_file(&receipt_like).unwrap();
        assert!(run("dependencies-ready").status.success());
    }

    let launcher = repo.join("apps/web/node_modules/.bin/tool");
    fs::write(&launcher, "#!/bin/sh\nexit 1\n").unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "same-role member launcher content mutation escaped the receipt"
    );
    fs::write(&launcher, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(run("dependencies-ready").status.success());

    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "member launcher execution mode mutation escaped the receipt"
    );
    let bootstrap = run("dependencies-bootstrap");
    assert!(
        bootstrap.status.success(),
        "non-frozen dependency bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&bootstrap.stdout),
        String::from_utf8_lossy(&bootstrap.stderr)
    );
    assert!(
        repo.join(".bootstrap-install-ran").is_file(),
        "dependency bootstrap did not use the package manager's non-frozen install mode"
    );
    assert!(run("dependencies-ready").status.success());

    let saved_modules = repo.join("apps/web/node_modules.saved");
    fs::rename(&member_modules, &saved_modules).unwrap();
    assert!(
        !run("dependencies-ready").status.success(),
        "workspace-member node_modules presence change escaped the receipt"
    );
    fs::rename(&saved_modules, &member_modules).unwrap();
    assert!(run("dependencies-ready").status.success());
}

#[cfg(unix)]
#[test]
fn generated_web_dependency_receipts_accept_only_genuinely_dependency_free_installs() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for (package_manager, lockfile, creates_empty_directory) in [
        ("npm", "package-lock.json", false),
        ("bun", "bun.lock", true),
    ] {
        let repo = temp.path().join(format!("empty-{package_manager}"));
        run_init(InitOpts {
            path: repo.clone(),
            scaffold: ScaffoldOpts::default(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some(format!("empty-{package_manager}")),
                sqlx_enabled: Some(false),
                web_package_manager: Some(package_manager.into()),
                frontend_apps: vec![FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                    role: "spa".into(),
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        fs::create_dir_all(repo.join("apps/web")).unwrap();
        fs::write(
            repo.join("package.json"),
            r#"{"private":true,"workspaces":["apps/*"]}"#,
        )
        .unwrap();
        fs::write(
            repo.join("apps/web/package.json"),
            r#"{"name":"web","scripts":{"lint":"true"}}"#,
        )
        .unwrap();

        let fake_bin = repo.join("fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        let fake_manager = fake_bin.join(package_manager);
        fs::write(
            &fake_manager,
            r#"#!/bin/sh
set -eu
case "${1:-}" in
  --version)
    case "$(basename "$0")" in
      pnpm) printf '%s\n' '10.12.1' ;;
      yarn) printf '%s\n' '4.17.1' ;;
      *) exit 2 ;;
    esac
    ;;
  config)
    [ "$(basename "$0")" = pnpm ] && [ "${2:-}" = list ] && [ "${3:-}" = --json ] || exit 2
    printf '%s\n' '{"sharedWorkspaceLockfile":true,"enableGlobalVirtualStore":false}'
    ;;
  pkg)
    [ "$(basename "$0")" = pnpm ] && [ "${NPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ "${PNPM_CONFIG_IGNORE_PNPMFILE:-}" = true ] && [ -z "${npm_config_ignore_pnpmfile+x}" ] && [ -z "${pnpm_config_ignore_pnpmfile+x}" ] || exit 2
    printf '%s\n' '{}'
    ;;
  ci|install)
    printf '%s\n' lock > "$LOCK_NAME"
    if [ "$CREATE_EMPTY_DIRECTORY" = "1" ]; then mkdir -p node_modules; fi
    ;;
  run) ;;
  *) exit 2 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&fake_manager, fs::Permissions::from_mode(0o755)).unwrap();
        let mut path = OsString::from(fake_bin.as_os_str());
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let command = |mode: &str| {
            let mut command = std::process::Command::new("bash");
            command.args(["scripts/check-webapps.sh", mode]);
            if mode == "dependencies-ready" {
                command.arg("apps/web");
            }
            command
                .current_dir(&repo)
                .env("PATH", &path)
                .env("LOCK_NAME", lockfile)
                .env(
                    "CREATE_EMPTY_DIRECTORY",
                    if creates_empty_directory { "1" } else { "0" },
                )
                .output()
                .unwrap()
        };

        let bootstrap = command("bootstrap");
        assert!(
            bootstrap.status.success(),
            "dependency-free {package_manager} bootstrap failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&bootstrap.stdout),
            String::from_utf8_lossy(&bootstrap.stderr)
        );
        assert_eq!(repo.join("node_modules").is_dir(), creates_empty_directory);
        assert!(command("dependencies-ready").status.success());

        fs::write(
            repo.join("apps/web/package.json"),
            r#"{"name":"web","scripts":{"lint":"true"},"dependencies":{"dep":"1.0.0"}}"#,
        )
        .unwrap();
        assert!(!command("dependencies-ready").status.success());
        assert!(
            !command("bootstrap").status.success(),
            "{package_manager} accepted an empty artifact after dependencies were declared"
        );
    }
}

#[test]
fn adopt_rejects_frontend_app_missing_required_ci_scripts() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": null,
    "typecheck": "tsc --noEmit"
  }
}
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("missing package.json scripts required by generated web CI"));
    assert!(error.contains("lint, build:bundle, test:coverage"));
    assert!(error.contains("remove the entry from frontend_apps"));
}

#[test]
fn adopt_requires_a_lockfile_and_accepts_an_app_local_npm_shrinkwrap() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("does not have a lockfile for npm"));
    assert!(error.contains("repo root or app directory"));
    assert!(error.contains("remove the entry from frontend_apps"));

    fs::write(repo.join("apps/web/npm-shrinkwrap.json"), "{}").unwrap();
    run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();
}
