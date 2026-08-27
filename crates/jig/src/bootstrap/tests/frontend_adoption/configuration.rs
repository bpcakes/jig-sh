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
    assert!(
        env_prefix_collision.contains("conflicts with another app after normalization to gate key")
    );
}

#[test]
fn init_toml_escapes_quoted_rust_and_workspace_gate_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("quoted-paths");

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
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            rust_crate_roots: vec!["crates/\"quoted".into()],
            frontend_workspace_roots: vec!["packages/\"shared".into()],
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

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    let config = toml::from_str::<toml::Value>(&rendered).unwrap();
    let gates = config["work"]["gates"].as_array().unwrap();
    let gate_paths = |id: &str| {
        gates
            .iter()
            .find(|gate| gate["id"].as_str() == Some(id))
            .unwrap()["paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap())
            .collect::<Vec<_>>()
    };

    assert!(gate_paths("rust-fmt").contains(&"crates/\"quoted/**"));
    assert!(gate_paths("typescript-web-lint").contains(&"packages/\"shared/**"));
}

#[test]
fn init_rejects_generated_roots_that_are_not_literal_gate_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    for (index, root) in ["crates/pkg{1}", "crates/a**b", "crates/\u{1}name"]
        .into_iter()
        .enumerate()
    {
        let error = run_init(InitOpts {
            path: temp.path().join(format!("invalid-rust-root-{index}")),
            scaffold: ScaffoldOpts::default(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            no_vault: true,
            answers: AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                sqlx_enabled: Some(false),
                rust_crate_roots: vec![root.into()],
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("cannot be represented safely"), "{error:?}");
    }

    let workspace_error = run_init(InitOpts {
        path: temp.path().join("invalid-workspace-root"),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            frontend_workspace_roots: vec!["packages/pkg?".into()],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(
        workspace_error.contains("cannot be represented safely"),
        "{workspace_error:?}"
    );
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
    assert!(!answers.contains("tool = \"jig.typescript_lint\""));
    assert!(!answers.contains("tool = \"jig.typescript_typecheck\""));
    assert!(!answers.contains("tool = \"jig.typescript_build\""));
    assert!(!answers.contains("tool = \"jig.typescript_coverage\""));
    for (gate, tool) in [
        ("typescript-web-lint", "jig.typescript_web_lint"),
        ("typescript-web-typecheck", "jig.typescript_web_typecheck"),
        ("typescript-web-build", "jig.typescript_web_build"),
        ("typescript-web-coverage", "jig.typescript_web_coverage"),
    ] {
        assert!(answers.contains(&format!("id = \"{gate}\"")), "{answers}");
        assert!(answers.contains(&format!("tool = \"{tool}\"")), "{answers}");
    }
    assert!(answers.contains("paths = [\"apps/web/**\", \"packages/**\""));
    assert!(answers.contains("id = \"jig-contract\""));
    assert!(!answers.contains("id = \"contract\""));
    assert!(answers.contains("[[dev.apps]]"));
    assert!(answers.contains(
        "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]"
    ));
    assert!(!answers.contains("dev_command"));

    assert!(!repo.join("Makefile").exists());
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let web_node = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    let web_sources = format!("{web_check}\n{web_node}");
    assert!(web_sources.contains("run_managed_npm_command install"));
    assert!(web_sources.contains("run_managed_npm_command run-script"));
    assert!(web_sources.contains("--location=project"));
    assert!(
        web_check
            .contains("if [ -f npm-shrinkwrap.json ]; then printf '%s\\n' \"npm-shrinkwrap.json\"")
    );
    assert!(web_sources.contains("dependencies_present"));
    assert!(web_sources.contains("dependency_fingerprint"));
    assert!(web_sources.contains("root.sha256"));
    assert!(web_sources.contains("web-dependencies.lock"));
    assert!(web_sources.contains("scripts/check-webapp-scripts.mjs"));
    assert!(web_sources.contains("scripts/enforce-coverage.cjs"));
    assert!(web_check.contains("app-check <app-dir> <lint|typecheck|build|coverage>"));
    assert!(web_check.contains("\"apps/web\") run_check \"$app_dir\" \"80\" \"$script_name\""));
    assert!(web_check.contains("scripts/web-node.cjs"));
    assert!(!web_check.contains("--jig-workspace-metadata \"$operation\" \"$@\" <<'NODE'"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains("\"typescript_lint_command\""));
    assert!(contract.contains(r#""name": "jig.typescript_lint""#));
    assert!(contract.contains(r#""name": "jig.typescript_typecheck""#));
    assert!(contract.contains(r#""name": "jig.typescript_build""#));
    assert!(contract.contains(r#""name": "jig.typescript_coverage""#));
    assert!(contract.contains(r#""contract_version": 5"#));
    assert!(contract.contains(r#""name": "jig.typescript_web_lint""#));
    assert!(contract.contains(r#""name": "jig.typescript_web_typecheck""#));
    assert!(contract.contains(r#""name": "jig.typescript_web_build""#));
    assert!(contract.contains(r#""name": "jig.typescript_web_coverage""#));
    assert!(repo.join("scripts/check-webapp-scripts.mjs").is_file());
    let script_helper = fs::read_to_string(repo.join("scripts/check-webapp-scripts.mjs")).unwrap();
    assert!(script_helper.contains("typeof command !== \"string\""));
    assert!(script_helper.contains("command.trim().length === 0"));
    let workspace_helper = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    assert!(workspace_helper.contains("unknown workspace metadata operation"));

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
fn adoption_persists_non_app_workspace_ownership_and_honors_exclusions() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::create_dir_all(repo.join("libs/shared")).unwrap();
    fs::create_dir_all(repo.join("libs/private")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*","libs/*","!libs/private"]}"#,
    )
    .unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name":"web",
  "scripts":{
    "dev":"vite",
    "lint":"eslint .",
    "typecheck":"tsc --noEmit",
    "build:bundle":"vite build",
    "test:coverage":"vitest run --coverage"
  }
}"#,
    )
    .unwrap();
    fs::write(
        repo.join("libs/shared/package.json"),
        r#"{"name":"shared","exports":"./index.js"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("libs/private/package.json"),
        r#"{"name":"private-fixture"}"#,
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
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(
        output["adoption_profile"]["frontend_workspace_roots"],
        serde_json::json!(["libs/shared"])
    );
    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(
        config.contains("frontend_workspace_roots = [\"libs/shared\"]"),
        "{config}"
    );
    assert!(config.contains("\"libs/shared/**\""), "{config}");
    assert!(!config.contains("libs/private/**"), "{config}");

    fs::write(
        repo.join("package.json"),
        r#"{"private":true,"workspaces":["apps/*","libs/private"]}"#,
    )
    .unwrap();
    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: true,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    let refreshed = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(
        refreshed.contains("frontend_workspace_roots = [\"libs/private\"]"),
        "{refreshed}"
    );
    assert!(refreshed.contains("\"libs/private/**\""), "{refreshed}");
    assert!(!refreshed.contains("libs/shared/**"), "{refreshed}");
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
    assert_eq!(resolve_spec(), "yarn@4.18.0");

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
    assert!(answers.contains("jig.typescript_web_lint"));

    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains("typescript_lint_command"));
    assert!(contract.contains("jig.typescript_lint"));

    let agent_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(agent_guide.contains("scripts/jig check typescript-lint"));
    assert!(!agent_guide.contains("make ci-webapps"));
}

include!("configuration_parts/part_01.rs");
