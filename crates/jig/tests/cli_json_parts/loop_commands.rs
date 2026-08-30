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
fn needs_attention_tick_and_run_exit_nonzero_after_json_output() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let cache = repo.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                "noop-status:item-1": {
                    "key": "noop-status:item-1",
                    "workflow_id": "noop-status",
                    "item_key": "item-1",
                    "attempts": 3,
                    "max_attempts": 3,
                    "last_attempt_ms": 1,
                    "next_eligible_ms": u64::MAX,
                    "exhausted": true,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    for args in [
        vec!["loop", "tick", "--workflow", "noop-status", "--json"],
        vec![
            "loop",
            "run",
            "--workflow",
            "noop-status",
            "--max-ticks",
            "1",
            "--json",
        ],
    ] {
        let output = jig()
            .current_dir(repo.path())
            .args(args)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false, "{value:#}");
        assert_eq!(value["status"], "needs_attention", "{value:#}");
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

#[test]
fn occurrence_reported_as_attention_can_be_acknowledged_directly() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let runtime_dir = repo.path().join(".agent/runtime/loop");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 3,
            "occurrences": {
                "nightly@100": {
                    "occurrence_id": "nightly@100",
                    "workflow_id": "nightly",
                    "scheduled_at_ms": 100,
                    "owner": "stopped-owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "status": "running"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let status = jig()
        .current_dir(repo.path())
        .args(["loop", "status", "--json"])
        .output()
        .unwrap();
    assert!(status.status.success(), "{status:?}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(
        status["needs_attention"]["scheduled_occurrences"][0]["occurrence_id"],
        "nightly@100"
    );

    let acknowledgement = jig()
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
    assert!(acknowledgement.status.success(), "{acknowledgement:?}");
    let acknowledgement: Value = serde_json::from_slice(&acknowledgement.stdout).unwrap();
    assert_eq!(acknowledgement["changed"], true);
    assert_eq!(acknowledgement["occurrence"]["status"], "acknowledged");
}

#[test]
fn loop_clear_attempt_accepts_a_removed_workflow_key() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let cache = repo.path().join(".agent/.cache/loop");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("attempts.json"),
        serde_json::to_vec_pretty(&json!({
            "attempts": {
                "removed-workflow:pr-7": {
                    "key": "removed-workflow:pr-7",
                    "workflow_id": "removed-workflow",
                    "item_key": "pr-7",
                    "attempts": 3,
                    "max_attempts": 3,
                    "last_attempt_ms": 1,
                    "next_eligible_ms": u64::MAX,
                    "exhausted": true,
                    "last_status": "failed"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = jig()
        .current_dir(repo.path())
        .args([
            "loop",
            "clear-attempt",
            "--workflow",
            "removed-workflow",
            "--item",
            "pr-7",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["workflow"]["id"], "removed-workflow");
    assert_eq!(value["workflow"]["configured"], false);
    assert_eq!(value["workflow"]["removed"], true);
    assert_eq!(value["workflow_id"], "removed-workflow");
    assert_eq!(value["item_key"], "pr-7");
    assert_eq!(value["cleared"], true);
    let attempts: Value = serde_json::from_slice(&fs::read(cache.join("attempts.json")).unwrap())
        .unwrap();
    assert_eq!(attempts["attempts"].as_object().unwrap().len(), 0);
}

#[test]
fn loop_clear_attempt_does_not_relabel_removed_workflow_aliases_as_builtin() {
    for workflow_id in ["noop-status", "noop_status"] {
        let repo = tempdir().unwrap();
        write_info_commands_repo(repo.path());
        let cache = repo.path().join(".agent/.cache/loop");
        fs::create_dir_all(&cache).unwrap();
        let attempt_key = format!("{workflow_id}:pr-7");
        fs::write(
            cache.join("attempts.json"),
            serde_json::to_vec_pretty(&json!({
                "attempts": {
                    attempt_key.clone(): {
                        "key": attempt_key,
                        "workflow_id": workflow_id,
                        "item_key": "pr-7",
                        "attempts": 3,
                        "max_attempts": 3,
                        "last_attempt_ms": 1,
                        "next_eligible_ms": u64::MAX,
                        "exhausted": true,
                        "last_status": "failed"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let output = jig()
            .current_dir(repo.path())
            .args([
                "loop",
                "clear-attempt",
                "--workflow",
                workflow_id,
                "--item",
                "pr-7",
                "--json",
            ])
            .output()
            .unwrap();

        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["workflow"]["id"], workflow_id);
        assert_eq!(value["workflow"]["configured"], false);
        assert_eq!(value["workflow"]["removed"], true);
        assert_eq!(value["workflow_id"], workflow_id);
        assert_eq!(value["cleared"], true);
    }
}

#[test]
fn loop_clear_attempt_keeps_builtin_descriptor_when_nothing_was_cleared() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());

    let output = jig()
        .current_dir(repo.path())
        .args([
            "loop",
            "clear-attempt",
            "--workflow",
            "noop_status",
            "--item",
            "missing",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["workflow"]["id"], "noop-status");
    assert_eq!(value["workflow"]["kind"], "noop_status");
    assert_eq!(value["workflow"]["removed"], Value::Null);
    assert_eq!(value["workflow_id"], "noop_status");
    assert_eq!(value["cleared"], false);
}

#[cfg(unix)]
#[test]
fn concurrent_dispatchers_execute_one_due_occurrence_once() {
    let repo = tempdir().unwrap();
    write_info_commands_repo(repo.path());
    let config = fs::read_to_string(repo.path().join(".jig.toml")).unwrap();
    fs::write(
        repo.path().join(".jig.toml"),
        format!(
            r#"{config}
[[loop.workflows]]
id = "scheduled-noop"
kind = "noop_status"
schedule = "0 0 1 1 *"
timezone = "UTC"
"#
        ),
    )
    .unwrap();

    let runtime_dir = repo.path().join(".agent/runtime/loop");
    fs::create_dir_all(&runtime_dir).unwrap();
    let schedule_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(runtime_dir.join("schedule.lock"))
        .unwrap();
    schedule_lock.lock_exclusive().unwrap();
    let spawn_dispatch = || {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .current_dir(repo.path())
            .env_remove("JIG_REPO_ROOT")
            .env_remove("JIG_INVOKE_CWD")
            .env("NO_COLOR", "1")
            .args(["loop", "dispatch", "--json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let mut first = spawn_dispatch();
    let mut second = spawn_dispatch();
    std::thread::sleep(Duration::from_millis(250));
    assert!(first.try_wait().unwrap().is_none());
    assert!(second.try_wait().unwrap().is_none());
    FileExt::unlock(&schedule_lock).unwrap();

    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();

    assert!(first.status.success(), "{first:?}");
    assert!(second.status.success(), "{second:?}");
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    let executed = first["executed_count"].as_u64().unwrap()
        + second["executed_count"].as_u64().unwrap();
    assert_eq!(executed, 1, "first={first:#}\nsecond={second:#}");

    let ledger: Value = serde_json::from_slice(
        &fs::read(repo.path().join(".agent/runtime/loop/schedule.json")).unwrap(),
    )
    .unwrap();
    let occurrences = ledger["occurrences"].as_object().unwrap();
    assert_eq!(occurrences.len(), 1, "{ledger:#}");
    assert_eq!(
        occurrences.values().next().unwrap()["status"],
        "succeeded"
    );
}

#[cfg(unix)]
fn write_failing_loop_repo(root: &Path) {
    write_info_commands_repo(root);
    let gitignore = fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    fs::write(
        root.join(".gitignore"),
        format!("{gitignore}\n.agent/.cache/\n.agent/runtime/\n"),
    )
    .unwrap();
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
