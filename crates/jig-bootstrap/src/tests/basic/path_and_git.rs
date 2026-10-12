use super::*;

#[test]
fn bootstrap_invocation_cwd_rejects_invalid_env_values() {
    let _guard = lock_env();
    let relative = EnvVarGuard::set(path::INVOCATION_CWD_ENV, "relative");
    let error = path::bootstrap_invocation_cwd().unwrap_err().to_string();
    assert!(error.contains("JIG_INVOKE_CWD must be an absolute path"));
    drop(relative);

    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing");
    let _missing = EnvVarGuard::set(path::INVOCATION_CWD_ENV, missing.as_os_str());
    let error = path::bootstrap_invocation_cwd().unwrap_err().to_string();
    assert!(error.contains("JIG_INVOKE_CWD is not a directory"));
}

#[test]
fn init_rejects_parent_components_before_answers_or_directory_creation() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("caller");
    fs::create_dir(&invocation).unwrap();
    fs::create_dir(invocation.join("existing")).unwrap();
    fs::write(invocation.join("existing/sentinel.txt"), "preserve\n").unwrap();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());

    for force in [false, true] {
        for requested in ["missing/../existing", "missing/.."] {
            let opts = InitOpts {
                path: PathBuf::from(requested),
                scaffold: ScaffoldOpts::default(),
                template: None,
                template_mode: None,
                vcs_ref: None,
                force,
                defaults: false,
                no_input: true,
                no_vault: true,
                answers: AnswerOpts {
                    answers_file: Some(invocation.join("answers-that-must-not-be-read.toml")),
                    ..AnswerOpts::default()
                },
            };

            for error in [
                preflight_init_destination(&opts).unwrap_err(),
                run_init(opts).unwrap_err(),
            ] {
                let error = error.to_string();
                assert!(
                    error.contains("must not contain '..'"),
                    "{requested}: {error}"
                );
                assert!(
                    !error.contains("answers-that-must-not-be-read"),
                    "{requested}: {error}"
                );
            }
            assert!(!invocation.join("missing").exists());
            assert_eq!(
                fs::read_to_string(invocation.join("existing/sentinel.txt")).unwrap(),
                "preserve\n"
            );
        }
    }
}

