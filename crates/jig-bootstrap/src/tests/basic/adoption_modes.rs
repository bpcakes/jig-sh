use super::*;

mod git_blocks;
mod preview;
mod vault_policy;

#[test]
fn minimal_to_full_uses_existing_answers_and_preserves_runtime_tables() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, false);
    minimal.answers.default_branch = Some("release".into());
    run_adopt(minimal).unwrap();
    add_project_runtime_tables(&repo);

    let mut full = footprint_adopt_opts(&repo, template.path(), false, true);
    full.answers.repo_name = None;
    full.answers.ci_github_runner = Some("macos-14".into());
    full.answers.sqlx_enabled = Some(true);
    full.answers.rust_migration_dir = Some("db/migrations".into());
    full.answers.rust_sqlx_metadata_dir = Some("db/sqlx-cache".into());
    full.answers.sqlx_check_command = Some("scripts/check-custom-sqlx.sh".into());
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("demo"));
    assert_eq!(config["default_branch"].as_str(), Some("release"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("macos-14"));
    assert_eq!(config["sqlx_enabled"].as_bool(), Some(true));
    assert_eq!(config["rust_migration_dir"].as_str(), Some("db/migrations"));
    assert_eq!(
        config["rust_sqlx_metadata_dir"].as_str(),
        Some("db/sqlx-cache")
    );
    assert_eq!(
        config["sqlx_check_command"].as_str(),
        Some("scripts/check-custom-sqlx.sh")
    );
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);

    let workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    for job in ["fmt", "clippy", "test"] {
        assert_eq!(workflow["jobs"][job]["runs-on"], "macos-14");
    }
    for event in ["pull_request", "push"] {
        let paths = workflow["on"][event]["paths"].as_array().unwrap();
        assert!(paths.iter().any(|path| path == "db/migrations/**"));
        assert!(paths.iter().any(|path| path == "db/sqlx-cache/**"));
    }
    for job in ["clippy", "test"] {
        assert_eq!(
            workflow["jobs"][job]["env"]["SQLX_OFFLINE_DIR"],
            "${{ github.workspace }}/db/sqlx-cache"
        );
    }
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""name": "jig.sqlx_check""#));
    assert!(contract.contains(r#""name": "jig.migration_add""#));
}

#[test]
fn minimal_to_full_preserves_a_complete_authored_repository_model() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("services/api")).unwrap();
    fs::create_dir_all(repo.join("services/worker")).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let authored = authored_mixed_repository_config();
    config["commands"] = authored["commands"].clone();
    config["repository"] = authored["repository"].clone();
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let updated =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let targets = updated["repository"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| {
            format!(
                "{}:{}",
                action["target"]["component"].as_str().unwrap(),
                action["target"]["action"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        [
            "repo:contract",
            "repo:bootstrap",
            "api:verify-custom",
            "worker:verify-custom"
        ]
    );
    assert_eq!(
        updated["repository"]["profiles"][0]["targets"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        updated["commands"]["api_verify_command"].as_str(),
        Some("go test ./...")
    );
    assert_eq!(
        updated["commands"]["worker_verify_command"].as_str(),
        Some("cargo test -p worker")
    );
}

#[test]
fn minimal_to_full_uses_explicit_answers_file_and_preserves_runtime_tables() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    add_project_runtime_tables(&repo);
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
default_branch = "file-branch"
sqlx_enabled = false
rust_test_command = "cargo nextest run"
"#,
    )
    .unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers = AnswerOpts {
        answers_file: Some(answers_file),
        ci_github_runner: Some("ubuntu-24.04".into()),
        ..AnswerOpts::default()
    };
    run_adopt(full).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("from-file"));
    assert_eq!(config["default_branch"].as_str(), Some("file-branch"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("ubuntu-24.04"));
    assert_eq!(
        config["commands"]["api_test_command"].as_str(),
        Some("cargo nextest run")
    );
    assert_eq!(config["harness_footprint"].as_str(), Some("full"));
    assert_project_runtime_tables(&config);
    let workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    let workflow = serde_yaml_ng::from_str::<serde_json::Value>(&workflow).unwrap();
    assert_eq!(workflow["jobs"]["test"]["runs-on"], "ubuntu-24.04");
    for event in ["pull_request", "push"] {
        let paths = workflow["on"][event]["paths"].as_array().unwrap();
        assert!(!paths.iter().any(|path| path == "migrations/**"));
        assert!(!paths.iter().any(|path| path == ".sqlx/**"));
    }
    assert!(workflow["jobs"]["clippy"]["env"].is_null());
    assert!(workflow["jobs"]["test"]["env"].is_null());
}

#[test]
fn full_to_minimal_seeds_existing_answers_before_cli_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.default_branch = Some("release".into());
    full.answers.ci_github_runner = Some("macos-14".into());
    full.answers.rust_test_command = Some("cargo nextest run".into());
    full.answers.dev_apps = vec![DevApp {
        name: "api".into(),
        dir: Some("crates/api".into()),
        kind: "env-port".into(),
        command: Some("cargo run -p api".into()),
        argv: Vec::new(),
        port: Some(8080),
        host: None,
        proxy: true,
    }];
    run_adopt(full).unwrap();
    let initial_config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(
        initial_config["commands"]["api_test_command"].as_str(),
        Some("cargo nextest run")
    );
    add_project_runtime_tables(&repo);
    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["vault"]["allow_global"] = toml::Value::Boolean(true);
    config["agent_tooling"]["codex"]["marketplaces"][0]["source"] =
        toml::Value::String("example/custom-skills".into());
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, true);
    minimal.answers.ci_github_runner = Some("ubuntu-24.04".into());
    run_adopt(minimal).unwrap();

    let config = toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(config["default_branch"].as_str(), Some("release"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("ubuntu-24.04"));
    assert_eq!(
        config["commands"]["api_test_command"].as_str(),
        Some("cargo nextest run")
    );
    assert_eq!(config["dev"]["apps"][0]["name"].as_str(), Some("api"));
    assert_eq!(config["vault"]["allow_global"].as_bool(), Some(true));
    assert_eq!(
        config["agent_tooling"]["codex"]["marketplaces"][0]["source"].as_str(),
        Some("example/custom-skills")
    );
    assert_project_runtime_tables(&config);
    assert_eq!(config["harness_footprint"].as_str(), Some("minimal"));
}

