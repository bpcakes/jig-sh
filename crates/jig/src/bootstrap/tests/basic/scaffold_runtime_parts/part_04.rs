
#[test]
fn scaffold_test_support_uses_absolute_paths_for_local_module_name_collisions() {
    let temp = tempdir().unwrap();

    for repo_name in ["app", "db", "http", "responses"] {
        let destination = temp.path().join(repo_name);
        fs::create_dir(&destination).unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(ScaffoldPreset::RustReact),
                db: Some(ScaffoldDb::Sqlite),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
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
            lib.contains(&format!("use ::{module_name} as app_crate;"))
                && lib.contains("app_crate::AppState::new()"),
            "application crate path was ambiguous for {repo_name}:\n{lib}"
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
