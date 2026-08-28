use super::*;

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