#[test]
fn full_to_minimal_keeps_explicit_answers_file_authoritative() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let mut full = footprint_adopt_opts(&repo, template.path(), false, false);
    full.answers.default_branch = Some("release".into());
    run_adopt(full).unwrap();
    let answers_file = temp.path().join("minimal-answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
default_branch = "file-branch"
ci_github_runner = "macos-14"
sqlx_enabled = false
"#,
    )
    .unwrap();

    let mut minimal = footprint_adopt_opts(&repo, template.path(), true, true);
    minimal.answers = AnswerOpts {
        answers_file: Some(answers_file),
        ci_github_runner: Some("ubuntu-24.04".into()),
        ..AnswerOpts::default()
    };
    run_adopt(minimal).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_eq!(config["repo_name"].as_str(), Some("from-file"));
    assert_eq!(config["default_branch"].as_str(), Some("file-branch"));
    assert_eq!(config["ci_github_runner"].as_str(), Some("ubuntu-24.04"));
    assert_eq!(config["harness_footprint"].as_str(), Some("minimal"));
}

#[test]
fn minimal_to_full_adoption_still_rejects_unrelated_managed_conflicts() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), true, false)).unwrap();
    fs::write(repo.join(".agent/state/.gitkeep"), "project notes\n").unwrap();

    let error = run_adopt(footprint_adopt_opts(&repo, template.path(), false, false))
        .unwrap_err()
        .to_string();

    assert!(error.contains(".agent/state/.gitkeep"));
    assert_eq!(
        fs::read_to_string(repo.join(".agent/state/.gitkeep")).unwrap(),
        "project notes\n"
    );
    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
}

