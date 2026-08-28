#[test]
fn run_init_uses_native_renderer_and_git() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nexec git \"$@\"\n",
            log_path.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
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
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains(" init -b main"));
    assert!(destination.exists());
    assert!(destination.join(".jig.toml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("[vault]"));
    assert!(answers.contains("scope = \"repo\""));
    assert!(answers.contains("allow_global = false"));
    assert!(answers.contains(
        "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx sqlx prepare --check --workspace -- --workspace --all-targets"
    ));
    assert!(!answers.contains("cargo sqlx prepare --check"));
    let gitignore = fs::read_to_string(destination.join(".gitignore")).unwrap();
    assert!(gitignore.contains("node_modules/"));
    assert!(gitignore.contains(".pnp.*"));
    assert!(gitignore.contains("!.yarn/patches"));
    assert!(gitignore.contains("target/"));
    assert!(gitignore.contains(".agent/.cache/*"));
    assert!(gitignore.contains(".agent/tmp/"));
    assert!(gitignore.contains("# BEGIN JIG MANAGED BLOCK"));
    let attributes = fs::read_to_string(destination.join(".gitattributes")).unwrap();
    assert!(attributes.contains(".agent/state/*.jsonl merge=union"));
    assert!(destination.join("scripts/jig").exists());
    let manifest_paths = managed_manifest_paths(&destination);
    assert!(
        manifest_paths
            .iter()
            .any(|path| path == managed_paths::MANIFEST_PATH)
    );
    assert!(
        manifest_paths
            .iter()
            .all(|path| destination.join(path).is_file())
    );
}

#[test]
fn run_init_sqlx_disabled_defaults_to_harness_only_safe_commands() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
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

    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("Command values are project-owned."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo bootstrap."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test."));
}

#[test]
fn run_init_explicit_harness_only_writes_no_starter_application() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::HarnessOnly),
            ..ScaffoldOpts::default()
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    assert!(!destination.join("Cargo.toml").exists());
    assert!(!destination.join("package.json").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn run_init_rejects_minimal_answers_with_rust_react_before_writes() {
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repo");
    let error = run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            harness_footprint: Some(HarnessFootprint::Minimal),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot combine harness_footprint = \"minimal\""));
    assert!(error.contains("Rust React scaffold"));
    assert!(!destination.exists());
}

#[test]
fn run_init_normalizes_minimal_answers_to_harness_only() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");

    let output = run_init(InitOpts {
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
            repo_name: Some("demo".into()),
            harness_footprint: Some(HarnessFootprint::Minimal),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["scaffold"].is_null());
    assert!(destination.join(".agent/jig-contract.json").is_file());
    assert!(
        fs::read_to_string(destination.join(".jig.toml"))
            .unwrap()
            .contains("harness_footprint = \"minimal\"")
    );
    assert!(!destination.join("scripts/jig").exists());
    assert!(!destination.join("Cargo.toml").exists());
}

