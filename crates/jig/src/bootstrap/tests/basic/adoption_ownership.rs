use super::*;

#[test]
fn adopt_defaults_to_tooling_only_when_sqlx_answers_are_omitted() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(
        output["detection_report"]["summary"]
            .as_str()
            .unwrap()
            .contains("no Rust workspace, no SQLx")
    );
    assert!(
        !output["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| { note.as_str().unwrap().contains("tooling-only profile") })
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"repo\""));
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!repo.join(".github/workflows/webapp-checks.yml").exists());
    assert!(!repo.join("scripts/check-webapps.sh").exists());
    assert!(!repo.join("scripts/check-webapp-scripts.mjs").exists());
    assert!(!repo.join("scripts/enforce-coverage.js").exists());
    assert!(!repo.join("scripts/enforce-coverage.cjs").exists());
    assert!(
        !output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".github/workflows/webapp-checks.yml")
    );
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn adopt_resolves_relative_answers_file_from_the_launcher_invocation_directory() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("invocation");
    let other = temp.path().join("other");
    let repo = invocation.join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();
    fs::write(
        invocation.join("answers.toml"),
        "repo_name = \"invocation-answers\"\nsqlx_enabled = false\n",
    )
    .unwrap();
    fs::write(
        other.join("answers.toml"),
        "repo_name = \"process-cwd-answers\"\nsqlx_enabled = false\n",
    )
    .unwrap();
    let template = materialize_template_worktree();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);

    run_adopt(AdoptOpts {
        path: PathBuf::from("repo"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(PathBuf::from("answers.toml")),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("repo_name = \"invocation-answers\""));
    assert!(!config.contains("process-cwd-answers"));
}

#[test]
fn adopt_minimal_writes_config_and_agent_scaffolding_only() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("README.md"), "project\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: true,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["harness_footprint"], "minimal");
    assert_eq!(output["ok"], true);
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap();
    assert!(
        generated_gates
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    assert!(generated_gates.iter().any(|gate| gate == "jig bootstrap"));
    let command_report = output["render_report"]["commands_detected_or_skipped"]
        .as_array()
        .unwrap();
    assert!(
        command_report
            .iter()
            .all(|command| { !command.as_str().unwrap().contains("scripts/jig") })
    );
    assert!(command_report.iter().any(|command| {
        command
            .as_str()
            .unwrap()
            .contains("bootstrap_command configured; run jig bootstrap")
    }));
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("harness_footprint = \"minimal\""));
    assert!(repo.join(".agent/jig-contract.json").is_file());
    assert!(repo.join(".agent/PLANS.md").is_file());
    assert!(repo.join(".agent/plans/.gitkeep").is_file());
    assert!(repo.join(".agent/state/.gitkeep").is_file());
    assert!(repo.join(".agent/.cache/.gitignore").is_file());
    assert!(repo.join(managed_paths::MANIFEST_PATH).is_file());
    assert!(repo.join(".gitignore").is_file());
    assert!(repo.join(".gitattributes").is_file());
    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join("scripts/install-jig.sh").exists());
    assert!(!repo.join(".mcp.json").exists());
    assert!(!repo.join("AGENTS.md").exists());
    assert!(!repo.join("agent-map.md").exists());
    assert!(!repo.join(".github/workflows/rust-tests.yml").exists());
    assert!(!repo.join(".github/workflows/repo-policy.yml").exists());
    assert!(!repo.join(".github/workflows/agent-map-check.yml").exists());
    let manifest_paths = managed_manifest_paths(&repo);
    assert_eq!(
        manifest_paths,
        output["adoption_profile"]["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        manifest_paths,
        output["render_report"]["active_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(manifest_paths.windows(2).all(|paths| paths[0] < paths[1]));
    assert!(manifest_paths.iter().all(|path| repo.join(path).is_file()));
    assert!(
        manifest_paths
            .iter()
            .any(|path| path == managed_paths::MANIFEST_PATH)
    );
    assert!(manifest_paths.iter().all(|path| path != "AGENTS.md"));
    assert!(
        output["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("Minimal adoption"))
    );
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("jig loop"))
    );

    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(ctx.repo_name(), "demo");
    assert!(!ctx.required_commands().is_empty());
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);

    run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join("AGENTS.md").exists());
    assert!(!repo.join("agent-map.md").exists());
    let answers_after_update = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers_after_update.contains("harness_footprint = \"minimal\""));
}

