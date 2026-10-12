use super::*;

#[test]
fn scaffold_options_require_preset() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: None,
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold options require --preset rust-react"));
}

#[test]
fn go_module_rejects_non_go_presets_with_stable_errors() {
    for (preset, expected) in [
        (None, "--go-module requires --preset go-react"),
        (
            Some(ScaffoldPreset::RustReact),
            "--go-module requires --preset go-react",
        ),
        (
            Some(ScaffoldPreset::HarnessOnly),
            "--preset harness-only cannot be combined with --db, --go-module, --frontend, --frontends, --metrics, or --jobs",
        ),
    ] {
        let error = ScaffoldOpts {
            preset,
            ..ScaffoldOpts::default()
        }
        .validate_init_invariants(&AnswerOpts {
            go_module: Some("example.com/ExampleProject".into()),
            ..AnswerOpts::default()
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains(expected), "{preset:?}: {error}");
    }
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
                metrics: None,
                jobs: None,
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
                metrics: None,
                jobs: None,
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
fn reserved_backend_dev_identity_is_scoped_to_application_presets() {
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

    let error = ScaffoldOpts {
        preset: Some(ScaffoldPreset::GoReact),
        frontends: vec![parse_scaffold_frontend("api:spa").unwrap()],
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts {
        go_module: Some("example.com/ExampleProject".into()),
        ..AnswerOpts::default()
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("go-react frontend app name 'api'"));
    assert!(error.contains("reserved backend dev app 'api'"));

    ScaffoldOpts {
        preset: Some(ScaffoldPreset::RustReact),
        frontends: vec![parse_scaffold_frontend("api-client:spa").unwrap()],
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts::default())
    .unwrap();
}

#[test]
fn application_presets_reject_conflicting_backend_identity_answers() {
    for (preset, backend_language, expected_message) in [
        (
            ScaffoldPreset::RustReact,
            BackendLanguage::Go,
            "--preset rust-react generates a rust backend",
        ),
        (
            ScaffoldPreset::GoReact,
            BackendLanguage::Rust,
            "--preset go-react generates a go backend",
        ),
    ] {
        let error = ScaffoldOpts {
            preset: Some(preset),
            ..ScaffoldOpts::default()
        }
        .validate_init_invariants(&AnswerOpts {
            backend_language: Some(backend_language),
            ..AnswerOpts::default()
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected_message), "{error}");
        assert!(error.contains("select a matching preset"), "{error}");
    }

    ScaffoldOpts {
        preset: Some(ScaffoldPreset::HarnessOnly),
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts {
        backend_language: Some(BackendLanguage::Go),
        ..AnswerOpts::default()
    })
    .unwrap();
}

#[test]
fn run_init_rejects_conflicting_backend_identity_before_destination_writes() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(&answers_file, "backend_language = \"go\"\n").unwrap();
    let destination = temp.path().join("ExampleProject");

    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("--preset rust-react generates a rust backend"));
    assert!(!destination.exists());
}

#[test]
fn rust_react_reserves_the_separate_admin_backend_identity() {
    let opts = ScaffoldOpts {
        preset: Some(ScaffoldPreset::RustReact),
        frontends: vec![parse_scaffold_frontend("admin_api:spa").unwrap()],
        ..ScaffoldOpts::default()
    };

    let error = opts
        .validate_init_invariants(&AnswerOpts::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("reserved backend dev app 'admin-api'"));
    assert!(error.contains("JIG_DEV_ADMIN_API"));
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
            metrics: None,
            jobs: None,
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

#[test]
fn run_init_rejects_frontend_names_that_cannot_become_component_ids_before_writes() {
    for supplied_name in ["_web", "web-"] {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repo");

        let error = run_init(InitOpts {
            path: destination.clone(),
            scaffold: ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: None,
                frontends: vec![ScaffoldFrontend {
                    name: supplied_name.into(),
                    kind: ScaffoldFrontendKind::Spa,
                    custom_default_name: false,
                }],
                frontend_list: Vec::new(),
                metrics: None,
                jobs: None,
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

        assert!(error.contains("Invalid frontend app name"), "{error}");
        assert!(!destination.exists());
    }
}
