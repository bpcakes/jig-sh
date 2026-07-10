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
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(bad_dir.contains("must not contain '..'"));

    let dot_dir = run_init(InitOpts {
        path: temp.path().join("dot-dir"),
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
                dir: "apps/./web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(dot_dir.contains("must not contain '.' path components"));

    let empty_segment_dir = run_init(InitOpts {
        path: temp.path().join("empty-segment-dir"),
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
                dir: "apps//web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(empty_segment_dir.contains("must not contain empty path components"));

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
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(absolute_dir.contains("must be relative"));

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
                },
                FrontendApp {
                    name: "web_app".into(),
                    dir: "apps/web_app".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
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
    assert!(answers.contains("argv = [\"npm\", \"run\", \"dev\"]"));
    assert!(!answers.contains("dev_command"));

    assert!(!repo.join("Makefile").exists());
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    assert!(web_check.contains("npm ci"));
    assert!(web_check.contains("npm run"));
    assert!(web_check.contains("if [ -f package.json ] && [ -f package-lock.json ]"));
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
    assert!(web_workflow.contains("npm ci"));
    assert!(web_workflow.contains("node scripts/check-webapp-scripts.mjs"));
    assert!(web_workflow.contains("node scripts/enforce-coverage.cjs"));
    assert!(!web_workflow.contains("make enforce-coverage"));
    assert!(!web_workflow.contains("oven-sh/setup-bun"));

    let rust_workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    assert!(rust_workflow.contains("scripts/jig check fmt"));
    assert!(!rust_workflow.contains("scripts/jig fmt-check"));

    let agent_map_workflow =
        fs::read_to_string(repo.join(".github/workflows/agent-map-check.yml")).unwrap();
    assert!(agent_map_workflow.contains("scripts/jig check agent-map"));
    assert!(!agent_map_workflow.contains("scripts/jig agent-map check"));
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
        ("npm", "npm ci", "npm run"),
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
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        assert!(!repo.join("Makefile").exists());
        let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
        assert!(
            web_check.contains(install_command),
            "missing install command for {package_manager}"
        );
        assert!(
            web_check.contains(run_command),
            "missing run command for {package_manager}"
        );
        let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
        assert!(
            answers.contains(&format!("argv = [\"{package_manager}\", \"run\", \"dev\"]")),
            "missing dev app argv for {package_manager}"
        );

        let workflow =
            fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
        if package_manager == "bun" {
            assert!(workflow.contains("oven-sh/setup-bun@v2"));
            assert!(workflow.contains("node-version: 22"));
        } else {
            assert!(workflow.contains(&format!("cache: {package_manager}")));
        }
        if matches!(package_manager, "pnpm" | "yarn") {
            assert!(workflow.contains("corepack enable"));
            let corepack = workflow.find("corepack enable").unwrap();
            let cache = workflow.find(&format!("cache: {package_manager}")).unwrap();
            assert!(
                corepack < cache,
                "corepack must be enabled before {package_manager} cache setup"
            );
        }
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
fn adopt_rejects_frontend_app_without_repo_or_app_lockfile() {
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
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("does not have a lockfile for npm"));
    assert!(error.contains("repo root or app directory"));
    assert!(error.contains("remove the entry from frontend_apps"));
}
