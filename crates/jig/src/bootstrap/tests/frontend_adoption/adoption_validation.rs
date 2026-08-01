use super::*;

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
