#[test]
fn run_init_rust_react_scaffold_generates_backend_and_frontends() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("my-app");
    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: Vec::new(),
            frontend_list: vec![
                parse_scaffold_frontend("web").unwrap(),
                parse_scaffold_frontend("landing").unwrap(),
                parse_scaffold_frontend("admin").unwrap(),
            ],
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            ci_github_runner: Some("macos-14".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    assert_rust_react_guidance_and_policy(&destination, &output);
    assert_rust_react_report_and_paths(&destination, &output);
    assert_workspace_and_contract_tooling(&destination);
    assert_public_spa_package_and_clients(&destination);
    assert_public_spa_source_and_vite(&destination);
    assert_public_spa_e2e(&destination);
    assert_generated_ci_workflows(&destination);
    assert_landing_and_admin_tooling(&destination);
    assert_admin_theme_and_components(&destination);
    assert_admin_data_and_routes(&destination);
    assert_agent_map_and_database_ignores(&destination);
    assert_api_entrypoint_and_dev_config(&destination);
    assert_workspace_and_backend_crates(&destination);
    assert_http_contract_and_test_support(&destination);
    assert_database_support_and_docs(&destination);
    assert_rendered_jig_answers(&destination);
}
