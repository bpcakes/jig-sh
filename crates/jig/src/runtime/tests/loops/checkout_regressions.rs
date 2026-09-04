#[cfg(unix)]
#[test]
fn scheduled_repo_checkouts_are_serialized_and_not_reported_as_worktrees() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(temp.path().join("tasks/nightly.md"), "Inspect the repo.\n").unwrap();
    let config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "repo-task"
kind = "codex_task"
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/nightly.md"
checkout = "repo"

[[loop.workflows]]
id = "repo-task-2"
kind = "codex_task"
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/nightly.md"
checkout = "repo"
"#
        ),
    )
    .unwrap();
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);
    let codex_path = bin.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
cat >/dev/null
printf 'repo task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["executed_count"], 2, "{output:#}");
    for action in output["actions"].as_array().unwrap() {
        assert_eq!(action["tick"]["lease"]["key"], "checkout:repo");
        assert_eq!(action["tick"]["actions"][0]["checkout"]["mode"], "repo");
        assert!(
            action["occurrence"]["worktree"].is_null(),
            "the repository root is not a retained worktree: {output:#}"
        );
    }
}

#[cfg(unix)]
#[test]
fn dispatch_stops_after_repo_task_changes_the_authoritative_revision() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("tasks")).unwrap();
    fs::write(temp.path().join("tasks/first.md"), "First authority prompt.\n").unwrap();
    fs::write(temp.path().join("tasks/second.md"), "Original second prompt.\n").unwrap();
    let base_config = fs::read_to_string(temp.path().join(".jig.toml")).unwrap();
    let workflows = r#"
[[loop.workflows]]
id = "first-repo-task"
kind = "codex_task"
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/first.md"
checkout = "repo"

[[loop.workflows]]
id = "second-repo-task"
kind = "codex_task"
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/second.md"
checkout = "repo"
"#;
    fs::write(
        temp.path().join(".jig.toml"),
        format!("{base_config}{workflows}"),
    )
    .unwrap();
    git_ok(temp.path(), ["init"]);
    git_ok(temp.path(), ["config", "user.email", "fixture@example.com"]);
    git_ok(temp.path(), ["config", "user.name", "Fixture"]);
    git_ok(temp.path(), ["add", "."]);
    git_ok(temp.path(), ["commit", "-m", "fixture"]);

    let updated_config = bin.path().join("updated-jig.toml");
    fs::write(
        &updated_config,
        format!(
            r#"{base_config}
[[loop.workflows]]
id = "first-repo-task"
kind = "codex_task"
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/first.md"
checkout = "repo"

[[loop.workflows]]
id = "second-repo-task"
kind = "codex_task"
enabled = false
schedule = "* * * * *"
timezone = "UTC"
prompt_file = "tasks/second.md"
sandbox = "read-only"
checkout = "repo"
"#
        ),
    )
    .unwrap();
    let unexpected_second_worker = bin.path().join("second-worker-ran");
    let codex_path = bin.path().join("codex-task-stub.sh");
    write_codex_stub(
        &codex_path,
        r#"#!/bin/sh
prompt=$(cat)
case "$prompt" in
  *"First authority prompt."*)
    cp "$JIG_TEST_UPDATED_CONFIG" .jig.toml
    printf 'Changed second prompt.\n' > tasks/second.md
    git add .jig.toml tasks/second.md
    git commit -m 'change dispatch authority' >/dev/null
    ;;
  *)
    touch "$JIG_TEST_SECOND_WORKER"
    ;;
esac
printf 'repo task complete\n'
"#,
    );
    let _codex = EnvVarGuard::set("JIG_CODEX_BIN", codex_path.as_os_str());
    let _updated = EnvVarGuard::set("JIG_TEST_UPDATED_CONFIG", updated_config.as_os_str());
    let _second = EnvVarGuard::set(
        "JIG_TEST_SECOND_WORKER",
        unexpected_second_worker.as_os_str(),
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Loop(LoopCommand::Dispatch(LoopDispatchRequest {})),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["executed_count"], 1, "{output:#}");
    assert_eq!(output["repository_revision_changed"], true, "{output:#}");
    assert_eq!(output["actions"].as_array().unwrap().len(), 1, "{output:#}");
    assert_eq!(
        output["actions"][0]["tick"]["actions"][0]["checkout"]["head_changed"],
        true,
        "{output:#}"
    );
    assert!(!unexpected_second_worker.exists(), "{output:#}");
}
