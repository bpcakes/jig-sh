#[test]
fn schema_git_commands_scrub_repository_redirects() {
    use std::ffi::OsStr;

    let mut command = Command::new("git");
    command
        .env("GIT_DIR", "elsewhere/.git")
        .env("GIT_WORK_TREE", "elsewhere")
        .env("GIT_INDEX_FILE", "elsewhere/index")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.worktree")
        .env("GIT_CONFIG_VALUE_0", "elsewhere");

    configure_known_root_git_environment(&mut command);

    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
    ] {
        assert_eq!(
            command
                .get_envs()
                .find(|(candidate, _)| *candidate == OsStr::new(name))
                .map(|(_, value)| value),
            Some(None),
            "{name} was not scrubbed"
        );
    }
}

#[test]
fn schema_check_supervises_timeout_and_descendant_cleanup() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("schema-descendant-survived");
    write_schema_policy_repo(
        temp.path(),
        &format!("(sleep 2; printf survived > '{}') & wait", marker.display()),
        Some(1),
    );
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = schema_check(&ctx).unwrap_err().to_string();

    assert!(error.contains("timed out after 1 seconds"), "{error}");
    std::thread::sleep(std::time::Duration::from_millis(1_250));
    assert!(
        !marker.exists(),
        "schema timeout left a configured-command descendant running"
    );
}

#[test]
fn schema_check_preserves_pre_start_cancellation() {
    struct Cancelled;

    impl crate::execution::ExecutionObserver for Cancelled {}

    impl crate::execution::ExecutionCancellation for Cancelled {
        fn cancelled(&self) -> bool {
            true
        }
    }

    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "exit 99", None);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = schema_check_with_observer(&ctx, &mut Cancelled).unwrap_err();

    assert!(matches!(error, ExecutionCommandError::CancelledBeforeStart));
}

#[test]
fn check_rust_file_loc_reports_oversized_tracked_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(
        temp.path().join("crates/app/src/large.rs"),
        "fn example() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &RustFileLocInput {
            staged: false,
            changed_against: None,
            all: true,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/large.rs"))
    );
}

#[test]
fn check_rust_file_loc_reports_oversized_staged_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(
        temp.path().join("crates/app/src/staged.rs"),
        "fn staged() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &RustFileLocInput {
            staged: true,
            changed_against: None,
            all: false,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/staged.rs"))
    );
}

#[test]
fn check_rust_file_loc_reports_oversized_changed_against_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(temp.path().join("crates/app/src/lib.rs"), "fn small() {}\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let base = super::git_text(temp.path(), &["rev-parse", "HEAD"]).unwrap();
    fs::write(
        temp.path().join("crates/app/src/large.rs"),
        "fn changed() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "large", "-q"]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &RustFileLocInput {
            staged: false,
            changed_against: Some(base.trim().to_string()),
            all: false,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/large.rs"))
    );
}
