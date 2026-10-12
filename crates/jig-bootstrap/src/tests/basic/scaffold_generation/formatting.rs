use super::*;

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
        .current_dir(temp.path())
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
fn assert_rendered_scaffold_go_is_formatted_and_config_is_runnable(
    plan: &scaffold::InitScaffoldPlan,
    case: &str,
) {
    if !test_program_is_available("gofmt", &["-h"])
        || !test_program_is_available("go", &["version"])
    {
        assert_ne!(
            std::env::var("JIG_REQUIRE_GO_TOOLCHAIN").as_deref(),
            Ok("1"),
            "{case}: JIG_REQUIRE_GO_TOOLCHAIN=1 but go/gofmt is unavailable"
        );
        return;
    }

    let rendered = plan.render_files().unwrap();
    let temp = tempdir().unwrap();
    let mut go_paths = Vec::new();
    let mut config_source = None;
    let mut config_test_source = None;
    for (index, file) in rendered
        .into_iter()
        .filter(|file| file.relative.ends_with(".go"))
        .enumerate()
    {
        if file.relative == "internal/config/config.go" {
            config_source = Some(file.contents.clone());
        } else if file.relative == "internal/config/config_test.go" {
            config_test_source = Some(file.contents.clone());
        }
        let path = temp.path().join(format!("rendered-{index}.go"));
        fs::write(&path, file.contents).unwrap();
        go_paths.push(path);
    }

    assert!(!go_paths.is_empty(), "{case}: scaffold rendered no Go");
    let output = Command::new("gofmt")
        .arg("-l")
        .args(&go_paths)
        .output()
        .unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty(),
        "rendered Go was not gofmt-stable for {case}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let config_dir = temp.path().join("config");
    fs::create_dir(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.go"),
        config_source.expect("Go scaffold renders internal/config/config.go"),
    )
    .unwrap();
    fs::write(
        config_dir.join("config_test.go"),
        config_test_source.expect("Go scaffold renders internal/config/config_test.go"),
    )
    .unwrap();
    let output = Command::new("go")
        .args(["test", "config.go", "config_test.go"])
        .current_dir(&config_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rendered Go config did not compile and pass its address cases for {case}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new("go")
        .args(["vet", "config.go", "config_test.go"])
        .current_dir(&config_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rendered Go config did not pass go vet for {case}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn scaffold_rendered_go_is_formatted_and_config_handles_ip_hosts() {
    let planning_root = tempdir().unwrap();
    for db in [ScaffoldDb::None, ScaffoldDb::Postgres] {
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::GoReact),
                db: Some(db),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
                metrics: None,
                jobs: None,
            },
            &AnswerOpts {
                repo_name: Some("demo".into()),
                go_module: Some("example.com/ExampleProject".into()),
                ..AnswerOpts::default()
            },
            planning_root.path(),
        )
        .unwrap()
        .unwrap();

        assert_rendered_scaffold_go_is_formatted_and_config_is_runnable(
            &plan,
            match db {
                ScaffoldDb::None => "none",
                ScaffoldDb::Postgres => "postgres",
            },
        );
    }
}

#[cfg(unix)]
#[test]
fn scaffold_rendered_rust_is_formatted_across_names_databases_and_migration_paths() {
    let planning_root = tempdir().unwrap();
    let names = [
        ("manual-qa", "node22-npm12-widget-web".to_string()),
        ("width-40", format!("r{}", "a".repeat(39))),
        ("width-52", format!("r{}", "a".repeat(51))),
        ("width-71", format!("r{}", "a".repeat(70))),
        ("supported-max-216", format!("r{}", "a".repeat(215))),
    ];
    for (label, name) in &names {
        let expected_len = match *label {
            "manual-qa" => 23,
            "width-40" => 40,
            "width-52" => 52,
            "width-71" => 71,
            "supported-max-216" => 216,
            _ => unreachable!(),
        };
        assert_eq!(name.len(), expected_len, "{label}");
    }

    // Cover base shapes, each service option alone, and the combined services.
    let shapes = [
        (ScaffoldDb::None, ScaffoldMetrics::None, ScaffoldJobs::None),
        (ScaffoldDb::None, ScaffoldMetrics::Otlp, ScaffoldJobs::None),
        (
            ScaffoldDb::Postgres,
            ScaffoldMetrics::None,
            ScaffoldJobs::None,
        ),
        (
            ScaffoldDb::Postgres,
            ScaffoldMetrics::None,
            ScaffoldJobs::Runledger,
        ),
        (
            ScaffoldDb::Postgres,
            ScaffoldMetrics::Otlp,
            ScaffoldJobs::Runledger,
        ),
    ];
    for (db, metrics, jobs) in shapes {
        let db_label = match db {
            ScaffoldDb::None => "none",
            ScaffoldDb::Postgres => "postgres",
        };
        for (name_label, repo_name) in &names {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: vec![
                        ScaffoldFrontend {
                            name: "web".into(),
                            kind: ScaffoldFrontendKind::Spa,
                            custom_default_name: false,
                        },
                        ScaffoldFrontend {
                            name: "admin".into(),
                            kind: ScaffoldFrontendKind::Admin,
                            custom_default_name: false,
                        },
                    ],
                    frontend_list: Vec::new(),
                    metrics: Some(metrics),
                    jobs: Some(jobs),
                },
                &AnswerOpts {
                    repo_name: Some(repo_name.clone()),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(
                &plan,
                &format!(
                    "{db_label}/{}/{}/{name_label}",
                    metrics.as_str(),
                    jobs.as_str()
                ),
            );
        }
    }

    for db in [ScaffoldDb::Postgres] {
        let db_label = "postgres";
        for migration_len in [13, 80, 216] {
            let plan = scaffold::InitScaffoldPlan::from_opts(
                &ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(db),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                    metrics: None,
                    jobs: None,
                },
                &AnswerOpts {
                    repo_name: Some("demo".into()),
                    rust_migration_dir: Some("m".repeat(migration_len)),
                    ..AnswerOpts::default()
                },
                planning_root.path(),
            )
            .unwrap()
            .unwrap();
            assert_rendered_scaffold_rust_is_formatted(
                &plan,
                &format!("{db_label}/migration-width-{migration_len}"),
            );
        }
    }
}
