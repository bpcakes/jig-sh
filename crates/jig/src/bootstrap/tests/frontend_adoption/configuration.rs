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

fn assert_npm_adoption_report(output: &serde_json::Value) {
    assert_json_array_contains(&output["notes"], "scripts/jig check agent-guides");
    for command in [
        "scripts/jig check typescript-lint",
        "typescript-typecheck",
        "typescript-build",
        "typescript-coverage",
    ] {
        assert_json_array_contains(&output["notes"], command);
    }
    assert_json_array_contains(&output["render_report"]["files_created"], "scripts/jig");
    assert_json_array_contains(
        &output["adoption_profile"]["managed_files"],
        ".github/workflows/webapp-checks.yml",
    );
    assert_json_array_contains_none(
        &output["adoption_profile"]["retired_managed_files"],
        ".github/workflows/webapp-checks.yml",
    );
    assert_json_array_contains(&output["render_report"]["todos"], "frontend app");
}

fn assert_npm_adoption_answers(repo: &Path) {
    assert!(!repo.join("crates/api/AGENTS.md").exists());
    assert!(!repo.join("Makefile").exists());
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert_text_contains_all(
        &answers,
        &[
            "web_package_manager = \"npm\"",
            "[[frontend_apps]]",
            "[commands]",
            "web_lint_command = \"scripts/check-webapps.sh check-one",
            "repo_compat_typescript_lint_command = \"scripts/check-webapps.sh lint\"",
            "kind = \"evidence\"",
            "profile = \"verify\"",
            "[[dev.apps]]",
            "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]",
        ],
    );
    assert_text_contains_none(
        &answers,
        &[
            "tool = \"jig.typescript_lint\"",
            "frontend-contract-drift",
            "frontend-public-boundary",
            "dev_command",
        ],
    );
}

fn assert_contract_runner_failure(repo: &Path, app: &str, expected_error: &str) {
    let output = std::process::Command::new("bash")
        .args(["scripts/check-webapps.sh", "contracts-drift-check", app])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_text_contains_all(&String::from_utf8_lossy(&output.stderr), &[expected_error]);
}

fn assert_npm_adoption_web_helpers(repo: &Path) {
    let web_check = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    assert_text_contains_all(
        &web_check,
        &[
            "app-check)",
            "application-contracts)",
            "public-artifacts)",
            "if [ -f npm-shrinkwrap.json ]; then printf '%s\\n' \"npm-shrinkwrap.json\"",
            "scripts/web-node.cjs",
        ],
    );
    assert_text_contains_none(
        &web_check,
        &["--jig-workspace-metadata \"$operation\" \"$@\" <<'NODE'"],
    );
    assert!(!repo.join("scripts/contracts.mjs").exists());
    assert_contract_runner_failure(
        repo,
        "apps/web",
        "Required frontend contract runner scripts/contracts.mjs is missing",
    );
    assert_contract_runner_failure(
        repo,
        "apps/not-configured",
        "is not configured in [[frontend_apps]]",
    );
    let web_node = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    let web_sources = format!("{web_check}\n{web_node}");
    assert_text_contains_all(
        &web_sources,
        &[
            "run_managed_npm_command install",
            "run_managed_npm_command run-script",
            "--location=project",
            "dependencies_present",
            "dependency_fingerprint",
            "root.sha256",
            "web-dependencies.lock",
            "scripts/check-webapp-scripts.mjs",
            "scripts/enforce-coverage.cjs",
        ],
    );
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert_text_contains_all(
        &contract,
        &[
            "\"web_lint_command\"",
            r#""name": "jig.typescript_lint""#,
            r#""name": "jig.typescript_typecheck""#,
            r#""name": "jig.typescript_build""#,
            r#""name": "jig.typescript_coverage""#,
        ],
    );
    assert!(repo.join("scripts/check-webapp-scripts.mjs").is_file());
    assert_text_contains_all(
        &fs::read_to_string(repo.join("scripts/check-webapp-scripts.mjs")).unwrap(),
        &[
            "typeof command !== \"string\"",
            "command.trim().length === 0",
        ],
    );
    assert_text_contains_all(&web_node, &["unknown workspace metadata operation"]);
}

fn assert_npm_adoption_workflows(repo: &Path) {
    let web = fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert_text_contains_all(
        &web,
        &[
            "actions/setup-node@v5",
            "cache: npm",
            "${{ matrix.app.dir }}/npm-shrinkwrap.json",
            r#"scripts/check-webapps.sh dependencies-install "$APP_DIR""#,
            "node scripts/check-webapp-scripts.mjs",
            "node scripts/enforce-coverage.cjs",
        ],
    );
    assert_text_contains_none(
        &web,
        &[
            "Check generated API clients and public boundary",
            "node scripts/contracts.mjs client-check",
            "make enforce-coverage",
            "oven-sh/setup-bun",
        ],
    );
    assert_eq!(web.matches(r#"- "npm-shrinkwrap.json""#).count(), 2);
    let rust = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    assert_text_contains_all(&rust, &["scripts/jig check api:fmt"]);
    assert_text_contains_none(&rust, &["scripts/jig fmt-check"]);
    assert_eq!(rust.matches(r#"- "rust-toolchain""#).count(), 2);
    let agent_map = fs::read_to_string(repo.join(".github/workflows/agent-map-check.yml")).unwrap();
    assert_text_contains_all(&agent_map, &["scripts/jig check agent-map"]);
    assert_text_contains_none(&agent_map, &["scripts/jig agent-map check"]);
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

    assert_npm_adoption_report(&output);
    assert_npm_adoption_answers(&repo);
    assert_npm_adoption_web_helpers(&repo);
    assert_npm_adoption_workflows(&repo);
}

mod package_managers;
