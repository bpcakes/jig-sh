#[test]
fn adopt_preserves_existing_vault_scope_id() {
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
        answers: AnswerOpts::default(),
    })
    .unwrap();
    let first_scope = rendered_vault_scope_id(&repo);

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
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert_eq!(rendered_vault_scope_id(&repo), first_scope);
}

#[test]
fn adopt_reports_legacy_vault_scope_migration_note() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap();

    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("Existing .jig.toml had no [vault] block")
    }));
}

#[test]
fn adopt_rejects_existing_repo_vault_scope_without_scope_id() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].scope_id is required"));
}

#[test]
fn adopt_rejects_malformed_existing_vault_policy() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_1"
allow_global = "false"
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].allow_global"));
    assert!(error.contains("must be a boolean"));

    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = 123
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("[vault].scope"));
    assert!(error.contains("must be a string"));

    fs::write(
        repo.join(".jig.toml"),
        r#"repo_name = "repo"
default_branch = "main"
ci_github_runner = "ubuntu-latest"
jig_version = "0.1.0"
template_source_url = "https://github.com/bpcakes/jig-sh.git"
sqlx_enabled = false
schema_dump_enabled = false
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_1"
unexpected = true
"#,
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: true,
        write: false,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts::default(),
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unknown [vault].unexpected"));
}
