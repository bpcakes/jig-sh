use super::*;

mod assertions;
mod legacy_paths;
mod manifest;
mod tracker;
mod web_paths;
use assertions::*;

#[test]
fn adopt_defaults_to_tooling_only_when_sqlx_answers_are_omitted() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let output = run_adopt(AdoptOpts {
        components: Default::default(),
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
    assert!(!repo.join(".jig/file-budget.toml").exists());
    assert!(
        !fs::read_to_string(repo.join("AGENTS.md"))
            .unwrap()
            .contains("scripts/jig file-budget audit")
    );
    assert!(!output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("scripts/jig file-budget audit")
    }));
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
        components: Default::default(),
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
        components: Default::default(),
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

    assert_minimal_report(&output);
    assert_minimal_files(&repo);
    assert_minimal_manifest(&repo, &output);
    assert_minimal_guidance(&output);
    assert_minimal_contract(&repo);

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
    let ctx = jig_context::RepoContext::load_from(&repo).unwrap();
    assert_eq!(ctx.frontend_apps().len(), 1);
    assert!(
        jig_features::required_contract_tools(&ctx)
            .iter()
            .all(|tool| !tool.contains("typescript"))
    );
    assert_eq!(jig_policy::contract_check(&ctx).exit_status, 0);
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
fn missing_rendered_config_fails_before_optional_authority_reconciliation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();

    for refresh in ["update", "readopt"] {
        let template = materialize_template_worktree();
        let repo = temp.path().join(refresh);
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
        add_project_runtime_tables(&repo);
        let config_path = repo.join(".jig.toml");
        let config_before = fs::read(&config_path).unwrap();
        fs::remove_file(template.path().join("templates/project/.jig.toml.jinja")).unwrap();

        let error = if refresh == "update" {
            run_update(update_opts(&repo, template.path(), false)).unwrap_err()
        } else {
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap_err()
        };
        let error = format!("{error:#}");

        assert!(
            error.contains("Staging render did not produce .jig.toml"),
            "{error}"
        );
        assert_eq!(fs::read(&config_path).unwrap(), config_before);
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
    assert!(!repo.join(".mcp.json").exists());
    assert!(repo.join(".github/workflows/rust-tests.yml").is_file());
    assert!(repo.join("AGENTS.md").is_file());
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);
    jig_context::RepoContext::load_from(&repo).unwrap();
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
            jig_context::RepoContext::load_from(&repo).unwrap();
        }
    }
}
