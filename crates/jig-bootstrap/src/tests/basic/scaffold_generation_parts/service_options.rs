fn rust_react_service_plan(
    destination: &Path,
    repo_name: &str,
    db: ScaffoldDb,
    metrics: Option<ScaffoldMetrics>,
    jobs: Option<ScaffoldJobs>,
) -> anyhow::Result<Option<scaffold::InitScaffoldPlan>> {
    scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(db),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
            metrics,
            jobs,
        },
        &AnswerOpts {
            repo_name: Some(repo_name.into()),
            ..AnswerOpts::default()
        },
        destination,
    )
}

#[test]
fn rust_react_service_options_add_metrics_export_and_a_runledger_worker() {
    let temp = tempdir().unwrap();
    let plan = rust_react_service_plan(
        temp.path(),
        "my-app",
        ScaffoldDb::Postgres,
        Some(ScaffoldMetrics::Otlp),
        Some(ScaffoldJobs::Runledger),
    )
    .unwrap()
    .unwrap();
    let report = plan.write(temp.path(), false).unwrap();
    assert_eq!(report["metrics"], "otlp");
    assert_eq!(report["jobs"], "runledger");
    let root = temp.path();
    assert_paths_exist(
        root,
        &[
            "crates/my-app-jobs/Cargo.toml",
            "crates/my-app-jobs/AGENTS.md",
            "crates/my-app-jobs/src/lib.rs",
            "crates/my-app-runtime/src/metrics.rs",
        ],
    );

    let workspace = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_contains_all(
        &workspace,
        &[
            r#"features = ["axum", "sqlx", "otlp", "runledger"] }"#,
            r#""crates/my-app-jobs","#,
            r#"uuid = { version = "1", features = ["v7"] }"#,
            "postgres-test-harness = { git =",
        ],
    );
    let runtime_cargo = fs::read_to_string(root.join("crates/my-app-runtime/Cargo.toml")).unwrap();
    assert_contains_all(
        &runtime_cargo,
        &[
            r#"my-app-jobs = { path = "../my-app-jobs", optional = true }"#,
            r#"db = ["my-app/db", "dep:my-app-jobs"]"#,
            r#"my-app-core = { path = "../my-app-core" }"#,
        ],
    );
    let runtime = fs::read_to_string(root.join("crates/my-app-runtime/src/lib.rs")).unwrap();
    assert_contains_all(
        &runtime,
        &[
            "mod metrics;",
            "let diagnostics = metrics::prepare(config.metrics_endpoint())?;",
            "let completion = service::start(startup, diagnostics).wait().await;",
            "metrics::report(completion.diagnostics());",
            "pub async fn serve_with_jobs<F, Fut>(",
            "run(config, true, assemble).await",
            "run(config, false, assemble).await",
            "jobs_crate::register(scope, &db, self.config.jobs_worker_id(), context).await?;",
        ],
    );
    let metrics = fs::read_to_string(root.join("crates/my-app-runtime/src/metrics.rs")).unwrap();
    assert_contains_all(
        &metrics,
        &[
            "otlp::prepare(endpoint, service_name(), schedule)?",
            "Schedule::new(EXPORT_INTERVAL, EXPORT_ATTEMPT, FINAL_EXPORT)?",
            "impl Diagnostics for MetricsDiagnostics",
            "absent_endpoint_installs_nothing",
        ],
    );
    let api_main = fs::read_to_string(root.join("apps/my-app-api/src/main.rs")).unwrap();
    assert_contains_all(
        &api_main,
        &[
            "runtime::serve_with_jobs(config, app_http_crate::assemble).await",
            "\",runledger=info\",",
        ],
    );
    let admin_main = fs::read_to_string(root.join("apps/my-app-admin-api/src/main.rs")).unwrap();
    assert!(admin_main.contains("runtime::serve(config, |state, admission, readiness| {"));
    assert!(!admin_main.contains("serve_with_jobs"));

    let jobs = fs::read_to_string(root.join("crates/my-app-jobs/src/lib.rs")).unwrap();
    assert_contains_all(
        &jobs,
        &[
            "pub const EXAMPLE_JOB: JobType<'static> = JobType::new(concat!(",
            "\"my_app\",",
            "impl JobHandler for ExampleJob",
            "JobCatalog::new().handler(ExampleJob)",
            "format!(\"{APP_NAME}-worker-{}\", uuid::Uuid::now_v7())",
            "batter::runledger::register_in(target, \"jobs\", startup.clone(), prepared)?;",
            "run_atomic(",
            "pub async fn enqueue_example(",
        ],
    );
    let db = fs::read_to_string(root.join("crates/my-app-db/src/lib.rs")).unwrap();
    assert_contains_all(
        &db,
        &[
            "use batter::runledger::{PgSessionProfile, RunledgerDatabase, native::postgres as runledger};",
            "database: RunledgerDatabase,",
            "PgSessionProfile::with_timeouts(",
            "runledger::migrate_after_idempotency_cutover(&self.database)",
            "lease.migrate(&application)",
            "runledger::ensure_schema_compatible_after_idempotency_cutover(&self.database)",
            "migrator.set_ignore_missing(true);",
        ],
    );
    assert!(!db.contains("batter::sqlx::pool_in"));
    let config = fs::read_to_string(root.join("crates/my-app/src/config.rs")).unwrap();
    assert_contains_all(
        &config,
        &[
            "metrics_endpoint: present(source.text(\"METRICS_OTLP_ENDPOINT\")?).map(str::to_owned),",
            "jobs_worker_id: present(source.text(\"JOBS_WORKER_ID\")?).map(str::to_owned),",
        ],
    );
    let test_support_db =
        fs::read_to_string(root.join("crates/my-app-test-support/src/db.rs")).unwrap();
    assert!(
        test_support_db.contains("(\"runledger\", &batter::runledger::native::postgres::MIGRATOR),")
    );
    let postgres_test =
        fs::read_to_string(root.join("crates/my-app-test-support/tests/postgres.rs")).unwrap();
    assert_contains_all(
        &postgres_test,
        &[
            "example_job_runs_under_the_supervised_worker",
            "jobs_crate::enqueue_example(&db, \"hello from the worker test\", \"worker-test-1\")",
            "wait_for_job(&db, \"worker-test-1\", \"SUCCEEDED\")",
        ],
    );
    let env_example = fs::read_to_string(root.join(".env.example")).unwrap();
    assert_contains_all(
        &env_example,
        &[
            ",batter=info,batter_axum=info,runledger=info\nDATABASE_URL=",
            "# METRICS_OTLP_ENDPOINT=http://127.0.0.1:4318/v1/metrics\n",
            "# JOBS_WORKER_ID=\n",
        ],
    );
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    assert_contains_all(
        &readme,
        &[
            "`axum`, `sqlx`, `otlp`, `runledger`.",
            "### Metrics export",
            "METRICS_OTLP_ENDPOINT",
            "### Background jobs",
            "`crates/my-app-jobs`",
            "JOBS_WORKER_ID",
        ],
    );
}

