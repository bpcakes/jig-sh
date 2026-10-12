use super::*;

#[test]
fn scaffold_prefixes_repo_names_that_are_invalid_rust_crate_identifiers() {
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
            repo_name: Some("123-type".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(plan.summary().contains("repo name app-123-type"));
    assert!(
        plan.sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'app-123-type'")
    );
    plan.write(temp.path(), false).unwrap();

    assert!(
        temp.path()
            .join("apps/app-123-type-api/src/main.rs")
            .exists()
    );
    let main_rs =
        fs::read_to_string(temp.path().join("apps/app-123-type-api/src/main.rs")).unwrap();
    assert!(main_rs.contains("use ::app_123_type_http as app_http_crate;"));
    assert!(main_rs.contains("runtime::serve(config, app_http_crate::assemble)"));
    let core_lib =
        fs::read_to_string(temp.path().join("crates/app-123-type-core/src/lib.rs")).unwrap();
    assert!(core_lib.contains("#[allow(clippy::useless_concat)]\npub const APP_NAME"));
    assert!(core_lib.contains("pub const APP_NAME: &str = concat!("));
    assert!(core_lib.contains("\"app-123-type\","));

    let mixed_case = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: None,
            frontends: Vec::new(),
            frontend_list: Vec::new(),
            metrics: None,
            jobs: None,
        },
        &AnswerOpts {
            repo_name: Some("MyApp".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();
    assert!(
        mixed_case
            .sanitized_repo_name_note()
            .unwrap()
            .contains("normalized to 'myapp'")
    );
}

#[test]
fn go_scaffold_keeps_names_that_are_only_rust_keywords() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts {
            repo_name: Some("loop".into()),
            go_module: Some("example.com/loop".into()),
            ..AnswerOpts::default()
        },
        temp.path(),
    )
    .unwrap()
    .unwrap();

    assert!(plan.summary().contains("Go backend for loop"));
    assert!(plan.sanitized_repo_name_note().is_none());
    plan.write(temp.path(), false).unwrap();
    let workspace = fs::read_to_string(temp.path().join("package.json")).unwrap();
    assert!(workspace.contains(r#""name": "loop-workspace""#));
}

#[test]
fn scaffold_generated_rust_workspace_has_valid_cargo_metadata() {
    let temp = tempdir().unwrap();
    let plan = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
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
    .unwrap()
    .unwrap();
    plan.write(temp.path(), false).unwrap();

    let output = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let package_names = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| package["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        "demo",
        "demo-api",
        "demo-core",
        "demo-db",
        "demo-http",
        "demo-test-support",
    ] {
        assert!(
            package_names.contains(&expected),
            "missing package {expected}"
        );
    }
}

#[test]
fn scaffold_test_support_uses_absolute_paths_for_local_module_name_collisions() {
    let temp = tempdir().unwrap();

    for repo_name in ["app", "db", "http", "responses"] {
        let destination = temp.path().join(repo_name);
        fs::create_dir(&destination).unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: Some(ScaffoldDb::Postgres),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
                metrics: None,
                jobs: None,
            },
            &AnswerOpts {
                repo_name: Some(repo_name.into()),
                ..AnswerOpts::default()
            },
            &destination,
        )
        .unwrap()
        .unwrap();
        plan.write(&destination, false).unwrap();

        let module_name = repo_name.replace('-', "_");
        let test_support = destination
            .join("crates")
            .join(format!("{repo_name}-test-support"));
        let lib = fs::read_to_string(test_support.join("src/lib.rs")).unwrap();
        assert!(
            lib.contains("pub use self::app::TestApp;")
                && lib.contains("pub use self::responses::TestResponse;")
                && !lib.contains(&format!("use {module_name}")),
            "test-support modules must not shadow the application crate for {repo_name}:\n{lib}"
        );
        let postgres = fs::read_to_string(test_support.join("tests/postgres.rs")).unwrap();
        assert!(
            postgres.contains(&format!("use ::{module_name} as app_crate;"))
                && postgres.contains(&format!(
                    "use ::{module_name}_test_support::db::with_migrated_database;"
                )),
            "integration test crate paths were ambiguous for {repo_name}:\n{postgres}"
        );
        let app = fs::read_to_string(test_support.join("src/app.rs")).unwrap();
        assert!(
            app.contains(&format!("use ::{module_name} as app_crate;"))
                && app.contains("app_crate::AppState::for_tests()"),
            "application crate path was ambiguous for {repo_name}:\n{app}"
        );
        let db = fs::read_to_string(test_support.join("src/db.rs")).unwrap();
        assert!(
            db.contains(&format!("use ::{module_name}_db as app_db_crate;"))
                && db.contains("pub type TestDbPool = app_db_crate::DbPool;"),
            "database crate path was ambiguous for {repo_name}:\n{db}"
        );

        if repo_name == "app" {
            let output = std::process::Command::new("cargo")
                .args(["fmt", "--all", "--", "--check"])
                .current_dir(&destination)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "cargo fmt failed for the colliding-name database scaffold\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
