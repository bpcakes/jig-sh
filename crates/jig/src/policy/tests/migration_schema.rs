use super::*;

#[test]
fn migration_add_rejects_when_sqlx_is_disabled() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "create users").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured SQLx or Go/PostgreSQL migration backend")
    );
}

#[test]
fn migration_add_rejects_names_without_slug_content() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "!!!").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must contain at least one alphanumeric")
    );
}

#[test]
fn schema_check_reports_stale_schema_dump() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && printf 'changed\\n' > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(output.stderr.contains("docs/schema"));
    assert_eq!(
        fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
        "stable\n",
        "a read-only schema check must restore generator output"
    );
    assert!(
        Command::new("git")
            .current_dir(temp.path())
            .args(["status", "--porcelain", "--", "docs/schema"])
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn v6_schema_check_reuses_the_owning_dump_actions_complete_runner() {
    let temp = tempdir().unwrap();
    write_v6_schema_policy_repo(
        temp.path(),
        "printf 'stable\\n' > docs/schema/tables.sql",
        "mkdir -p ../docs/schema && printf '%s\\n' \"$SCHEMA_VALUE\" > ../docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(output.stderr.contains("+changed"), "{}", output.stderr);
    assert_eq!(
        fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
        "stable\n"
    );
}

#[test]
fn v6_schema_check_rejects_an_invalid_dump_runner_environment() {
    let temp = tempdir().unwrap();
    write_v6_schema_policy_repo(temp.path(), "true", "true");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("SCHEMA_VALUE = \"changed\"", "\"A=B\" = \"invalid\"");
    fs::write(config_path, config).unwrap();
    let contract_path = temp.path().join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["actions"][1]["runner"]["environment"] = json!({"A=B": "invalid"});
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = validate_contract(&ctx).unwrap_err().to_string();

    assert!(
        error.contains("environment variable name \"A=B\" is invalid"),
        "{error}"
    );
}

#[test]
fn v6_schema_check_uses_the_dump_runner_schema_output_directory() {
    let temp = tempdir().unwrap();
    write_v6_schema_policy_repo(
        temp.path(),
        "true",
        "mkdir -p \"$JIG_REPO_ROOT/$SCHEMA_DOCS_DIR\" && printf 'changed\\n' > \"$JIG_REPO_ROOT/$SCHEMA_DOCS_DIR/tables.sql\"",
    );
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "SCHEMA_VALUE = \"changed\"",
            "SCHEMA_VALUE = \"changed\", SCHEMA_DOCS_DIR = \"generated/schema\"",
        )
        .replace(
            "schema_dump_enabled = true",
            "schema_dump_enabled = true\nschema_docs_dir = \"generated/schema\"",
        );
    fs::write(config_path, config).unwrap();
    let contract_path = temp.path().join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["actions"][1]["runner"]["environment"]["SCHEMA_DOCS_DIR"] = json!("generated/schema");
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("generated/schema")).unwrap();
    fs::write(temp.path().join("generated/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(output.stderr.contains("generated/schema"));
    assert_eq!(
        fs::read_to_string(temp.path().join("generated/schema/tables.sql")).unwrap(),
        "stable\n"
    );
}

#[test]
fn v6_repository_schema_failure_preserves_the_generator_exit_and_output() {
    let temp = tempdir().unwrap();
    write_v6_schema_policy_repo(
        temp.path(),
        "true",
        "printf 'generator stdout'; printf 'generator stderr' >&2; exit 7",
    );
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        crate::command::RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: vec!["api:schema".into()],
                profile: None,
                affected_base: None,
                comparison: None,
                explain: false,
                fail_fast: false,
                tool: crate::command::ToolRequest::new(None, false),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["run"]["conclusion"], "failure");
    assert_eq!(output["run"]["targets"][0]["conclusion"], "failure");
    assert_eq!(output["run"]["targets"][0]["exit_code"], 7);
    assert_eq!(output["results"][0]["response"]["result"]["exit_status"], 7);
    assert_eq!(
        output["results"][0]["response"]["result"]["stdout"],
        "generator stdout"
    );
    assert_eq!(
        output["results"][0]["response"]["result"]["stderr"],
        "generator stderr"
    );
}

#[test]
fn schema_check_snapshots_dirty_worktrees_without_repository_git_identity() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "cat schema-input > docs/schema/tables.sql");
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    fs::write(temp.path().join("schema-input"), "stable\n").unwrap();
    fs::write(temp.path().join("unrelated.txt"), "baseline\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    git(temp.path(), &["config", "user.name", ""]);
    git(temp.path(), &["config", "user.email", ""]);
    fs::write(temp.path().join("unrelated.txt"), "dirty\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert_eq!(
        fs::read_to_string(temp.path().join("unrelated.txt")).unwrap(),
        "dirty\n"
    );
}

