#[cfg(unix)]
#[test]
fn failed_loop_tick_and_dispatch_exit_nonzero_after_json_output() {
    for args in [
        vec!["loop", "tick", "--workflow", "failing-task", "--json"],
        vec!["loop", "dispatch", "--json"],
    ] {
        let repo = tempdir().unwrap();
        write_failing_loop_repo(repo.path());
        let output = jig()
            .current_dir(repo.path())
            .env("JIG_CODEX_BIN", repo.path().join("missing-codex"))
            .args(args)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false, "{value:#}");
        assert_eq!(value["status"], "failed", "{value:#}");
    }
}
#[test]
fn loop_acknowledge_occurrence_has_human_and_json_contracts() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let runtime_dir = repo.path().join(".agent/runtime/loop");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 2,
            "occurrences": {
                "nightly@100": {
                    "occurrence_id": "nightly@100",
                    "workflow_id": "nightly",
                    "scheduled_at_ms": 100,
                    "owner": "owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "finished_at_ms": 200,
                    "status": "needs_attention"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let human = jig()
        .current_dir(repo.path())
        .args([
            "loop",
            "acknowledge-occurrence",
            "--occurrence",
            "nightly@100",
        ])
        .output()
        .unwrap();
    assert!(human.status.success(), "{human:?}");
    assert!(human.stderr.is_empty(), "{human:?}");
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("Loop acknowledge-occurrence: acknowledged"));
    assert!(human.contains("Occurrence: nightly@100"));

    let json = jig()
        .current_dir(repo.path())
        .args([
            "loop",
            "acknowledge-occurrence",
            "--occurrence",
            "nightly@100",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(json.status.success(), "{json:?}");
    assert!(json.stderr.is_empty(), "{json:?}");
    let json: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["command"], "loop acknowledge-occurrence");
    assert_eq!(json["occurrence_id"], "nightly@100");
    assert_eq!(json["changed"], false);
    assert_eq!(json["occurrence"]["status"], "acknowledged");
}

#[cfg(unix)]
fn write_failing_loop_repo(root: &Path) {
    write_info_commands_repo(root);
    fs::create_dir_all(root.join("tasks")).unwrap();
    fs::write(root.join("tasks/task.md"), "Review the repository.\n").unwrap();
    let config = fs::read_to_string(root.join(".jig.toml")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "failing-task"
kind = "codex_task"
schedule = "* * * * *"
prompt_file = "tasks/task.md"
checkout = "repo"
"#
        ),
    )
    .unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "fixture@example.com"],
        vec!["config", "user.name", "Fixture"],
        vec!["add", "."],
        vec!["commit", "-m", "fixture"],
    ] {
        let output = Command::new("git")
            .current_dir(root)
            .args(&args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
