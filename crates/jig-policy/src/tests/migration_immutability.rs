use super::*;

const DOCUMENTATION_CHANGES: &[u8] = b"M\0migrations/AGENTS.md\0D\0migrations/README.md\0T\0migrations/notes/Guide.MD\0R090\0migrations/README.md\0migrations/GUIDE.md\0R100\0migrations/draft.md\0migrations/003_from_draft.sql\0";

fn violations_against_parent(root: &Path) -> Vec<String> {
    let ctx = RepoContext::load_from(root).unwrap();
    let output = check_migration_immutability(
        &ctx,
        &MigrationImmutabilityInput {
            changed_against: "HEAD^".into(),
        },
    )
    .unwrap();
    assert_eq!(output["ok"], json!(output["violations"] == json!([])));
    output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|violation| violation.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn migration_immutability_allows_changes_to_markdown_documentation() {
    let violations = migration_immutability_violations(DOCUMENTATION_CHANGES, true);

    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn migration_immutability_reports_markdown_when_documentation_is_not_exempt() {
    let violations = migration_immutability_violations(DOCUMENTATION_CHANGES, false);

    assert_eq!(violations.len(), 5, "{violations:?}");
    assert!(violations[0].contains("Change detected (M) in migrations/AGENTS.md"));
}

#[test]
fn migration_immutability_still_rejects_turning_a_migration_into_documentation() {
    let bytes = b"R100\0migrations/001_create.up.sql\0migrations/001_create.md\0M\0migrations/001_create.down.sql\0";

    let violations = migration_immutability_violations(bytes, true);

    assert_eq!(violations.len(), 2, "{violations:?}");
    assert!(violations[0].contains(
        "Rename detected (R100): migrations/001_create.up.sql -> migrations/001_create.md"
    ));
    assert!(violations[1].contains("Change detected (M) in migrations/001_create.down.sql"));
}

#[test]
fn migration_immutability_reports_only_migrations_changed_beside_their_guide() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    let migrations = temp.path().join("migrations");
    fs::create_dir_all(&migrations).unwrap();
    fs::write(migrations.join("AGENTS.md"), "# Migrations\n").unwrap();
    fs::write(migrations.join("001_create.up.sql"), "-- initial\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    fs::write(
        migrations.join("AGENTS.md"),
        "# Migrations\n\nUpdated policy.\n",
    )
    .unwrap();
    fs::write(migrations.join("001_create.up.sql"), "-- changed\n").unwrap();
    git(
        temp.path(),
        &["commit", "-qam", "change guide and migration"],
    );

    let violations = violations_against_parent(temp.path());

    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("Change detected (M) in migrations/001_create.up.sql"));
}

#[cfg(unix)]
#[test]
fn migration_immutability_reports_markdown_a_migration_symlink_loads() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    let migrations = temp.path().join("migrations");
    fs::create_dir_all(&migrations).unwrap();
    fs::write(migrations.join("body.md"), "create table items (id int);\n").unwrap();
    symlink("body.md", migrations.join("001_create.up.sql")).unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "initial"]);
    fs::write(migrations.join("body.md"), "drop table items;\n").unwrap();
    git(
        temp.path(),
        &["commit", "-qam", "change the linked migration"],
    );

    let violations = violations_against_parent(temp.path());

    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("Change detected (M) in migrations/body.md"));
}