#[test]
fn minimal_frontend_keeps_metadata_without_enabling_web_harness_capabilities() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);
    let mut opts = footprint_adopt_opts(&repo, template.path(), true, false);
    opts.answers.frontend_apps = vec![frontend_app()];
    opts.answers.sqlx_enabled = Some(true);
    opts.answers.rust_migration_dir = Some("migrations".into());

    let output = run_adopt(opts).unwrap();

    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("[[frontend_apps]]"));
    assert!(config.contains("[[dev.apps]]"));
    assert!(!config.contains("typescript_lint_command"));
    assert!(!config.contains("tool = \"jig.typescript_"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("typescript_"));
    assert!(contract.contains(r#""name": "jig.sqlx_check""#));
    assert!(!repo.join("scripts/check-webapps.sh").exists());
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap();
    assert!(
        generated_gates
            .iter()
            .all(|gate| !gate.as_str().unwrap().contains("typescript"))
    );
    assert!(generated_gates.iter().any(|gate| gate == "jig check sqlx"));
    assert!(
        generated_gates
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    let command_report = output["render_report"]["commands_detected_or_skipped"]
        .as_array()
        .unwrap();
    assert!(
        command_report
            .iter()
            .any(|command| { command.as_str() == Some("[[dev.apps]] configured; run jig dev") })
    );
    assert!(command_report.iter().all(|command| {
        !command.as_str().unwrap().contains("scripts/jig")
            && !command.as_str().unwrap().contains("typescript")
    }));
    let ctx = crate::context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(ctx.frontend_apps().len(), 1);
    assert!(
        jig_features::required_contract_tools(&ctx)
            .iter()
            .all(|tool| !tool.contains("typescript"))
    );
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);
}

#[test]
fn first_time_minimal_adoption_preserves_project_owned_omitted_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let mcp_contents = b"{\"mcpServers\":{\"project\":{}}}\n";
    let workflow_contents = b"name: project rust tests\n";
    let legacy_paths = [
        "scripts/check-agent-guides.sh",
        "scripts/add-migration.sh",
        "scripts/check-schema-dump.sh",
        "scripts/enforce-coverage.js",
    ];

    for force in [false, true] {
        let repo = temp.path().join(if force { "forced" } else { "normal" });
        fs::create_dir_all(repo.join(".github/workflows")).unwrap();
        fs::write(repo.join(".mcp.json"), mcp_contents).unwrap();
        fs::write(
            repo.join(".github/workflows/rust-tests.yml"),
            workflow_contents,
        )
        .unwrap();
        write_project_sentinels(&repo, &legacy_paths);

        let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, force)).unwrap();

        assert_eq!(fs::read(repo.join(".mcp.json")).unwrap(), mcp_contents);
        assert_eq!(
            fs::read(repo.join(".github/workflows/rust-tests.yml")).unwrap(),
            workflow_contents
        );
        assert_project_sentinels(&repo, &legacy_paths);
        assert!(
            !output["render_report"]["files_removed"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == ".mcp.json" || path == ".github/workflows/rust-tests.yml")
        );
    }
}

