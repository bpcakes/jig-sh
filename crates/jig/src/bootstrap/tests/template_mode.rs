use super::*;

#[test]
fn adopt_local_git_template_defaults_to_committed_mode() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

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
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("_template_mode = \"committed\""));
    assert!(answers.contains("_template_local_path = "));
    assert!(
        answers.contains(
            &fs::canonicalize(template.path())
                .unwrap()
                .display()
                .to_string()
        )
    );
}

#[test]
fn adopt_local_git_template_rejects_dirty_committed_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();
    write_test_crate_guide(&repo);

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
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("clean git working tree"));
    assert!(error.contains("Commit or stash template changes"));
}

#[test]
fn update_rejects_legacy_working_tree_template_state() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert(
        TEMPLATE_MODE_KEY.into(),
        TomlValue::String("working-tree".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported legacy template mode 'working-tree'"));
    assert!(error.contains("committed template source"));
}

#[test]
fn update_committed_mode_rejects_switching_local_template_checkout() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    let other_template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let error = run_update(UpdateOpts {
        path: repo,
        template: Some(other_template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot switch template source paths in-place"));
}

#[test]
fn update_default_committed_mode_uses_clean_local_template_head() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    commit_template_root_guide(
        template.path(),
        "# Default Update Marker\n",
        "template update",
    );

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Default Update Marker"));
}

#[test]
fn update_replaces_jig_block_without_overwriting_custom_root_agents() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\nCustom repo guidance.\n",
    )
    .unwrap();

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    commit_template_root_guide(template.path(), "Updated Jig Block\n", "template update");

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Custom repo guidance."));
    assert!(root_guide.contains("Updated Jig Block"));
    assert_eq!(
        root_guide
            .matches("<!-- BEGIN JIG MANAGED BLOCK -->")
            .count(),
        1
    );
}

#[test]
fn update_recopy_normalizes_legacy_schema_dump_true_when_sqlx_disabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.toml");
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert("schema_dump_enabled".into(), TomlValue::Boolean(true));
    answers.insert(
        "bootstrap_command".into(),
        TomlValue::String("cargo fetch".into()),
    );
    answers.insert(
        "rust_fmt_check_command".into(),
        TomlValue::String("cargo fmt --all -- --check".into()),
    );
    answers.insert(
        "rust_clippy_command".into(),
        TomlValue::String("cargo clippy --workspace --all-targets --locked -- -D warnings".into()),
    );
    answers.insert(
        "rust_test_command".into(),
        TomlValue::String("cargo test --workspace".into()),
    );
    answers.insert(
        "rust_test_locked_command".into(),
        TomlValue::String("cargo test --workspace --locked".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
    assert!(answers.contains("schema_dump_enabled = false"));
    assert!(answers.contains("No Cargo.toml found; skipping cargo bootstrap."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo fmt."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo clippy."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test."));
    assert!(answers.contains("No Cargo.toml found; skipping cargo test-locked."));
    assert!(!answers.contains("tool = \"jig.schema_check\""));
}

#[test]
fn update_refuses_managed_file_changes_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let original_mcp = fs::read_to_string(repo.join(".mcp.json")).unwrap();
    fs::write(
        template.path().join("templates/project/.mcp.json.jinja"),
        "{\n  \"changed\": true\n}\n",
    )
    .unwrap();
    git(
        template.path(),
        ["add", "templates/project/.mcp.json.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "template update"]).unwrap();

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Update would overwrite or remove template-managed paths"));
    assert!(error.contains(".mcp.json"));
    assert_eq!(
        fs::read_to_string(repo.join(".mcp.json")).unwrap(),
        original_mcp
    );

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let mcp = fs::read_to_string(repo.join(".mcp.json")).unwrap();
    assert!(mcp.contains("\"changed\": true"));
}

#[test]
fn update_default_committed_mode_rejects_dirty_local_template() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("clean git working tree"));
}

#[test]
fn update_committed_mode_with_vcs_ref_only_updates_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    commit_template_root_guide(template.path(), "# Older Marker\n", "older template");

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    let new_ref = commit_template_root_guide(template.path(), "# Newer Marker\n", "newer template");

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: Some(new_ref.clone()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Newer Marker"));
    assert!(!root_guide.contains("Older Marker"));

    let answers_path = repo.join(".jig.toml");
    assert_eq!(
        read_optional_answer_string(&answers_path, "_commit")
            .unwrap()
            .as_deref(),
        Some(new_ref.as_str())
    );
}
