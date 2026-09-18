use super::*;

#[test]
fn work_retire_json_reports_partial_completion() {
    assert_partial_completion(true, true);
}

#[test]
fn work_finish_json_reports_partial_completion() {
    assert_partial_completion(false, true);
}

#[test]
fn work_retire_human_error_reports_partial_completion() {
    assert_partial_completion(true, false);
}

fn assert_partial_completion(retiring: bool, json_output: bool) {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"
contract_check_command = "true"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_vec(&json!({
            "contract_version": 3, "jig_version": "0.2.0-beta.1", "tool_namespace": "jig",
            "required_commands": ["contract_check_command"], "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ],
    ] {
        assert!(
            Command::new("git")
                .current_dir(temp.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let started = jig()
        .current_dir(temp.path())
        .args([
            "work",
            "start",
            "--title",
            "ExampleProject closure",
            "--body",
            "Verify partial completion.",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    let started: Value = serde_json::from_slice(&started.stdout).unwrap();
    let plan_id = started["plan"]["plan_id"].as_str().unwrap();
    let state = temp.path().join(".agent/state");
    let receipts_before = fs::read(state.join("receipts.jsonl")).unwrap();
    let sessions_before = fs::read(state.join("sessions.jsonl")).unwrap();
    let lock_path = temp
        .path()
        .join(".agent/.cache/state-locks/receipts.jsonl.lock");
    fs::remove_file(&lock_path).unwrap();
    fs::create_dir(&lock_path).unwrap();
    let mut args = vec![
        "work",
        if retiring { "retire" } else { "finish" },
        "--plan-id",
        plan_id,
    ];
    if retiring {
        args.extend(["--disposition", "obsolete", "--reason", "No longer needed."]);
    }
    if json_output {
        args.push("--json");
    }
    let output = jig().current_dir(temp.path()).args(&args).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    if json_output {
        assert_json_partial_completion(&output, plan_id, &state);
    } else {
        assert_human_partial_completion(&output);
    }
    assert_eq!(
        fs::read(state.join("receipts.jsonl")).unwrap(),
        receipts_before
    );
    assert_eq!(
        fs::read(state.join("sessions.jsonl")).unwrap(),
        sessions_before
    );
    let events_after = fs::read(state.join("plans.jsonl")).unwrap();
    let retried = jig().current_dir(temp.path()).args(args).output().unwrap();
    assert!(!retried.status.success());
    if json_output {
        let error: Value = serde_json::from_slice(&retried.stdout).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("already closed")
        );
        assert!(error.get("partial_completion").is_none());
    }
    assert_eq!(fs::read(state.join("plans.jsonl")).unwrap(), events_after);
}

fn assert_json_partial_completion(output: &std::process::Output, plan_id: &str, state: &Path) {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["error"]["kind"], "command_failed");
    let partial = &payload["partial_completion"];
    assert_eq!(partial["plan_id"], plan_id);
    assert_eq!(partial["plan_state"], "closed");
    assert_eq!(partial["receipt"]["status"], "not_recorded");
    assert_eq!(partial["session_teardown"]["status"], "not_attempted");
    assert_eq!(partial["retry_safe"], false);
    let closes: Vec<Value> = fs::read_to_string(state.join("plans.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| event["event"] == "close")
        .collect();
    assert_eq!(closes.len(), 1);
    assert_eq!(partial["close_event_id"], closes[0]["id"]);
}

fn assert_human_partial_completion(output: &std::process::Output) {
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("already closed by retirement"), "{error}");
    assert!(
        error.contains("Session teardown was not attempted"),
        "{error}"
    );
    assert!(
        error.contains("Repeating finish or retire will be rejected"),
        "{error}"
    );
}
