
#[test]
fn run_init_rust_react_scaffold_omits_admin_contract_without_admin_frontend() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("public-app");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(destination.join("openapi/public.json").exists());
    assert!(
        destination
            .join("packages/public-api-client/src/generated/sdk.gen.ts")
            .exists()
    );
    assert!(!destination.join("openapi/admin.json").exists());
    assert!(!destination.join("crates/public-app-admin-http").exists());
    assert!(!destination.join("apps/public-app-admin-api").exists());
    assert!(!destination.join("packages/admin-api-client").exists());

    let workspace_cargo = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(workspace_cargo.contains("rust-version = \"1.94\""));
    assert!(!workspace_cargo.contains("sqlx ="));
    let root_readme = fs::read_to_string(destination.join("README.md")).unwrap();
    assert!(root_readme.contains("Prerequisites: Rust 1.94 or newer"));

    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packages/public-api-client""#));
    assert!(!workspace_package.contains(r#""packages/admin-api-client""#));
    let exporter =
        fs::read_to_string(destination.join("apps/public-app-api/src/bin/export-openapi.rs"))
            .unwrap();
    assert!(exporter.contains("public_openapi"));
    assert!(!exporter.contains("admin"));
    let public_api_manifest =
        fs::read_to_string(destination.join("apps/public-app-api/Cargo.toml")).unwrap();
    assert!(!public_api_manifest.contains("admin"));
    let contracts = fs::read_to_string(destination.join("scripts/contracts.mjs")).unwrap();
    assert!(!contracts.contains("cargoPackage: \"public-app-admin-api\""));
    assert!(!contracts.contains("name: \"admin\""));
}

#[cfg(unix)]
#[test]
fn generated_dependency_failure_names_the_exact_bootstrap_recovery_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("dependency-recovery");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let package_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(destination.join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_json["packageManager"], "npm@12.0.2");
    assert_eq!(package_json["allowScripts"]["esbuild@0.28.2"], true);

    let output = Command::new("bash")
        .args(["scripts/check-webapps.sh", "dependencies-install", "web"])
        .current_dir(&destination)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing package-lock.json"), "{stderr}");
    assert!(
        stderr.contains("Run 'scripts/check-webapps.sh bootstrap' from the repository root"),
        "{stderr}"
    );
}

// Repeat the dependency-backed proof with:
// cargo test -p jig-sh bootstrap::tests::basic::scaffold_generation::generated_spa_coverage_counts_uncovered_future_production_modules -- --ignored --exact --nocapture
#[cfg(unix)]
#[test]
#[ignore = "requires npm registry access and a local Node/npm toolchain"]
fn generated_spa_coverage_counts_uncovered_future_production_modules() {
    use std::fmt::Write as _;

    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("coverage-proof");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let dependencies = Command::new("scripts/check-webapps.sh")
        .args(["dependencies-bootstrap", "web"])
        .env("NODE_ENV", "production")
        .env("NPM_CONFIG_OMIT", "dev")
        .current_dir(&destination)
        .output()
        .unwrap();
    assert!(
        dependencies.status.success(),
        "generated dependency bootstrap could not prepare the coverage fixture:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dependencies.stdout),
        String::from_utf8_lossy(&dependencies.stderr)
    );

    let run_coverage = || {
        Command::new("scripts/check-webapps.sh")
            .arg("coverage")
            .current_dir(&destination)
            .output()
            .unwrap()
    };
    let statement_coverage = || {
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(destination.join("web/coverage/coverage-summary.json")).unwrap(),
        )
        .unwrap()["total"]["statements"]["pct"]
            .as_f64()
            .unwrap()
    };

    let baseline = run_coverage();
    assert!(
        baseline.status.success(),
        "generated SPA coverage baseline failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&baseline.stdout),
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baseline_statements = statement_coverage();
    assert!(baseline_statements >= 80.0);

    let mut uncovered_module = String::new();
    for index in 0..20 {
        write!(
            uncovered_module,
            "export function uncovered{index}(value: number): number {{\n  const shifted = value + {index};\n  const doubled = shifted * 2;\n  return doubled > 10 ? doubled : 10;\n}}\n\n"
        )
        .unwrap();
    }
    fs::write(
        destination.join("web/src/uncovered-production.ts"),
        uncovered_module,
    )
    .unwrap();

    let negative = run_coverage();
    assert!(!negative.status.success());
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&negative.stdout),
        String::from_utf8_lossy(&negative.stderr)
    );
    assert!(diagnostics.contains("Coverage below threshold 80%"));
    let uncovered_statements = statement_coverage();
    assert!(
        uncovered_statements < 80.0,
        "future production module stayed outside the coverage denominator: baseline {baseline_statements}%, after addition {uncovered_statements}%"
    );
}