#[test]
fn schema_check_reports_drift_in_an_unborn_repository_without_mutating_it() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && printf 'generated\\n' > docs/schema/tables.sql",
    );
    init_git(temp.path());
    let status_before = Command::new("git")
        .current_dir(temp.path())
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .unwrap()
        .stdout;
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(!temp.path().join("docs/schema/tables.sql").exists());
    let status_after = Command::new("git")
        .current_dir(temp.path())
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(status_after, status_before);
    assert!(
        !Command::new("git")
            .current_dir(temp.path())
            .args(["rev-parse", "--verify", "--quiet", "HEAD"])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn unborn_schema_snapshot_keeps_files_beyond_the_small_diagnostic_output_limit() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && cp zzzz-schema-input docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("inputs")).unwrap();
    for index in 0..500 {
        fs::write(
            temp.path().join(format!(
                "inputs/input_{index:04}_abcdefghijklmnopqrstuvwxyz0123456789.txt"
            )),
            "fixture\n",
        )
        .unwrap();
    }
    fs::write(temp.path().join("zzzz-schema-input"), "generated\n").unwrap();
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(!temp.path().join("docs/schema/tables.sql").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn schema_snapshot_preserves_non_utf8_untracked_paths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && find inputs -type f ! -name .keep -exec cat {} \\; > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::create_dir_all(temp.path().join("inputs")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "").unwrap();
    fs::write(temp.path().join("inputs/.keep"), "").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let input_name = OsString::from_vec(b"input_\xff".to_vec());
    fs::write(temp.path().join("inputs").join(input_name), "generated\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert_eq!(
        fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
        ""
    );
}

#[test]
fn schema_check_preserves_preexisting_schema_edits_without_running_the_generator() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "printf 'generator-ran\\n' > generator-marker && printf 'changed\\n' > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::write(temp.path().join("docs/schema/tables.sql"), "local edit\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("already has uncommitted changes"));
    assert_eq!(
        fs::read_to_string(temp.path().join("docs/schema/tables.sql")).unwrap(),
        "local edit\n"
    );
    assert!(!temp.path().join("generator-marker").exists());
}

#[test]
fn schema_check_discards_new_files_staged_by_the_generator() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && printf 'new\\n' > docs/schema/new.sql && git add docs/schema/new.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(!temp.path().join("docs/schema/new.sql").exists());
    assert!(
        Command::new("git")
            .current_dir(temp.path())
            .args(["status", "--porcelain", "--", "docs/schema"])
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );
}

#[test]
fn schema_check_isolates_unrelated_generator_writes_and_reads_untracked_inputs() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "printf 'mutated\\n' > unrelated.txt && cat schema-input > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::write(temp.path().join("schema-input"), "stable\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert!(!temp.path().join("unrelated.txt").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("schema-input")).unwrap(),
        "stable\n"
    );
}

#[test]
fn schema_check_reads_ignored_dotenv_inputs_in_the_snapshot() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "cat .env > docs/schema/tables.sql");
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    fs::write(temp.path().join(".gitignore"), ".env\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::write(temp.path().join(".env"), "stable\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert_eq!(
        fs::read_to_string(temp.path().join(".env")).unwrap(),
        "stable\n"
    );
}

#[test]
fn schema_check_does_not_overlay_dotenv_from_a_wholly_ignored_directory() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "test ! -e generated/.env && printf 'stable\\n' > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    fs::write(temp.path().join(".gitignore"), "generated/\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    fs::create_dir_all(temp.path().join("generated")).unwrap();
    fs::write(temp.path().join("generated/.env"), "generated\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[test]
fn schema_check_reads_initialized_submodule_worktrees_in_the_snapshot() {
    let dependency = tempdir().unwrap();
    fs::write(dependency.path().join("schema-input"), "stable\n").unwrap();
    init_git(dependency.path());
    git(dependency.path(), &["add", "."]);
    git(dependency.path(), &["commit", "-m", "baseline", "-q"]);

    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "cat vendor/example/schema-input > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(
        temp.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            dependency.path().to_str().unwrap(),
            "vendor/example",
        ],
    );
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[cfg(unix)]
#[test]
fn schema_check_preserves_untracked_symlinks_without_following_them() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(temp.path(), "printf 'stable\\n' > docs/schema/tables.sql");
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    std::os::unix::fs::symlink("missing-target", temp.path().join("unrelated-link")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
    assert!(
        fs::symlink_metadata(temp.path().join("unrelated-link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn controlled_native_output_is_bounded() {
    let mut command = Command::new("bash");
    command.args(["-c", "yes x | head -c 2000000"]);

    let output = controlled_output(
        &mut command,
        Instant::now() + Duration::from_secs(10),
        &|| false,
    )
    .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.contains("[output truncated by Jig]"));
    assert!(output.stdout.len() < 2_000_000);
}

#[test]
fn controlled_native_output_overrides_caller_stdin_with_null() {
    let mut command = Command::new("bash");
    command
        .args(["-c", "read -r ignored || true; printf controlled"])
        .stdin(std::process::Stdio::piped());

    let output = controlled_output(
        &mut command,
        Instant::now() + Duration::from_secs(1),
        &|| false,
    )
    .unwrap();

    assert!(output.status.success());
    assert_eq!(output.stdout, "controlled");
}

#[test]
fn schema_dump_output_overflow_is_a_fatal_failure_with_a_bounded_prefix() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo_with_execution(
        temp.path(),
        "printf 'schema-prefix\\n'; yes x",
        None,
        Some(1_024),
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stdout.starts_with("schema-prefix\n"));
    assert!(output.stdout.len() <= 1_024);
    assert!(output.stderr.contains("1024 byte stdout capture limit"));
}

#[test]
fn schema_check_supervises_timeout_and_descendant_cleanup() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("schema-descendant-survived");
    write_schema_policy_repo_with_timeout(
        temp.path(),
        &format!("(sleep 2; printf survived > '{}') & wait", marker.display()),
        Some(1),
    );
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
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
    write_schema_policy_repo(temp.path(), "exit 99");
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = schema_check_with_observer_and_timeout(
        &ctx,
        None,
        ctx.command_timeout().duration(),
        &mut Cancelled,
    )
    .unwrap_err();

    assert!(matches!(error, ExecutionCommandError::CancelledBeforeStart));
}
