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
        "apps/web/package.json",
        "apps/web/.gitignore",
        "apps/web/playwright.config.ts",
        "apps/web/e2e/app.spec.ts",
        "apps/web/components.json",
        "apps/web/src/App.tsx",
        "apps/web/src/api.ts",
        "apps/web/src/app/router.ts",
        "apps/web/src/routeTree.gen.ts",
        "apps/web/src/routes/index.tsx",
        "apps/web/src/components/ui/button.tsx",
        "apps/web/src/lib/utils.ts",
        "apps/landing/package.json",
        "apps/landing/src/pages/index.astro",
        "apps/admin-panel/package.json",
        "apps/admin-panel/components.json",
        "apps/admin-panel/src/app/router.ts",
        "apps/admin-panel/src/routeTree.gen.ts",
        "apps/admin-panel/src/routes/index.tsx",
        "apps/admin-panel/src/routes/settings.tsx",
        "apps/admin-panel/src/components/ui/sidebar.tsx",
        "apps/admin-panel/src/features/overview/overview-page.tsx",
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
