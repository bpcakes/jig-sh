
#[test]
fn init_rejects_windows_aliased_scaffold_components_before_any_repository_write() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for force in [false, true] {
        for (case_name, frontend_dir) in [
            ("trailing-dot", "web."),
            ("device", "CON"),
            ("device-extension", "NUL.txt"),
        ] {
            let destination = temp.path().join(format!("{case_name}-{force}"));
            fs::create_dir(&destination).unwrap();
            let outside = temp.path().join(format!("outside-{case_name}-{force}"));
            fs::write(&outside, "outside sentinel\n").unwrap();

            let error = run_init(InitOpts {
                path: destination.clone(),
                scaffold: ScaffoldOpts {
                    preset: Some(ScaffoldPreset::RustReact),
                    db: Some(ScaffoldDb::None),
                    frontends: Vec::new(),
                    frontend_list: Vec::new(),
                },
                template: Some(template.path().display().to_string()),
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    repo_name: Some("demo".into()),
                    frontend_apps: vec![FrontendApp {
                        name: "client".into(),
                        dir: frontend_dir.into(),
                        coverage_threshold: 80,
                        kind: "vite".into(),
                        role: "spa".into(),
                    }],
                    ..AnswerOpts::default()
                },
            })
            .unwrap_err()
            .to_string();

            assert!(
                error.contains("not portable to Windows"),
                "{case_name}/{force}: {error}"
            );
            assert!(error.contains(frontend_dir), "{case_name}/{force}: {error}");
            assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
            assert!(
                fs::read_dir(&destination).unwrap().next().is_none(),
                "{case_name}/{force}: portability preflight partially mutated the destination"
            );
        }
    }
}
