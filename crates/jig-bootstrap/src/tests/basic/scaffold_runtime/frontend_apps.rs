use super::*;

#[test]
fn scaffold_uses_explicit_frontend_role_without_name_inference() {
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
            metrics: None,
            jobs: None,
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
    for dir in [
        "apps",
        "apps/demo-api",
        "apps/demo-api/ui",
        "apps/demo-admin-api",
    ] {
        let error = scaffold::InitScaffoldPlan::from_opts(
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
                    dir: dir.into(),
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

        assert!(error.contains(&format!("uses reserved directory '{dir}'")));
    }
}

#[test]
fn go_scaffold_places_backend_named_frontends_under_apps() {
    for dir in ["cmd", "internal"] {
        let temp = tempdir().unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::GoReact),
                db: Some(ScaffoldDb::None),
                frontends: vec![parse_scaffold_frontend(dir).unwrap()],
                frontend_list: Vec::new(),
                metrics: None,
                jobs: None,
            },
            &AnswerOpts {
                go_module: Some("example.com/example-project".into()),
                ..AnswerOpts::default()
            },
            temp.path(),
        )
        .unwrap()
        .unwrap();

        let report = plan.write(temp.path(), false).unwrap();
        assert_eq!(report["frontends"][0]["dir"], format!("apps/{dir}"));
    }
}

#[test]
fn go_scaffold_rejects_answer_frontends_under_backend_roots() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts {
            go_module: Some("example.com/example-project".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "internal/web".into(),
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

    assert!(error.contains("uses reserved directory 'internal/web'"));
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
