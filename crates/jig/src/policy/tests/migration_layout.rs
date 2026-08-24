use super::*;

fn write_versioned_sqlx_policy_repo(root: &Path) {
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    TestRepoBuilder::new(root)
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
}

#[test]
fn contract_validation_rejects_migration_add_for_versioned_artifacts() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
harness_footprint = "minimal"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
sqlx_check_command = "true"
"#,
        )
        .required_commands(["sqlx_check_command"])
        .tools([
            json!({
                "name": tool::SQLX_CHECK,
                "kind": kind::COMMAND,
                "description": "SQLx check.",
                "command": "sqlx_check_command"
            }),
            json!({
                "name": tool::MIGRATION_ADD,
                "kind": kind::NATIVE,
                "description": "Add migration."
            }),
        ])
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = validate_contract(&ctx).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured Rust migration layout does not permit flat migration stubs")
    );
    assert!(error.to_string().contains("jig update --recopy"));
}

#[test]
fn contract_validation_rejects_command_backed_migration_add_for_versioned_artifacts() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
harness_footprint = "minimal"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
migration_add_command = "printf migration"
"#,
        )
        .required_commands(["migration_add_command"])
        .tool(json!({
            "name": tool::MIGRATION_ADD,
            "kind": kind::COMMAND,
            "description": "Add migration.",
            "command": "migration_add_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = validate_contract(&ctx).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured Rust migration layout does not permit flat migration stubs")
    );
}

#[test]
fn contract_validation_preserves_legacy_required_migration_tool_mapping() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
harness_footprint = "minimal"
sqlx_enabled = false
migration_add_command = "printf migration"
"#,
        )
        .contract_version(2)
        .required_commands(["migration_add_command"])
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = validate_contract(&ctx).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Missing required jig tool definition: jig.migration_add")
    );
}

#[test]
fn migration_immutability_remains_recursive_for_both_layouts() {
    for layout in ["flat_migrations", "versioned_artifacts"] {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path())
            .config(format!(
                r#"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "{layout}"
"#
            ))
            .write();
        init_git(temp.path());
        fs::create_dir_all(temp.path().join("schema/v1/nested")).unwrap();
        fs::write(
            temp.path().join("schema/v1/nested/install.sql"),
            "select 1;\n",
        )
        .unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "baseline", "-q"]);

        fs::write(
            temp.path().join("schema/v1/nested/install.sql"),
            "select 2;\n",
        )
        .unwrap();
        fs::create_dir_all(temp.path().join("schema/v2/nested")).unwrap();
        fs::write(
            temp.path().join("schema/v2/nested/install.sql"),
            "select 3;\n",
        )
        .unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "change", "-q"]);

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let output = check_migration_immutability(
            &ctx,
            &MigrationImmutabilityInput {
                changed_against: "HEAD~1".into(),
            },
        )
        .unwrap();
        let violations = output["violations"].as_array().unwrap();

        assert_eq!(violations.len(), 1, "layout {layout}: {output}");
        assert!(
            violations[0]
                .as_str()
                .unwrap()
                .contains("schema/v1/nested/install.sql"),
            "layout {layout}: {output}"
        );
        assert!(
            !output.to_string().contains("schema/v2/nested/install.sql"),
            "layout {layout}: {output}"
        );
    }
}

#[test]
fn migration_add_rejects_versioned_artifacts_before_creating_files() {
    let temp = tempdir().unwrap();
    write_versioned_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "create users").unwrap_err();

    assert_eq!(
        error.to_string(),
        "sqlx migration add requires rust_migration_layout = \"flat_migrations\"; this repository has rust_migration_layout = \"versioned_artifacts\""
    );
    assert!(!temp.path().join("schema").exists());
}
