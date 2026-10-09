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
        (ScaffoldDb::Postgres, ScaffoldMetrics::None, ScaffoldJobs::None),
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