#[test]
fn missing_manifest_blocks_update_and_explicit_adopt_establishes_ownership() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    add_project_runtime_tables(&repo);
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["web_package_manager"] = toml::Value::String("npm".into());
    config["dev"].as_table_mut().unwrap().insert(
        "apps".into(),
        toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
            ("name".into(), toml::Value::String("api".into())),
            ("kind".into(), toml::Value::String("env-port".into())),
            (
                "command".into(),
                toml::Value::String("cargo run -p api".into()),
            ),
        ]))]),
    );
    config["agent_tooling"]["codex"]["marketplaces"][0]["source"] =
        toml::Value::String("example/custom-skills".into());
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    fs::remove_file(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
    let project_owned = ["scripts/check-agent-guides.sh", "scripts/add-migration.sh"];
    write_project_sentinels(&repo, &project_owned);

    let error = run_update(update_opts(&repo, template.path(), false))
        .unwrap_err()
        .to_string();
    assert!(error.contains(managed_paths::MANIFEST_PATH), "{error}");
    assert!(error.contains("jig adopt . --write"), "{error}");

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

    assert!(repo.join(managed_paths::MANIFEST_PATH).is_file());
    assert_project_sentinels(&repo, &project_owned);
    assert!(
        output["adoption_profile"]["retired_managed_files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| { !project_owned.contains(&path.as_str()) })
    );
    let established =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(established["web_package_manager"].as_str(), Some("npm"));
    assert_eq!(established["dev"]["apps"][0]["name"].as_str(), Some("api"));
    assert_eq!(
        established["agent_tooling"]["codex"]["marketplaces"][0]["source"].as_str(),
        Some("example/custom-skills")
    );
    assert_project_runtime_tables(&established);
    run_update(update_opts(&repo, template.path(), false)).unwrap();
}

#[test]
fn missing_manifest_blocks_full_to_minimal_until_full_ownership_is_established() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    fs::remove_file(repo.join(managed_paths::MANIFEST_PATH)).unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true))
        .unwrap_err()
        .to_string();
    assert!(error.contains("without --minimal"), "{error}");
    assert!(repo.join("scripts/jig").is_file());
    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"full\"")
    );

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();
    assert!(!repo.join("scripts/jig").exists());
}

#[test]
fn invalid_manifest_blocks_forced_adoption_without_changes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let sentinel = fs::read(repo.join("scripts/jig")).unwrap();
    fs::write(
        repo.join(managed_paths::MANIFEST_PATH),
        r#"{"version":1,"paths":["../outside",".agent/jig-managed-paths.json"]}"#,
    )
    .unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true))
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Invalid Jig managed-path manifest"),
        "{error}"
    );
    assert_eq!(fs::read(repo.join("scripts/jig")).unwrap(), sentinel);
}

#[test]
fn tampered_manifest_cannot_make_update_or_adopt_remove_project_directory() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for mode in [
        "update",
        "update-force",
        "adopt-preview",
        "adopt-write",
        "adopt-force",
    ] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

        fs::create_dir(repo.join("project-directory")).unwrap();
        fs::write(
            repo.join("project-directory/project-sentinel"),
            "project metadata\n",
        )
        .unwrap();
        fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();
        let existing_backup = repo.join(".agent/.cache/adopt/backups/existing");
        fs::create_dir_all(&existing_backup).unwrap();
        fs::write(existing_backup.join("project-sentinel"), "backup\n").unwrap();
        add_managed_manifest_path(&repo, "project-directory");

        let manifest_before = fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap();
        let canonical_receipt_before = fs::read(repo.join(ADOPT_RECEIPT_PATH)).unwrap();
        let legacy_receipt_before = fs::read(repo.join(LEGACY_ADOPT_RECEIPT_PATH)).unwrap();
        let repo_before = regular_file_tree_snapshot(&repo);

        let error = match mode {
            "update" => run_update(update_opts(&repo, template.path(), false)).unwrap_err(),
            "update-force" => run_update(update_opts(&repo, template.path(), true)).unwrap_err(),
            "adopt-preview" => {
                let mut opts = footprint_adopt_opts(&repo, template.path(), false, false);
                opts.write = false;
                run_adopt(opts).unwrap_err()
            }
            "adopt-write" => {
                run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap_err()
            }
            "adopt-force" => {
                run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap_err()
            }
            _ => unreachable!(),
        }
        .to_string();

        assert!(error.contains("destination leaf"), "{mode}: {error}");
        assert!(error.contains("project-directory"), "{mode}: {error}");
        assert!(error.contains("is a directory"), "{mode}: {error}");
        assert!(
            !error.contains("Re-run with --force") && !error.contains("re-run with --force"),
            "{mode}: structural errors must not suggest force: {error}"
        );
        assert_eq!(regular_file_tree_snapshot(&repo), repo_before, "{mode}");
        assert_eq!(
            fs::read(repo.join(managed_paths::MANIFEST_PATH)).unwrap(),
            manifest_before,
            "{mode}: manifest changed"
        );
        assert_eq!(
            fs::read_to_string(repo.join("project-directory/project-sentinel")).unwrap(),
            "project metadata\n",
            "{mode}: project directory changed"
        );
        assert_eq!(
            fs::read(repo.join(ADOPT_RECEIPT_PATH)).unwrap(),
            canonical_receipt_before,
            "{mode}: canonical receipt changed"
        );
        assert_eq!(
            fs::read(repo.join(LEGACY_ADOPT_RECEIPT_PATH)).unwrap(),
            legacy_receipt_before,
            "{mode}: legacy receipt changed"
        );
        assert_eq!(
            fs::read_to_string(existing_backup.join("project-sentinel")).unwrap(),
            "backup\n",
            "{mode}: existing backup changed"
        );
        assert_eq!(
            fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
            "project plan notes\n",
            "{mode}: an earlier managed path changed"
        );
    }
}

