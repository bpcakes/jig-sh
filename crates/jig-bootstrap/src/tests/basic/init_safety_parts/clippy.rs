#[test]
fn rust_scaffolds_treat_clippy_config_as_project_owned_whole_file() {
    for preset in [
        ScaffoldPreset::RustReact,
        ScaffoldPreset::RustLibrary,
        ScaffoldPreset::RustCli,
    ] {
        let destination = tempdir().unwrap();
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(preset),
                db: (preset == ScaffoldPreset::RustReact).then_some(ScaffoldDb::None),
                frontends: Vec::new(),
                frontend_list: Vec::new(),
            },
            &AnswerOpts {
                repo_name: Some("ExampleProject".into()),
                ..AnswerOpts::default()
            },
            destination.path(),
        )
        .unwrap()
        .unwrap();

        plan.write(destination.path(), false).unwrap();
        let clippy_path = destination.path().join("clippy.toml");
        let generated = fs::read_to_string(&clippy_path).unwrap();
        let project_owned = "# Project-owned Clippy policy\ncognitive-complexity-threshold = 40\n";
        fs::write(&clippy_path, project_owned).unwrap();
        let before_conflict = regular_file_tree_snapshot(destination.path());

        let error = plan.write(destination.path(), false).unwrap_err().to_string();
        assert!(error.contains("clippy.toml"), "{}: {error}", preset.as_str());
        assert!(error.contains("pass --force"), "{}: {error}", preset.as_str());
        assert_eq!(regular_file_tree_snapshot(destination.path()), before_conflict);

        let forced = plan.write(destination.path(), true).unwrap();
        assert_eq!(forced["files_modified"], serde_json::json!(["clippy.toml"]));
        assert_eq!(fs::read_to_string(&clippy_path).unwrap(), generated);

        let rerun = plan.write(destination.path(), false).unwrap();
        assert!(
            rerun["files_unchanged"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == "clippy.toml")
        );
    }
}