#[test]
fn rust_react_service_options_default_to_none() {
    let temp = tempdir().unwrap();
    let plan = rust_react_service_plan(temp.path(), "my-app", ScaffoldDb::None, None, None)
        .unwrap()
        .unwrap();
    let report = plan.write(temp.path(), false).unwrap();
    assert_eq!(report["metrics"], "none");
    assert_eq!(report["jobs"], "none");
    assert_paths_absent(
        temp.path(),
        &[
            "crates/my-app-jobs",
            "crates/my-app-runtime/src/metrics.rs",
        ],
    );
    let workspace = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(workspace.contains(r#"features = ["axum"] }"#));
    let env_example = fs::read_to_string(temp.path().join(".env.example")).unwrap();
    assert_contains_none(&env_example, &["METRICS_OTLP_ENDPOINT", "JOBS_WORKER_ID"]);
}

#[test]
fn metrics_without_a_database_renders_the_exporter_only() {
    let temp = tempdir().unwrap();
    let plan = rust_react_service_plan(
        temp.path(),
        "my-app",
        ScaffoldDb::None,
        Some(ScaffoldMetrics::Otlp),
        Some(ScaffoldJobs::None),
    )
    .unwrap()
    .unwrap();
    plan.write(temp.path(), false).unwrap();
    let workspace = fs::read_to_string(temp.path().join("Cargo.toml")).unwrap();
    assert!(workspace.contains(r#"features = ["axum", "otlp"] }"#));
    assert!(!workspace.contains("uuid ="));
    assert!(temp.path().join("crates/my-app-runtime/src/metrics.rs").exists());
    assert!(!temp.path().join("crates/my-app-jobs").exists());
}

#[test]
fn service_options_require_rust_react_and_runledger_requires_postgres() {
    for preset in [
        ScaffoldPreset::GoReact,
        ScaffoldPreset::RustLibrary,
        ScaffoldPreset::RustCli,
    ] {
        for (metrics, jobs, flag) in [
            (Some(ScaffoldMetrics::Otlp), None, "--metrics"),
            (None, Some(ScaffoldJobs::Runledger), "--jobs"),
        ] {
            let error = ScaffoldOpts {
                preset: Some(preset),
                metrics,
                jobs,
                ..ScaffoldOpts::default()
            }
            .validate_init_invariants(&AnswerOpts::default())
            .unwrap_err()
            .to_string();
            assert!(
                error.contains(&format!("{flag} requires --preset rust-react")),
                "{preset:?}: {error}"
            );
        }
    }
    let harness_only = ScaffoldOpts {
        preset: Some(ScaffoldPreset::HarnessOnly),
        metrics: Some(ScaffoldMetrics::None),
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts::default())
    .unwrap_err()
    .to_string();
    assert!(harness_only.contains("--metrics, or --jobs"), "{harness_only}");

    let explicit_none = ScaffoldOpts {
        preset: Some(ScaffoldPreset::RustReact),
        db: Some(ScaffoldDb::None),
        jobs: Some(ScaffoldJobs::Runledger),
        ..ScaffoldOpts::default()
    }
    .validate_init_invariants(&AnswerOpts::default())
    .unwrap_err()
    .to_string();
    assert!(explicit_none.contains("--jobs runledger requires --db postgres"));

    let temp = tempdir().unwrap();
    let omitted_database = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            jobs: Some(ScaffoldJobs::Runledger),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(omitted_database.contains("--jobs runledger requires --db postgres"));
}

#[test]
fn rust_react_rejects_names_colliding_with_selected_service_packages() {
    let temp = tempdir().unwrap();
    let cases = [
        (
            "runledger",
            ScaffoldDb::Postgres,
            None,
            Some(ScaffoldJobs::Runledger),
            "the generated package 'runledger-core'",
        ),
        (
            "batter-runledger",
            ScaffoldDb::Postgres,
            None,
            Some(ScaffoldJobs::Runledger),
            "normalizes to 'batter-runledger', which",
        ),
        (
            "batter-otlp",
            ScaffoldDb::None,
            Some(ScaffoldMetrics::Otlp),
            None,
            "normalizes to 'batter-otlp', which",
        ),
        (
            "postgres-test-harness",
            ScaffoldDb::Postgres,
            None,
            None,
            "normalizes to 'postgres-test-harness', which",
        ),
        (
            "batter-test-support",
            ScaffoldDb::Postgres,
            None,
            None,
            "normalizes to 'batter-test-support', which",
        ),
    ];
    for (name, db, metrics, jobs, expected) in cases {
        let error = rust_react_service_plan(temp.path(), name, db, metrics, jobs)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("conflicts with a required Batter dependency"),
            "{name}: {error}"
        );
        assert!(error.contains(expected), "{name}: {error}");
        assert!(error.contains("Choose a different --repo-name"), "{name}: {error}");
    }
    for (name, db) in [
        ("runledger", ScaffoldDb::Postgres),
        ("batter-otlp", ScaffoldDb::None),
        ("batter-test-support", ScaffoldDb::None),
        ("postgres-test-harness", ScaffoldDb::None),
    ] {
        rust_react_service_plan(temp.path(), name, db, None, None)
            .unwrap_or_else(|error| panic!("{name} without its package must be accepted: {error}"))
            .unwrap();
    }
}
