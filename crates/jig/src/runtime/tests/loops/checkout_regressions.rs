#[cfg(unix)]
#[test]
fn scheduled_repo_checkouts_are_serialized_and_not_reported_as_worktrees() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
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
    let codex_path = temp.path().join("codex-task-stub.sh");
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
