use super::*;
use crate::policy::sqlx::check_non_test;

fn run_git(root: &std::path::Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap()
            .success(),
        "git {args:?}"
    );
}

/// `git ls-files` reports the index, so a file deleted from the worktree but
/// not yet staged stays in the listing throughout an ordinary refactor.
#[test]
fn both_surfaces_skip_paths_deleted_from_the_worktree() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("crates/app/src")).unwrap();
    TestRepoBuilder::new(temp.path())
        .config("rust_crate_roots = [\"crates\"]\nrust_test_command = \"cargo test\"\n")
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .write();
    let source = "pub async fn load(pool: &sqlx::PgPool) {\n    let _ = sqlx::query(\"SELECT 1\").fetch_one(pool).await;\n}\n";
    for name in ["kept.rs", "removed.rs"] {
        fs::write(temp.path().join("crates/app/src").join(name), source).unwrap();
    }
    run_git(temp.path(), &["init", "-q"]);
    run_git(temp.path(), &["add", "-A"]);
    fs::remove_file(temp.path().join("crates/app/src/removed.rs")).unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let checked = check_non_test(&ctx).unwrap();
    assert_eq!(checked["non_test_count"], 1);
    let generated = generate_todo(&ctx, &SqlxTodoInput { output: None }).unwrap();
    assert_eq!(generated["non_test_count"], 1);
    let body = fs::read_to_string(temp.path().join("docs/sqlx-unchecked-queries-todo.md")).unwrap();
    assert!(body.contains("- [ ] `crates/app/src/kept.rs:2`"), "{body}");
    assert!(!body.contains("removed.rs"), "{body}");
}
