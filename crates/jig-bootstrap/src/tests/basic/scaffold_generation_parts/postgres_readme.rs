#[test]
fn postgres_readme_matches_selected_backend_test_contract() {
    let planning_root = tempdir().unwrap();
    let go_contract = ["requires Docker", "`TEST_DATABASE_URL`"];
    let rust_contract = ["Batter's fixture harness", "`POSTGRES_TEST_ADMIN_URL`"];
    for (preset, expected, forbidden) in [
        (ScaffoldPreset::GoReact, go_contract, rust_contract),
        (ScaffoldPreset::RustReact, rust_contract, go_contract),
    ] {
        let plan = scaffold::InitScaffoldPlan::from_opts(
            &ScaffoldOpts {
                preset: Some(preset),
                db: Some(ScaffoldDb::Postgres),
                frontend_list: vec![parse_scaffold_frontend("web").unwrap()],
                ..ScaffoldOpts::default()
            },
            &AnswerOpts {
                repo_name: Some("example-project".into()),
                go_module: match preset {
                    ScaffoldPreset::GoReact => Some("example.com/ExampleProject".into()),
                    _ => None,
                },
                ..AnswerOpts::default()
            },
            planning_root.path(),
        )
        .unwrap()
        .unwrap();

        let rendered = plan.render_files().unwrap();
        let readme = rendered_contents(&rendered, "README.md");
        assert_contains_all(readme, &expected);
        assert_contains_none(readme, &forbidden);
        assert!(readme.contains("The tests never fall back to `DATABASE_URL`."));
    }
}
