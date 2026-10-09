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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
