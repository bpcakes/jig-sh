#[test]
fn adopt_infers_just_recipes_with_default_arguments() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r#"test target="all":
    cargo test --workspace {{target}}
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
}

#[test]
fn adopt_warns_when_wrapper_test_pairs_with_nextest_locked_command() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join(".config/nextest.toml"), "[profile.default]\n").unwrap();
    fs::write(
        repo.join("Justfile"),
        r"test:
    cargo test --workspace
",
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(output["detection_report"]["rust_test_command"], "just test");
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "cargo nextest run --workspace --locked"
    );
    assert!(
        output["detection_report"]["metadata"]["rust_test_locked_command"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("different runners"))
    );
}

#[test]
fn adopt_ignores_make_assignments_that_look_like_rust_recipes() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Makefile"),
        r"test := cargo test --workspace
fmt-check:
	cargo fmt --all -- --check
",
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_fmt_check_command"],
        "make fmt-check"
    );
    assert!(output["detection_report"]["rust_test_command"].is_null());
}

#[test]
fn adopt_infers_nextest_when_no_project_wrapper_exists() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(repo.join(".config/nextest.toml"), "[profile.default]\n").unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(
        output["detection_report"]["rust_test_command"],
        "cargo nextest run --workspace"
    );
    assert_eq!(
        output["detection_report"]["rust_test_locked_command"],
        "cargo nextest run --workspace --locked"
    );
    assert_eq!(
        output["detection_report"]["metadata"]["rust_test_command"]["sources"][0],
        ".config/nextest.toml"
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("api_test_command = \"cargo nextest run --workspace\""));
}

#[test]
fn adopt_keeps_explicit_answers_ahead_of_inference() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("web")).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
sqlx_enabled = false
web_package_manager = "yarn"
rust_test_command = "cargo test --workspace"
frontend_apps = []
"#,
    )
    .unwrap();
    fs::write(
        repo.join("Justfile"),
        r"test:
    cargo nextest run --workspace
",
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            repo_name: Some("from-cli".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("web_package_manager: inferred npm ignored"))
    );
    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("frontend_apps: inferred web ignored"))
    );
    assert!(
        output["adoption_profile"]["overrides"]
            .as_array()
            .unwrap()
            .iter()
            .any(|override_note| override_note
                .as_str()
                .unwrap()
                .contains("rust_test_command: inferred just test ignored"))
    );

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("repo_name = \"from-cli\""));
    assert!(answers.contains("web_package_manager = \"yarn\""));
    assert!(answers.contains("api_test_command = \"cargo test --workspace\""));
    assert!(answers.contains("frontend_apps = []"));
    assert!(!answers.contains("[[frontend_apps]]"));
}

#[test]
fn adopt_answer_file_migration_dir_keeps_sqlx_enabled_when_inference_finds_no_sqlx() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
rust_migration_dir = "migrations"
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
}

#[test]
fn adopt_answer_file_sqlx_disabled_suppresses_inferred_migration_defaults() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join("migrations")).unwrap();
    fs::write(repo.join("migrations/0001_init.sql"), "select 1;").unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
sqlx_enabled = false
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(!answers.contains("rust_migration_dir ="));
}

#[test]
fn adopt_answer_file_schema_dump_disabled_still_uses_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
schema_dump_enabled = false
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
}

#[test]
fn adopt_answer_file_schema_dump_enabled_blocks_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "from-file"
schema_dump_enabled = true
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("schema_dump_enabled implies SQLx"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_cli_sqlx_metadata_dir_blocks_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_infers_root_frontend_app() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("root-web");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{
  "name": "root-web",
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("web_package_manager = \"npm\""));
    assert!(answers.contains("name = \"root-web\""));
    assert!(answers.contains("dir = \".\""));
    assert!(answers.contains("kind = \"vite\""));
    assert!(answers.contains(
        "argv = [\"npm\", \"--prefix=.\", \"--workspace=.\", \"--workspaces=true\", \"--include-workspace-root=true\", \"--global=false\", \"--location=project\", \"--if-present=false\", \"--include=dev\", \"--include=optional\", \"--include=peer\", \"run\", \"dev\"]"
    ));
}

#[test]
fn adopt_defaults_with_migration_dir_keeps_sqlx_enabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("rust_migration_dir = \"migrations\""));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(!answers.contains("schema_dump_command"));
}

#[test]
fn adopt_schema_dump_command_opts_into_schema_dumps() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
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
        answers: AnswerOpts {
            rust_migration_dir: Some("migrations".into()),
            schema_dump_command: Some("scripts/custom-dump-schema.sh".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
    assert!(answers.contains("schema_dump_enabled = true"));
    assert!(answers.contains("schema_dump_command = \"scripts/custom-dump-schema.sh\""));
}

#[test]
fn adopt_defaults_with_schema_dump_enabled_still_requires_sqlx_migration_answer() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
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
            schema_dump_enabled: Some(true),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Missing required answer when sqlx_enabled is true"));
    assert!(error.contains("--rust-migration-dir <dir>"));
}

#[test]
fn adopt_no_input_without_defaults_uses_inferred_no_sqlx_profile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: false,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}
