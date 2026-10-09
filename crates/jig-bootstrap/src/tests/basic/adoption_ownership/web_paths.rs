use super::*;

#[test]
fn full_without_web_preserves_project_web_paths_during_minimal_retirement() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    write_project_sentinels(&repo, WEB_HARNESS_PATHS);

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_project_sentinels(&repo, WEB_HARNESS_PATHS);
    assert!(WEB_HARNESS_PATHS.iter().all(|path| {
        !output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|removed| removed == *path)
    }));
}

#[test]
fn full_with_web_retires_web_paths_when_switching_to_minimal() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.frontend_apps = vec![frontend_app()];
    run_adopt(full).unwrap();
    add_project_runtime_tables(&repo);
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"].as_table_mut().unwrap().insert(
        "release_command".into(),
        toml::Value::String("just release".into()),
    );
    config["commands"].as_table_mut().unwrap().insert(
        "typescript_lint_command".into(),
        toml::Value::String("npm run project-lint".into()),
    );
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    assert!(
        WEB_HARNESS_PATHS
            .iter()
            .all(|path| repo.join(path).is_file())
    );

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert!(
        WEB_HARNESS_PATHS
            .iter()
            .all(|path| !repo.join(path).exists())
    );
    assert!(WEB_HARNESS_PATHS.iter().all(|path| {
        output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|removed| removed == *path)
    }));
    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("[[frontend_apps]]"));
    assert!(config.contains("[[dev.apps]]"));
    assert!(config.contains("typescript_lint_command = \"npm run project-lint\""));
    assert!(!config.contains("typescript_typecheck_command"));
    assert!(!config.contains("typescript_build_command"));
    assert!(!config.contains("typescript_coverage_command"));
    assert!(!config.contains("tool = \"jig.typescript_"));
    assert!(config.contains("release_command = \"just release\""));
    let config = toml::from_str::<toml::Value>(&config).unwrap();
    assert_project_runtime_tables(&config);
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("typescript_"));
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| { !WEB_HARNESS_PATHS.contains(&path.as_str()) })
    );
}

#[test]
fn full_readoption_preserves_authored_web_ownership_when_manifests_disappear() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut with_web = footprint_adopt_opts(&repo, template.path(), false, false);
    with_web.answers.frontend_apps = vec![frontend_app()];
    run_adopt(with_web).unwrap();
    fs::remove_dir_all(repo.join("apps")).unwrap();
    fs::remove_file(repo.join("package.json")).unwrap();
    fs::remove_file(repo.join("package-lock.json")).unwrap();

    let before = fs::read(repo.join(".jig.toml")).unwrap();
    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap_err();
    assert!(
        error.to_string().contains("missing package.json"),
        "{error:#}"
    );
    assert!(
        error.to_string().contains("[repository.components]")
            && error.to_string().contains("[repository.actions]"),
        "{error:#}"
    );
    assert_eq!(fs::read(repo.join(".jig.toml")).unwrap(), before);
    assert!(
        WEB_HARNESS_PATHS
            .iter()
            .all(|path| repo.join(path).is_file())
    );
}

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
        config["commands"]["repo_compat_typescript_lint_command"].as_str(),
        Some("npm run project-lint")
    );
    assert_eq!(
        config["commands"]["repo_compat_typescript_typecheck_command"].as_str(),
        Some("scripts/check-webapps.sh typecheck")
    );
    assert_eq!(
        config["commands"]["repo_compat_typescript_build_command"].as_str(),
        Some("scripts/check-webapps.sh build")
    );
    assert!(config["commands"].get("rust_test_command").is_none());
    for key in [
        "web_lint_command",
        "web_typecheck_command",
        "web_build_command",
        "web_test_command",
    ] {
        assert!(config["commands"][key].as_str().is_some(), "missing {key}");
    }
    let ctx = jig_context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(jig_policy::contract_check(&ctx).exit_status, 0);
}
