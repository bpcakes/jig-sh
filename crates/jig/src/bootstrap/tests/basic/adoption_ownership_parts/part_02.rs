#[test]
fn custom_template_cannot_stage_reserved_git_metadata_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let custom_template = template.path().join("templates/project/.git/config.jinja");
    fs::create_dir_all(custom_template.parent().unwrap()).unwrap();
    fs::write(&custom_template, "managed git config\n").unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("project-sentinel"), "project-owned\n").unwrap();
    let repo_before = regular_file_tree_snapshot(&repo);

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
        .unwrap_err()
        .to_string();

    assert!(error.contains("reserved Git metadata component"), "{error}");
    assert!(error.contains(".git/config"), "{error}");
    assert!(!error.to_ascii_lowercase().contains("--force"), "{error}");
    assert_eq!(regular_file_tree_snapshot(&repo), repo_before);
    assert_eq!(
        fs::read_to_string(repo.join("project-sentinel")).unwrap(),
        "project-owned\n"
    );
}

#[test]
fn manifest_retires_custom_template_paths_removed_by_a_later_render() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let custom_template = template
        .path()
        .join("templates/project/custom-policy.txt.jinja");
    fs::write(&custom_template, "managed custom policy\n").unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    assert!(repo.join("custom-policy.txt").is_file());
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .any(|path| path == "custom-policy.txt")
    );
    fs::remove_file(custom_template).unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    assert!(!repo.join("custom-policy.txt").exists());
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| path != "custom-policy.txt")
    );
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "custom-policy.txt")
    );
}

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
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("typescript_"));
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| { !WEB_HARNESS_PATHS.contains(&path.as_str()) })
    );
}

#[test]
fn full_with_web_retires_web_paths_when_readopted_without_web() {
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

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

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
}

#[test]
fn legacy_named_project_paths_absent_from_manifest_are_preserved() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let unconditional = ["scripts/check-agent-guides.sh"];
    let conditional = [
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &unconditional);
    write_project_sentinels(&repo, &conditional);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_project_sentinels(&repo, &unconditional);
    assert_project_sentinels(&repo, &conditional);
}

#[test]
fn runtime_sqlx_answers_do_not_infer_legacy_path_ownership() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.sqlx_enabled = Some(true);
    full.answers.rust_migration_dir = Some("migrations".into());
    full.answers.schema_dump_enabled = Some(false);
    run_adopt(full).unwrap();
    let sqlx_path = "scripts/add-migration.sh";
    let unrelated = [
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &[sqlx_path]);
    write_project_sentinels(&repo, &unrelated);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_project_sentinels(&repo, &[sqlx_path]);
    assert_project_sentinels(&repo, &unrelated);
}

#[test]
fn runtime_feature_answers_do_not_authorize_legacy_retirement() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.frontend_apps = vec![frontend_app()];
    full.answers.sqlx_enabled = Some(true);
    full.answers.rust_migration_dir = Some("migrations".into());
    full.answers.schema_dump_enabled = Some(true);
    run_adopt(full).unwrap();
    let legacy = [
        "scripts/check-agent-guides.sh",
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &legacy);

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, true);
    minimal.answers.sqlx_enabled = None;
    run_adopt(minimal).unwrap();

    assert_project_sentinels(&repo, &legacy);
}

#[test]
fn minimal_adoption_staging_still_rejects_invalid_commands_and_tools() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let config_template = template.path().join("templates/project/.jig.toml.jinja");
    let config = fs::read_to_string(&config_template).unwrap();
    let config = config.replace(
        "<<[ repository_commands_toml ]>>",
        "<<[ repository_commands_toml | replace(bootstrap_command, \"  \") ]>>",
    );
    fs::write(&config_template, format!("{config}\n")).unwrap();
    let contract_template = template
        .path()
        .join("templates/project/.agent/jig-contract.json.jinja");
    let contract = fs::read_to_string(&contract_template).unwrap().replace(
        "\"tools\": <<[ repository.tools | tojson(indent=2) ]>>",
        "\"tools\": [{\"name\":\"jig.unsupported\",\"kind\":\"native\",\"description\":\"unsupported test tool\"}]",
    );
    fs::write(&contract_template, contract).unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap_err();
    let error = format!("{error:#}");

    assert!(
        error.contains("Command key repo_bootstrap_command is empty"),
        "{error}"
    );
    assert!(
        error.contains("Unsupported native tool: jig.unsupported"),
        "{error}"
    );
    assert!(!repo.join(".jig.toml").exists());
}

#[test]
fn forced_minimal_adoption_with_invalid_prior_config_preserves_omitted_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    let mcp_contents = b"{\"projectOwned\":true}\n";
    let workflow_contents = b"name: project policy\n";
    fs::create_dir_all(repo.join(".github/workflows")).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        "harness_footprint = \"not-a-footprint\"\n",
    )
    .unwrap();
    fs::write(repo.join(".mcp.json"), mcp_contents).unwrap();
    fs::write(
        repo.join(".github/workflows/repo-policy.yml"),
        workflow_contents,
    )
    .unwrap();
    let legacy_paths = [
        "scripts/check-agent-guides.sh",
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];
    write_project_sentinels(&repo, &legacy_paths);

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();

    assert_eq!(fs::read(repo.join(".mcp.json")).unwrap(), mcp_contents);
    assert_eq!(
        fs::read(repo.join(".github/workflows/repo-policy.yml")).unwrap(),
        workflow_contents
    );
    assert_project_sentinels(&repo, &legacy_paths);
    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
}

#[test]
fn invalid_runtime_config_is_not_preserved_by_readoption_or_update() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for update in [false, true] {
        let repo = temp.path().join(if update { "update" } else { "readopt" });
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

        let config_path = repo.join(".jig.toml");
        let mut config =
            toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
        config
            .as_table_mut()
            .unwrap()
            .insert("commands".into(), toml::Value::String("invalid".into()));
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        assert!(crate::context::RepoContext::validate_config_file(&repo).is_err());

        if update {
            run_update(update_opts(&repo, template.path(), false)).unwrap();
        } else {
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();
        }

        let repaired =
            toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                .unwrap();
        assert!(repaired["commands"].as_table().is_some());
        assert!(repaired["commands"]["api_test_command"].as_str().is_some());
        crate::context::RepoContext::load_from(&repo).unwrap();
    }
}

#[test]
fn minimal_adoption_expands_to_full_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    add_project_runtime_tables(&repo);
    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    assert_eq!(output["harness_footprint"], "full");
    assert!(repo.join("scripts/jig").is_file());
    assert!(repo.join(".mcp.json").is_file());
    assert!(repo.join(".github/workflows/rust-tests.yml").is_file());
    assert!(repo.join("AGENTS.md").is_file());
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);
    crate::context::RepoContext::load_from(&repo).unwrap();
}

#[test]
fn update_preserves_project_runtime_tables_for_minimal_and_full_harnesses() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for minimal in [true, false] {
        for force in [false, true] {
            let repo = temp.path().join(format!(
                "{}-{force}",
                if minimal { "minimal" } else { "full" }
            ));
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), minimal, false)).unwrap();
            add_project_runtime_tables(&repo);

            run_update(update_opts(&repo, template.path(), force)).unwrap();

            let config =
                toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                    .unwrap();
            assert_project_runtime_tables(&config);
            assert_eq!(
                config["harness_footprint"].as_str(),
                Some(if minimal { "minimal" } else { "full" })
            );
            crate::context::RepoContext::load_from(&repo).unwrap();
        }
    }
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
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);
}