#[test]
fn run_init_applies_relative_answers_file_before_scaffold_defaults() {
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
    fs::write(
        invocation.join("answers.toml"),
        r#"repo_name = "file-app"
default_branch = "trunk"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf file-bootstrap"
web_package_manager = "pnpm"

[[frontend_apps]]
name = "portal"
dir = "clients/portal"
coverage_threshold = 77
kind = "vite"
role = "spa"

[dev]
[[dev.apps]]
name = "worker"
kind = "env-port"
command = "cargo run -p worker"
proxy = false
"#,
    )
    .unwrap();
    let _invocation_cwd = EnvVarGuard::set(path::INVOCATION_CWD_ENV, invocation.as_os_str());
    let _cwd = CurrentDirGuard::set(&other);
    let destination = invocation.join("generated");

    let output = run_init(InitOpts {
        path: PathBuf::from("generated"),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some("template".into()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(PathBuf::from("answers.toml")),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(destination.join("apps/file-app-api").is_dir());
    assert!(destination.join("clients/portal/package.json").is_file());
    assert!(!destination.join("web").exists());
    assert_eq!(output["scaffold"]["frontends"][0]["name"], "portal");
    let portal_eslint =
        fs::read_to_string(destination.join("clients/portal/eslint.config.js")).unwrap();
    assert!(portal_eslint.contains(r#"from "../../eslint.config.shared.mjs""#));
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packageManager": "pnpm@"#));
    assert!(destination.join("pnpm-workspace.yaml").is_file());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"file-app\""));
    assert!(answers.contains("default_branch = \"trunk\""));
    assert!(answers.contains("bootstrap_command = \"printf file-bootstrap\""));
    assert!(answers.contains("name = \"worker\""));
    assert!(answers.contains("command = \"cargo run -p worker\""));
    assert!(!answers.contains("[[dev.apps]]\nname = \"api\""));
    assert!(!answers.contains("cargo run -p file-app-api -- --bootstrap-database"));
    let workflow = fs::read_to_string(destination.join(".github/workflows/e2e.yml")).unwrap();
    assert!(workflow.contains("      - \"trunk\""));
    assert_eq!(
        git_stdout(&destination, ["symbolic-ref", "--short", "HEAD"]).unwrap(),
        "trunk"
    );
}

#[test]
fn run_init_cli_answers_override_answers_file_before_scaffold_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "file-app"
default_branch = "file-branch"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "printf file-bootstrap"
web_package_manager = "pnpm"
"#,
    )
    .unwrap();
    let destination = temp.path().join("generated");

    run_init(InitOpts {
        path: destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            repo_name: Some("cli-app".into()),
            default_branch: Some("cli-branch".into()),
            bootstrap_command: Some("printf cli-bootstrap".into()),
            web_package_manager: Some("npm".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(destination.join("apps/cli-app-api").is_dir());
    assert!(!destination.join("apps/file-app-api").exists());
    let workspace_package = fs::read_to_string(destination.join("package.json")).unwrap();
    assert!(workspace_package.contains(r#""packageManager": "npm@"#));
    assert!(!destination.join("pnpm-workspace.yaml").exists());
    let answers = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"cli-app\""));
    assert!(answers.contains("default_branch = \"cli-branch\""));
    assert!(answers.contains("bootstrap_command = \"printf cli-bootstrap\""));
    assert!(!answers.contains("printf file-bootstrap"));
    assert_eq!(
        git_stdout(&destination, ["symbolic-ref", "--short", "HEAD"]).unwrap(),
        "cli-branch"
    );
}

#[test]
fn run_init_rejects_malformed_or_conflicting_answers_before_destination_writes() {
    let temp = tempdir().unwrap();
    let malformed_answers = temp.path().join("malformed.toml");
    fs::write(&malformed_answers, "repo_name = [\n").unwrap();
    let malformed_destination = temp.path().join("malformed-repo");

    let error = run_init(InitOpts {
        path: malformed_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: Vec::new(),
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(malformed_answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("Failed to parse"));
    assert!(!malformed_destination.exists());

    let conflicting_answers = temp.path().join("conflicting.toml");
    fs::write(
        &conflicting_answers,
        r#"repo_name = "demo"
sqlx_enabled = false
schema_dump_enabled = false

[[frontend_apps]]
name = "portal"
dir = "portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let conflicting_destination = temp.path().join("conflicting-repo");
    let error = run_init(InitOpts {
        path: conflicting_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::None),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(conflicting_answers),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot be combined with --frontend-app answers"));
    assert!(!conflicting_destination.exists());

    let unsafe_metadata_destination = temp.path().join("unsafe-metadata-repo");
    let error = run_init(InitOpts {
        path: unsafe_metadata_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some("../sqlx-cache".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("SQLx metadata") && error.contains("must not contain '.' or '..'"),
        "{error:?}"
    );
    assert!(!unsafe_metadata_destination.exists());

    let custom_metadata_destination = temp.path().join("custom-metadata-repo");
    let error = run_init(InitOpts {
        path: custom_metadata_destination.clone(),
        scaffold: ScaffoldOpts {
            preset: Some(ScaffoldPreset::RustReact),
            db: Some(ScaffoldDb::Postgres),
            frontends: vec![parse_scaffold_frontend("web").unwrap()],
            frontend_list: Vec::new(),
        },
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some("db/sqlx-cache".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("pin SQLx 0.9"));
    assert!(error.contains("rust_sqlx_metadata_dir = '.sqlx'"));
    assert!(error.contains("jig adopt"));
    assert!(error.contains("sqlx_check_command"));
    assert!(!custom_metadata_destination.exists());
}
