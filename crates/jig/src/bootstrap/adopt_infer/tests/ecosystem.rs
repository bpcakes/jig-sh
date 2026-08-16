use super::*;

#[test]
fn workspace_glob_segment_match_supports_multiple_stars() {
    assert!(segment_matches("*-app-*", "demo-app-web"));
    assert!(segment_matches("app-*-web", "app-demo-web"));
    assert!(!segment_matches("app-*-web", "app-demo-api"));
}

#[test]
fn sqlx_detection_includes_cargo_sqlx_commands() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/sqlx.yml"),
        "steps:\n  - run: cargo sqlx prepare --check\n",
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(sqlx.enabled.value);
    assert_eq!(
        sqlx.migration_dir
            .as_ref()
            .map(|value| value.value.as_str()),
        Some("migrations")
    );
    assert_eq!(
        sqlx.check_command
            .as_ref()
            .map(|value| value.value.as_str()),
        Some(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' cargo sqlx prepare --check -- --all-targets"
        )
    );
    assert!(
        sqlx.signals
            .iter()
            .any(|signal| signal == "cargo sqlx command")
    );
}

#[test]
fn sqlx_check_command_uses_workspace_flag_for_cargo_workspaces() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert_eq!(
        sqlx.check_command
            .as_ref()
            .map(|value| value.value.as_str()),
        Some(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' cargo sqlx prepare --check --workspace -- --all-targets"
        )
    );
}

#[test]
fn sqlx_detection_ignores_benign_cargo_sqlx_mentions() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/lib.rs"),
        "/// Example: sqlx::migrate!();\n// sqlx::migrate!();\n/* sqlx::migrate!(); */\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("notes.toml"),
        "# run cargo sqlx prepare manually if needed\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("script.sh"),
        "# cargo sqlx prepare --check\nnpm test # cargo sqlx prepare --check\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "steps:\n  - run: npm test # cargo sqlx prepare --check\n",
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(!sqlx.enabled.value);
    assert!(
        sqlx.signals
            .iter()
            .any(|signal| { signal.contains("no SQLx signals detected") })
    );
}

#[test]
fn root_named_like_skipped_dir_is_still_scanned() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(&root);

    assert_eq!(inference.rust_crate_roots, vec!["."]);
}

#[test]
fn nested_package_manager_conflicts_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/api")).unwrap();
    fs::write(temp.path().join("apps/web/package-lock.json"), "{}").unwrap();
    fs::write(temp.path().join("packages/api/pnpm-lock.yaml"), "").unwrap();

    let mut warnings = Vec::new();
    let manager = infer_package_manager(temp.path(), &mut warnings);

    assert_eq!(manager.as_deref(), Some("npm"));
    assert!(
        warnings
            .iter()
            .any(|warning| { warning.contains("multiple package manager lockfiles detected") })
    );
}

#[test]
fn root_package_manager_conflicts_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("bun.lock"), "").unwrap();
    fs::write(temp.path().join("package-lock.json"), "{}").unwrap();

    let mut warnings = Vec::new();
    let manager = infer_package_manager(temp.path(), &mut warnings);

    assert_eq!(manager.as_deref(), Some("bun"));
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("multiple root package manager lockfiles detected")
        })
    );
}

#[test]
fn npm_shrinkwrap_is_inferred_and_precedes_package_lock() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("npm-shrinkwrap.json"), "{}").unwrap();

    let mut warnings = Vec::new();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );

    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["npm-shrinkwrap.json"]);
    assert!(warnings.is_empty());

    fs::write(temp.path().join("package-lock.json"), "{}").unwrap();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );
    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["npm-shrinkwrap.json"]);

    fs::remove_file(temp.path().join("npm-shrinkwrap.json")).unwrap();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );
    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["package-lock.json"]);

    fs::remove_file(temp.path().join("package-lock.json")).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(temp.path().join("apps/web/npm-shrinkwrap.json"), "{}").unwrap();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );
    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["apps/web/npm-shrinkwrap.json"]);
}

#[test]
fn default_branch_prefers_known_origin_refs_over_current_branch() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();
    git(
        temp.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .unwrap();

    let mut warnings = Vec::new();
    assert_eq!(
        infer_default_branch(temp.path(), &mut warnings).as_deref(),
        Some("main")
    );
}

#[test]
fn default_branch_warns_when_multiple_origin_candidates_exist() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();
    git(
        temp.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .unwrap();
    git(
        temp.path(),
        ["update-ref", "refs/remotes/origin/master", "HEAD"],
    )
    .unwrap();

    let mut warnings = Vec::new();
    assert_eq!(
        infer_default_branch(temp.path(), &mut warnings).as_deref(),
        Some("main")
    );
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("multiple origin default branch candidates detected")
        })
    );
}

#[test]
fn default_branch_does_not_infer_unknown_current_branch() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();

    let mut warnings = Vec::new();
    assert_eq!(infer_default_branch(temp.path(), &mut warnings), None);
    assert!(warnings.iter().any(|warning| {
        warning.contains("current branch feature is not a known default branch name")
    }));
}

#[test]
fn default_branch_infers_known_local_head_without_origin() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "main"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();

    let mut warnings = Vec::new();
    assert_eq!(
        infer_default_branch(temp.path(), &mut warnings).as_deref(),
        Some("main")
    );
}

#[test]
fn default_branch_ignores_malformed_origin_head() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();
    git(
        temp.path(),
        [
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/heads/feature",
        ],
    )
    .unwrap();

    let mut warnings = Vec::new();
    assert_eq!(infer_default_branch(temp.path(), &mut warnings), None);
}

#[test]
fn sqlx_detection_reports_nested_and_multiple_migration_dirs() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("crates/api/migrations/20240101_init")).unwrap();
    fs::create_dir_all(temp.path().join("services/billing/migrations")).unwrap();
    fs::write(
        temp.path()
            .join("crates/api/migrations/20240101_init/up.sql"),
        "select 1;",
    )
    .unwrap();
    fs::write(
        temp.path().join("services/billing/migrations/0001.sql"),
        "select 1;",
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(sqlx.enabled.value);
    assert_eq!(
        sqlx.migration_dirs.value,
        vec![
            "crates/api/migrations".to_string(),
            "services/billing/migrations".to_string(),
        ]
    );
    assert_eq!(
        sqlx.migration_dir
            .as_ref()
            .map(|value| value.value.as_str()),
        Some("crates/api/migrations")
    );
    assert!(sqlx.signals.iter().any(|signal| {
        signal
            == "migration directories detected: crates/api/migrations, services/billing/migrations"
    }));
    assert!(warnings.iter().any(|warning| {
        warning.contains("multiple migration directories detected")
            && warning.contains("crates/api/migrations")
    }));
}

#[test]
fn migration_dir_ignores_non_migration_sql_snippets() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("migrations/archive")).unwrap();
    fs::write(
        temp.path().join("migrations/archive/old_dump.sql"),
        "select 1;",
    )
    .unwrap();
    fs::write(temp.path().join("migrations/README.sql"), "notes").unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(!sqlx.enabled.value);
    assert!(sqlx.migration_dirs.value.is_empty());
}