#[test]
fn rust_react_admin_dynamic_values_use_formatter_stable_boundaries() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo_name = "r".repeat(120);
    let frontend_name = format!("admin-{}", "x".repeat(100));
    let destination = temp.path().join(&repo_name);

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend(&format!("{frontend_name}:admin")).unwrap(),
            ],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let admin = destination.join(&frontend_name);
    let theme_storage_key = format!("{frontend_name}-theme");
    let index = fs::read_to_string(admin.join("index.html")).unwrap();
    assert!(index.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(index.matches(&theme_storage_key).count(), 1);
    assert!(index.contains("localStorage.getItem(themeStorageKey)"));
    assert!(index.contains("<!-- prettier-ignore -->\n    <title>"));

    let providers = fs::read_to_string(admin.join("src/app/providers.tsx")).unwrap();
    assert!(providers.contains(&format!("const themeStorageKey = \"{theme_storage_key}\"")));
    assert_eq!(providers.matches(&theme_storage_key).count(), 1);
    assert!(providers.contains("storageKey={themeStorageKey}"));

    let shell = fs::read_to_string(admin.join("src/app/shell.tsx")).unwrap();
    assert!(shell.contains("const appTitle = \""));
    assert!(shell.contains(">{appTitle}</p>"));

    let sidebar = fs::read_to_string(admin.join("src/components/app-sidebar.tsx")).unwrap();
    assert!(sidebar.contains(&format!("const appName = \"{repo_name}\"")));
    assert_eq!(sidebar.matches(&repo_name).count(), 1);
    assert!(sidebar.contains(">{appName}</span>"));

    let overview_test =
        fs::read_to_string(admin.join("src/features/overview/overview-page.test.tsx")).unwrap();
    assert!(overview_test.contains(&format!("const expectedAppName = \"{repo_name}\"")));
    assert_eq!(overview_test.matches(&repo_name).count(), 1);
    assert!(overview_test.contains("name: expectedAppName"));
    assert!(overview_test.contains("screen.findAllByText(expectedAppName)"));
}

#[test]
fn scaffold_options_require_preset() {
    let temp = tempdir().unwrap();
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: None,
            db: Some(ScaffoldDb::Sqlite),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts::default(),
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Scaffold options require --preset rust-react"));
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
fn reserved_backend_dev_identity_is_scoped_to_rust_react() {
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

    ScaffoldOpts {
        preset: Some(ScaffoldPreset::RustReact),
        frontends: vec![parse_scaffold_frontend("api-client:spa").unwrap()],
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts::default())
    .unwrap();
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

#[cfg(unix)]
fn assert_rendered_scaffold_rust_is_formatted(plan: &scaffold::InitScaffoldPlan, case: &str) {
    let rendered = plan.render_files().unwrap();
    let temp = tempdir().unwrap();
    let mut rust_paths = Vec::new();

    for (index, file) in rendered
        .into_iter()
        .filter(|file| file.relative.ends_with(".rs"))
        .enumerate()
    {
        let path = temp.path().join(format!("rendered-{index}.rs"));
        fs::write(&path, file.contents).unwrap();
        rust_paths.push(path);
    }

    assert!(!rust_paths.is_empty(), "{case}: scaffold rendered no Rust");
    let output = Command::new("rustfmt")
        .args([
            "--edition",
            "2024",
            "--check",
            "--config",
            "skip_children=true",
        ])
        .args(&rust_paths)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rendered Rust was not rustfmt-stable for {case}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn rust_react_package_stem_limit_is_applied_before_destination_mutation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();

    let accepted_name = "r".repeat(216);
    let accepted_destination = temp.path().join("accepted");
    fs::create_dir(&accepted_destination).unwrap();
    let accepted_plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        &AnswerOpts {
            repo_name: Some(accepted_name.clone()),
            ..AnswerOpts::default()
        },
        &accepted_destination,
    )
    .unwrap()
    .unwrap();
    accepted_plan.write(&accepted_destination, false).unwrap();

    assert!(
        accepted_destination
            .join(format!("crates/{accepted_name}-test-support/Cargo.toml"))
            .is_file()
    );
    let vite_config = fs::read_to_string(accepted_destination.join("web/vite.config.ts")).unwrap();
    let repo_label = vite_config
        .split_once("http://api.")
        .unwrap()
        .1
        .split_once(".localhost:1355")
        .unwrap()
        .0;
    assert_eq!(repo_label.len(), 63);
    assert_eq!(repo_label, "r".repeat(63));

    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&accepted_destination)
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "maximum supported scaffold has invalid Cargo metadata\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&metadata.stdout),
        String::from_utf8_lossy(&metadata.stderr)
    );

    let rejected_destination = temp.path().join("rejected");
    let error = run_init(InitOpts {
        path: rejected_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(materialize_template_worktree().path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("r".repeat(217)),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("217-byte Cargo package stem"), "{error}");
    assert!(error.contains("at most 216 bytes"), "{error}");
    assert!(
        error.contains("lib<stem>_test_support-<hash>.rmeta"),
        "{error}"
    );
    assert!(!rejected_destination.exists());
}