#[test]
fn init_and_adopt_resolve_relative_bootstrap_paths_from_invocation_cwd() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let invocation = temp.path().join("caller");
    let other = temp.path().join("other");
    let template = invocation.join("template");
    fs::create_dir_all(&invocation).unwrap();
    fs::create_dir_all(&other).unwrap();
    copy_dir_recursive(
        &template_repo_root().join("templates"),
        &template.join("templates"),
    );
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);

    run_init(InitOpts {
        path: PathBuf::from("new-repo"),
        scaffold: ScaffoldOpts::default(),
        template: Some("template".into()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();
    assert!(invocation.join("new-repo/.jig.toml").exists());

    fs::create_dir_all(invocation.join("existing-repo")).unwrap();
    run_adopt(AdoptOpts {
        components: Default::default(),
        path: PathBuf::from("existing-repo"),
        template: Some("template".into()),
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
    assert!(invocation.join("existing-repo/.jig.toml").exists());

    run_update(UpdateOpts {
        path: PathBuf::from("existing-repo"),
        template: Some("template".into()),
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
}

#[test]
fn run_init_renders_empty_agent_tooling_lists_as_toml_arrays() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[agent_tooling.codex]
marketplaces = []
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("marketplaces = []"));
    let ctx = jig_context::RepoContext::load_from(&destination).unwrap();
    assert!(ctx.codex_marketplaces().is_empty());
}

#[test]
fn run_init_preserves_an_authored_repository_from_its_answers_file() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    let mut authored = authored_mixed_repository_config();
    let table = authored.as_table_mut().unwrap();
    table.insert("repo_name".into(), toml::Value::String("demo".into()));
    table.insert("sqlx_enabled".into(), toml::Value::Boolean(false));
    table.insert("schema_dump_enabled".into(), toml::Value::Boolean(false));
    fs::write(&answers_file, toml::to_string_pretty(&authored).unwrap()).unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered =
        toml::from_str::<toml::Value>(&fs::read_to_string(destination.join(".jig.toml")).unwrap())
            .unwrap();
    let targets = rendered["repository"]["actions"]
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
    assert!(targets.contains(&"api:verify-custom".to_owned()));
    assert!(targets.contains(&"worker:verify-custom".to_owned()));
    assert_eq!(
        rendered["commands"]["api_verify_command"].as_str(),
        Some("go test ./...")
    );
    assert_eq!(
        rendered["commands"]["worker_verify_command"].as_str(),
        Some("cargo test -p worker")
    );
    assert_eq!(
        rendered["commands"]["release_command"].as_str(),
        Some("just release")
    );
}

#[test]
fn run_init_renders_empty_agent_tooling_plugin_lists_as_toml_arrays() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"
plugins = []
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("plugins = []"));
    let ctx = jig_context::RepoContext::load_from(&destination).unwrap();
    assert_eq!(ctx.codex_marketplaces().len(), 1);
    assert!(ctx.codex_marketplaces()[0].plugins.is_empty());
}

#[test]
fn run_init_falls_back_only_for_unsupported_git_branch_flag() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nprevious=\nfor arg in \"$@\"; do\n  if [ \"$previous\" = \"init\" ] && [ \"$arg\" = \"-b\" ]; then\n    printf 'error: unknown switch `b`\\n' >&2\n    exit 129\n  fi\n  previous=$arg\ndone\nexec git \"$@\"\n",
                log_path.display()
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(jig_git::GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination,
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            default_branch: Some("trunk".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains(" init -b trunk"));
    assert!(log.lines().any(|line| line.ends_with(" init")));
    assert!(log.contains(" symbolic-ref HEAD refs/heads/trunk"));
}

#[test]
fn run_init_surfaces_git_branch_init_failures() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nprevious=\nfor arg in \"$@\"; do\n  if [ \"$previous\" = \"init\" ] && [ \"$arg\" = \"-b\" ]; then\n    printf 'fatal: repository storage is broken\\n' >&2\n    exit 1\n  fi\n  previous=$arg\ndone\nexec git \"$@\"\n",
                log_path.display()
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(jig_git::GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let error = run_init(InitOpts {
        path: temp.path().join("repo"),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("git init -b main failed"), "{error}");
    assert!(error.contains("repository storage is broken"), "{error}");
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains(" init -b main"));
    assert!(!log.contains(" symbolic-ref HEAD refs/heads/main"));
}

#[test]
fn adopt_with_real_template_runs_destination_tasks() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
    assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
    assert!(!repo.join("scripts/add-migration.sh").exists());
    assert!(
        !repo
            .join("scripts/check-migration-immutability.sh")
            .exists()
    );
    let launcher = fs::read_to_string(repo.join("scripts/jig")).unwrap();
    assert!(launcher.contains("cd \"$ROOT_DIR\""));
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn adopt_keeps_project_owned_build_and_lint_configuration() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    let project_owned_files = [
        (
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = []\n",
        ),
        (
            "clippy.toml",
            "# Project-owned Clippy policy\ncognitive-complexity-threshold = 40\n",
        ),
        ("Makefile", "project-owned:\n\t@true\n"),
    ];
    for (relative, contents) in project_owned_files {
        fs::write(repo.join(relative), contents).unwrap();
    }

    run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
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

    for (relative, contents) in project_owned_files {
        assert_eq!(fs::read_to_string(repo.join(relative)).unwrap(), contents);
    }
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(!answers.contains("makefile_enabled"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(&format!(
        r#""contract_version": {}"#,
        jig_context::CURRENT_CONTRACT_VERSION
    )));
    assert!(!contract.contains("jig_version"));
    assert!(contract.contains(r#""kind": "command""#));
    assert!(!contract.contains("jig.run_target"));
}
