use std::process::Command;

use tempfile::tempdir;

use super::*;
use crate::test_env::TestRepoBuilder;

#[test]
fn local_status_schema_two_omits_the_removed_subsystem() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .repo_name("ExampleProject")
        .write();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let snapshot = snapshot(&ctx).unwrap();
    let keys = snapshot
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys,
        jig_ui::dashboard::STATUS_ROOT_FIELDS
            .iter()
            .copied()
            .collect()
    );
    assert_eq!(snapshot["schema_version"], 2);
    assert_eq!(snapshot["command"], "status");
    assert!(snapshot.get("providers").is_none());
    let encoded = snapshot.to_string();
    assert!(!encoded.contains("work_packages"));
    assert!(!encoded.contains("input_freshness"));

    let typed: jig_ui::dashboard::StatusSnapshot =
        serde_json::from_value(snapshot.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), snapshot);
}

#[test]
fn removed_status_configuration_is_rejected() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
[status]
providers = []
"#,
        )
        .write();

    let error = RepoContext::load_from(temp.path()).unwrap_err();
    let chain = format!("{error:#}");
    assert!(chain.contains("unknown field `status`"), "{chain}");
}

#[test]
fn status_collection_is_read_only() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .repo_name("ExampleProject")
        .write();
    init_git_repo(temp.path());
    let before = git_output(temp.path(), &["status", "--porcelain=v1"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let snapshot = snapshot(&ctx).unwrap();

    assert!(snapshot["repository"].is_object());
    assert!(snapshot["work"].is_object());
    assert!(snapshot["loops"].is_object());
    assert_eq!(
        git_output(temp.path(), &["status", "--porcelain=v1"]),
        before
    );
}

#[test]
fn work_snapshot_propagates_a_non_sticky_typed_cancellation() {
    use std::cell::Cell;

    let root = tempdir().unwrap();
    TestRepoBuilder::new(root.path())
        .repo_name("ExampleProject")
        .config(
            r"
sqlx_enabled = false
",
        )
        .required_commands(["bootstrap_command"])
        .write();
    let ctx = RepoContext::load_from(root.path()).unwrap();
    let calls = Cell::new(0);

    let result = work_snapshot(&ctx, &|| {
        let current = calls.get();
        calls.set(current + 1);
        current == 1
    });
    let Err(error) = result else {
        panic!("non-sticky cancellation was converted into a partial work snapshot")
    };

    assert!(is_status_collection_cancellation(&error));
    assert_eq!(calls.get(), 2);
}

fn init_git_repo(root: &std::path::Path) {
    run_git(root, &["init", "--quiet"]);
    run_git(root, &["config", "user.email", "test@example.com"]);
    run_git(root, &["config", "user.name", "Example Test"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "--quiet", "-m", "fixture"]);
}

fn run_git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}