#[test]
fn tampered_manifest_cannot_manage_linked_worktree_git_file() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for alias in [
        ".git",
        "GIT~1/config",
        ".git::$INDEX_ALLOCATION",
        ".g\u{200c}it/config",
        "vendor\\.GiT...\\config",
    ] {
        for mode in [
            "update",
            "update-force",
            "adopt-preview",
            "adopt-write",
            "adopt-force",
        ] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();

            fs::write(repo.join(".git"), "gitdir: ../main/.git/worktrees/demo\n").unwrap();
            fs::write(repo.join(".agent/PLANS.md"), "project plan notes\n").unwrap();
            let existing_backup = repo.join(".agent/.cache/adopt/backups/existing");
            fs::create_dir_all(&existing_backup).unwrap();
            fs::write(existing_backup.join("project-sentinel"), "backup\n").unwrap();
            add_managed_manifest_path(&repo, alias);

            let repo_before = regular_file_tree_snapshot(&repo);

            let error = match mode {
                "update" => run_update(update_opts(&repo, template.path(), false)).unwrap_err(),
                "update-force" => {
                    run_update(update_opts(&repo, template.path(), true)).unwrap_err()
                }
                "adopt-preview" => {
                    let mut opts = footprint_adopt_opts(&repo, template.path(), false, false);
                    opts.write = false;
                    run_adopt(opts).unwrap_err()
                }
                "adopt-write" => {
                    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false))
                        .unwrap_err()
                }
                "adopt-force" => {
                    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true))
                        .unwrap_err()
                }
                _ => unreachable!(),
            }
            .to_string();

            assert!(
                error.contains("reserved Git metadata component"),
                "{alias}/{mode}: {error}"
            );
            assert!(error.contains(".git"), "{alias}/{mode}: {error}");
            assert!(
                !error.to_ascii_lowercase().contains("--force"),
                "{alias}/{mode}: reserved-path errors must not suggest force: {error}"
            );
            assert_eq!(
                regular_file_tree_snapshot(&repo),
                repo_before,
                "{alias}/{mode}"
            );
            assert_eq!(
                fs::read_to_string(repo.join(".git")).unwrap(),
                "gitdir: ../main/.git/worktrees/demo\n",
                "{alias}/{mode}: linked-worktree metadata changed"
            );
            assert_eq!(
                fs::read_to_string(existing_backup.join("project-sentinel")).unwrap(),
                "backup\n",
                "{alias}/{mode}: existing backup changed"
            );
            assert_eq!(
                fs::read_to_string(repo.join(".agent/PLANS.md")).unwrap(),
                "project plan notes\n",
                "{alias}/{mode}: an earlier managed path changed"
            );
        }
    }
}

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
