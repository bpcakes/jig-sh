use super::*;

#[test]
fn migration_immutability_detects_added_numeric_duplicates_in_the_worktree() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    fs::create_dir(temp.path().join("migrations")).unwrap();
    fs::write(temp.path().join("migrations/1_first.sql"), "SELECT 1;\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "baseline"]);
    fs::write(temp.path().join("migrations/01_second.sql"), "SELECT 2;\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = check_migration_immutability(
        &ctx,
        &MigrationImmutabilityInput {
            changed_against: "HEAD".into(),
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    let violations = output["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 1);
    let message = violations[0].as_str().unwrap();
    assert!(
        message.contains("Duplicate SQLx migration version 1"),
        "{message}"
    );
    assert!(message.contains("migrations/1_first.sql"), "{message}");
    assert!(message.contains("migrations/01_second.sql"), "{message}");
}

#[test]
fn migration_versions_skip_versioned_artifacts_and_goose_owned_sources() {
    let versioned = tempdir().unwrap();
    TestRepoBuilder::new(versioned.path()).config(
        "sqlx_enabled = true\nmigration_dir = \"schema\"\nrust_migration_layout = \"versioned_artifacts\""
    ).write();
    let mixed = tempdir().unwrap();
    write_v6_mixed_migration_policy_repo(mixed.path(), "api");
    for (root, directory) in [
        (versioned.path(), "schema"),
        (mixed.path(), "database/migrations"),
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
        fs::write(root.join(directory).join("1_first.sql"), "").unwrap();
        fs::write(root.join(directory).join("01_second.sql"), "").unwrap();
        let ctx = RepoContext::load_from(root).unwrap();
        assert!(ctx.sqlx_enabled());
        assert!(
            crate::migration_versions::violations(&ctx)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn migration_versions_detect_duplicates_for_the_sqlx_owner_in_a_mixed_repository() {
    let temp = tempdir().unwrap();
    write_v6_mixed_migration_policy_repo(temp.path(), "worker");
    let directory = temp.path().join("database/migrations");
    fs::create_dir_all(&directory).unwrap();
    for name in ["1_first.sql", "01_second.sql"] {
        fs::write(directory.join(name), "").unwrap();
    }
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(ctx.sqlx_enabled());
    assert_eq!(
        ctx.migration_backend().unwrap(),
        Some(jig_context::MigrationBackend::Sqlx)
    );
    assert!(ctx.sqlx_owns_migration_authoring());

    let violations = crate::migration_versions::violations(&ctx).unwrap();

    assert_eq!(violations.len(), 1, "{violations:?}");
    let message = &violations[0];
    assert!(
        message.contains("Duplicate SQLx migration version 1"),
        "{message}"
    );
    assert!(
        message.contains("database/migrations/1_first.sql"),
        "{message}"
    );
    assert!(
        message.contains("database/migrations/01_second.sql"),
        "{message}"
    );
}

#[test]
fn migration_versions_use_only_direct_files_in_the_canonical_directory() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config("sqlx_enabled = true\nmigration_dir = \"database/changes\"")
        .write();
    for path in [
        "database/changes/1_pair.up.sql",
        "database/changes/01_pair.down.sql",
        "database/changes/child/1_other.sql",
        "database/changes/child/01_duplicate.sql",
        "other/migrations/1_other.sql",
        "other/migrations/01_duplicate.sql",
    ] {
        fs::create_dir_all(temp.path().join(path).parent().unwrap()).unwrap();
        fs::write(temp.path().join(path), "").unwrap();
    }
    // A directory with a migration-shaped name is not a SQLx migration file.
    fs::create_dir(temp.path().join("database/changes/1_directory.sql")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(
        crate::migration_versions::violations(&ctx)
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn migration_version_diagnostics_escape_control_characters_in_filenames() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    fs::create_dir(temp.path().join("migrations")).unwrap();
    for name in ["1_first.sql", "01_other\n\u{1b}[31m.sql"] {
        fs::write(temp.path().join("migrations").join(name), "").unwrap();
    }
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let violations = crate::migration_versions::violations(&ctx).unwrap();
    assert_eq!(violations.len(), 1);
    assert!(!violations[0].contains('\n'));
    assert!(!violations[0].contains('\u{1b}'));
    assert!(violations[0].contains("\\n"));
}