#[test]
fn update_retires_formerly_managed_exec_plan_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    let retired = [".agent/PLANS.md", ".agent/plans/.gitkeep"];
    fs::create_dir_all(repo.join(".agent/plans")).unwrap();
    for path in retired {
        fs::write(repo.join(path), "").unwrap();
        add_managed_manifest_path(&repo, path);
    }

    let error = run_update(update_opts(&repo, template.path(), false))
        .unwrap_err()
        .to_string();
    for path in retired {
        assert!(error.contains(path), "{error}");
        assert!(repo.join(path).is_file(), "{path} changed without --force");
    }

    let output = run_update(update_opts(&repo, template.path(), true)).unwrap();
    let reported = output["render_report"]["retired_managed_paths"]
        .as_array()
        .unwrap();
    for path in retired {
        assert!(reported.iter().any(|reported| reported == path), "{path}");
        assert!(!repo.join(path).exists(), "{path} was not retired");
    }
    assert!(
        managed_manifest_paths(&repo)
            .iter()
            .all(|path| !retired.contains(&path.as_str()))
    );
}

#[test]
fn forced_full_to_minimal_adoption_retires_full_harness_paths() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
    add_project_runtime_tables(&repo);
    let full_manifest = managed_manifest_paths(&repo)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(!repo.join(".mcp.json").exists());
    assert!(repo.join("scripts/jig").is_file());
    assert!(repo.join(".github/workflows/rust-tests.yml").is_file());

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), true, true)).unwrap();
    let minimal_manifest = managed_manifest_paths(&repo)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let expected_retirements = full_manifest
        .difference(&minimal_manifest)
        .cloned()
        .collect::<Vec<_>>();
    let reported_retirements = output["adoption_profile"]["retired_managed_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(reported_retirements, expected_retirements);
    assert_eq!(
        reported_retirements,
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    );

    assert_eq!(output["harness_footprint"], "minimal");
    assert!(!repo.join(".mcp.json").exists());
    assert!(!repo.join("scripts/jig").exists());
    assert!(!repo.join(".github/workflows/rust-tests.yml").exists());
    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert_eq!(root_guide, "# Repository Guidelines\n");
    assert!(
        output["render_report"]["files_removed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/jig")
    );
    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_project_runtime_tables(&config);
    jig_context::RepoContext::load_from(&repo).unwrap();
}

#[cfg(unix)]
#[test]
fn minimal_adoption_rejects_managed_symlink_ancestors_in_preview_write_and_force_modes() {
    let _guard = lock_env();
    let template = materialize_template_worktree();

    for ancestor in [".agent", ".github", "scripts"] {
        for (label, write, force) in [
            ("preview", false, false),
            ("write", true, false),
            ("force", true, true),
        ] {
            let temp = tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
            let config_before = fs::read(repo.join(".jig.toml")).unwrap();
            let outside = temp.path().join(format!(
                "outside-{}-{label}",
                ancestor.trim_start_matches('.')
            ));
            fs::rename(repo.join(ancestor), &outside).unwrap();
            fs::write(outside.join("project-sentinel"), "outside\n").unwrap();
            let protected_relative = match ancestor {
                ".agent" => managed_paths::MANIFEST_PATH
                    .strip_prefix(".agent/")
                    .unwrap(),
                ".github" => "workflows/rust-tests.yml",
                "scripts" => "jig",
                _ => unreachable!(),
            };
            let protected_before = fs::read(outside.join(protected_relative)).unwrap();
            let outside_before = regular_file_tree_snapshot(&outside);
            create_symlink(&outside, &repo.join(ancestor)).unwrap();
            let mut opts = footprint_adopt_opts(&repo, template.path(), true, force);
            opts.write = write;

            let error = run_adopt(opts).unwrap_err().to_string();

            assert!(
                error.contains("is a symlink"),
                "{ancestor}/{label}: {error}"
            );
            assert_eq!(fs::read(repo.join(".jig.toml")).unwrap(), config_before);
            assert_eq!(
                fs::read(outside.join(protected_relative)).unwrap(),
                protected_before,
                "{ancestor}/{label} changed an outside managed path"
            );
            assert_eq!(
                fs::read_to_string(outside.join("project-sentinel")).unwrap(),
                "outside\n"
            );
            assert_eq!(regular_file_tree_snapshot(&outside), outside_before);
            assert!(
                fs::symlink_metadata(repo.join(ancestor))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
    }
}
