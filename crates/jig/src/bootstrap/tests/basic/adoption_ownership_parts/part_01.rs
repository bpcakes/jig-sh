#[test]
fn minimal_expansion_adds_generated_frontend_commands_around_project_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let mut commands = toml::Table::new();
    commands.insert(
        "release_command".into(),
        toml::Value::String("just release".into()),
    );
    commands.insert(
        "typescript_lint_command".into(),
        toml::Value::String("npm run project-lint".into()),
    );
    commands.insert(
        "typescript_typecheck_command".into(),
        toml::Value::String("  ".into()),
    );
    commands.insert(
        "typescript_build_command".into(),
        toml::Value::String(String::new()),
    );
    commands.insert(
        "rust_test_command".into(),
        toml::Value::String(" \t ".into()),
    );
    config
        .as_table_mut()
        .unwrap()
        .insert("commands".into(), toml::Value::Table(commands));
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.web_package_manager = Some("npm".into());
    full.answers.frontend_apps = vec![frontend_app()];
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(
        config["commands"]["release_command"].as_str(),
        Some("just release")
    );
    assert_eq!(
        config["commands"]["typescript_lint_command"].as_str(),
        Some("npm run project-lint")
    );
    assert_eq!(
        config["commands"]["typescript_typecheck_command"].as_str(),
        Some("scripts/check-webapps.sh typecheck")
    );
    assert_eq!(
        config["commands"]["typescript_build_command"].as_str(),
        Some("scripts/check-webapps.sh build")
    );
    assert!(config["commands"].get("rust_test_command").is_none());
    for key in [
        "typescript_lint_command",
        "typescript_typecheck_command",
        "typescript_build_command",
        "typescript_coverage_command",
    ] {
        assert!(config["commands"][key].as_str().is_some(), "missing {key}");
    }
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);
}
