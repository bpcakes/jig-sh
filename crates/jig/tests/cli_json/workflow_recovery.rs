use super::*;

fn fixture(root: &Path) {
    write_v6_failing_test_repo(root);
    let path = root.join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config["commands"]["api_test_command"] = "printf 'Example target executed\\n'".into();
    config["commands"]
        .as_table_mut()
        .unwrap()
        .insert("bootstrap_command".into(), "true".into());
    let component = json!({"id":"repo", "root":"."});
    let contract = json!({
        "target":{"component":"repo","action":"contract"},
        "intent":"check", "effects":["read_only"], "inputs":[".jig.toml"],
        "runner":{"kind":"native", "operation":"jig.contract_check"},
        "legacy_aliases":["jig.contract_check"]
    });
    let bootstrap = json!({
        "target":{"component":"repo","action":"bootstrap"},
        "intent":"operate", "effects":["worktree","process","external"],
        "runner":{"kind":"command", "command":"bootstrap_command"},
        "legacy_aliases":["jig.bootstrap"]
    });
    config["repository"]["components"]
        .as_array_mut()
        .unwrap()
        .push(toml::Value::try_from(&component).unwrap());
    config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .push(toml::Value::try_from(&contract).unwrap());
    config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .push(toml::Value::try_from(&bootstrap).unwrap());
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["components"]
        .as_array_mut()
        .unwrap()
        .push(component);
    manifest["actions"].as_array_mut().unwrap().push(contract);
    manifest["actions"].as_array_mut().unwrap().push(bootstrap);
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("bootstrap_command"));
    manifest["tools"] = json!([
        {"name":"jig.bootstrap", "kind":"command", "command":"bootstrap_command", "description":"Example bootstrap"},
        {"name":"jig.contract_check", "kind":"native", "description":"Validate contract"}
    ]);
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let mut text = toml::to_string(&config).unwrap();
    text.push_str("\n[[work.gates]]\nid='verify'\nkind='evidence'\nprofile='verify'\n");
    fs::write(path, text).unwrap();
    // A tiny launcher implements the real private handoff without installing a
    // runtime. Its executable path is part of the behavior under test.
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join(".mcp.json"), "{}\n").unwrap();
    fs::write(root.join("scripts/install-jig.sh"), "#!/bin/bash\nexit 1\n").unwrap();
    let quoted = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let root_arg = quoted(root.to_str().unwrap());
    fs::write(
        root.join("scripts/jig"),
        format!(
            "#!/bin/bash\nset -eu\ncd -- {root_arg}\nexec {} --__launcher-contract-version 6 --__launcher-profile runtime --__launcher-repo-root {root_arg} \"$@\"\n",
            quoted(env!("CARGO_BIN_EXE_jig")),
        ),
    )
    .unwrap();
    fs::set_permissions(root.join("scripts/jig"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(root.join("tools")).unwrap();
    // Only actual dependencies are available; there is deliberately no `jig`.
    for (name, path) in [("bash", "/bin/bash"), ("git", "/usr/bin/git")] {
        std::os::unix::fs::symlink(path, root.join("tools").join(name)).unwrap();
    }
}

fn invoke(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(root.join("scripts/jig"))
        .current_dir(root)
        .env_clear()
        .env("PATH", root.join("tools"))
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .unwrap()
}

fn journal(root: &Path) -> Option<Vec<u8>> {
    fs::read(root.join(".agent/state/receipts.jsonl")).ok()
}

fn error_message(output: &std::process::Output, json_output: bool) -> String {
    assert!(!output.status.success());
    if json_output {
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 3, "{value:#}");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"].as_object().unwrap().len(), 2);
        assert!(value["error"]["kind"].is_string());
        assert_eq!(value["exit_status"], output.status.code().unwrap());
        value["error"]["message"].as_str().unwrap().to_owned()
    } else {
        assert!(output.stdout.is_empty());
        String::from_utf8(output.stderr.clone()).unwrap()
    }
}

fn apply(root: &Path, message: &str) -> std::process::Output {
    apply_from(root, root, message)
}

fn apply_from(root: &Path, cwd: &Path, message: &str) -> std::process::Output {
    // Execute the entire displayed command, including its executable unchanged.
    let command = message.lines().last().unwrap().trim();
    Command::new("/bin/bash")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", root.join("tools"))
        .env("NO_COLOR", "1")
        .args(["--noprofile", "--norc", "-c", command])
        .output()
        .unwrap()
}

#[test]
fn launcher_recovery_preserves_repository_ownership_from_another_directory() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("Example 'owned' repository");
    let caller = temp.path().join("Example caller");
    fs::create_dir_all(&caller).unwrap();
    fixture(&root);
    let plan = open(&root, "Example owner");
    for args in [
        vec![
            "work",
            "start",
            "--title",
            "Example",
            "--description",
            "notes",
            "--json",
        ],
        vec!["info", "--summary", "--json"],
        vec!["--summary", "work", "status", "--json"],
        vec![
            "work",
            "check",
            "--plan-id",
            &plan,
            "--tool",
            "api:test",
            "--json",
        ],
        vec![
            "work",
            "check",
            "--plan-id",
            &plan,
            "--tool",
            "api:missing",
            "--json",
        ],
    ] {
        let before = journal(&root);
        let output = Command::new(root.join("scripts/jig"))
            .current_dir(&caller)
            .env_clear()
            .env("PATH", root.join("tools"))
            .args(&args)
            .output()
            .unwrap();
        let message = error_message(&output, true);
        assert_eq!(journal(&root), before);
        assert!(!message.lines().last().unwrap().contains("--__launcher"));
        let retried = apply_from(&root, &caller, &message);
        assert!(retried.status.success(), "{message}\n{retried:?}");
        assert!(!caller.join(".agent").exists());
        if args[0] != "work" || args.contains(&"api:missing") {
            assert_eq!(journal(&root), before);
        }
    }
    assert!(root.join(".agent/state/plans.jsonl").exists());
}

#[test]
fn direct_binary_recovery_preserves_the_invoked_executable() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    let output = jig()
        .current_dir(repo.path())
        .args(["info", "--summary", "--json"])
        .output()
        .unwrap();
    let message = error_message(&output, true);
    let before = journal(repo.path());
    let retried = apply(repo.path(), &message);
    assert!(retried.status.success(), "{message}\n{retried:?}");
    assert_eq!(journal(repo.path()), before);
}

fn open(root: &Path, title: &str) -> String {
    let output = invoke(
        root,
        &[
            "work",
            "start",
            "--title",
            title,
            "--body",
            "Example notes",
            "--json",
        ],
    );
    assert!(output.status.success(), "{:?}", output);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["plan"]["plan_id"].as_str().unwrap().to_owned()
}

#[test]
fn description_recovery_runs_only_on_explicit_retry_and_preserves_shell_literals() {
    for json_output in [false, true] {
        let repo = tempdir().unwrap();
        fixture(repo.path());
        let title = "Example '$(touch SHOULD_NOT_EXIST)' title";
        let mut args = vec![
            "work",
            "start",
            "--title",
            title,
            "--description",
            "Example notes with spaces",
        ];
        if json_output {
            args.push("--json");
        }
        let before = journal(repo.path());
        let output = invoke(repo.path(), &args);
        let message = error_message(&output, json_output);
        assert_eq!(before, journal(repo.path()));
        assert!(!repo.path().join(".agent/state/plans.jsonl").exists());
        let retried = apply(repo.path(), &message);
        assert!(retried.status.success(), "{message}\n{retried:?}");
        assert!(!repo.path().join("SHOULD_NOT_EXIST").exists());
        let records = fs::read_to_string(repo.path().join(".agent/state/plans.jsonl")).unwrap();
        assert!(
            records
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .any(|plan| plan["title"] == title),
            "{records}"
        );
    }
}

#[test]
fn plan_status_recovery_keeps_explicit_plan_even_with_multiple_open_plans() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    let selected = open(repo.path(), "Example first");
    open(repo.path(), "Example second");
    let before = journal(repo.path());
    let output = invoke(
        repo.path(),
        &["work", "status", "--plan-id", &selected, "--json"],
    );
    let message = error_message(&output, true);
    assert_eq!(before, journal(repo.path()));
    let retried = apply(repo.path(), &message);
    assert!(retried.status.success(), "{retried:?}");
    let value: Value = serde_json::from_slice(&retried.stdout).unwrap();
    assert_eq!(value["plan_id"], selected);
    assert!(value.get("gates_ok").is_some());
    assert_eq!(before, journal(repo.path()));
    assert!(
        invoke(repo.path(), &["work", "status", "--json"])
            .status
            .success()
    );
}

#[test]
fn summary_recovery_uses_existing_projection_or_truthful_help_without_auto_execution() {
    for command in ["check", "gates", "evidence", "status"] {
        let repo = tempdir().unwrap();
        fixture(repo.path());
        let plan = open(repo.path(), "Example projection");
        let mut args = vec!["work", command, "--summary", "--json"];
        if command != "status" {
            args.extend(["--plan-id", &plan]);
        }
        let before = journal(repo.path());
        let output = invoke(repo.path(), &args);
        let message = error_message(&output, true);
        assert_eq!(before, journal(repo.path()));
        let retried = apply(repo.path(), &message);
        assert!(retried.status.success(), "{message}\n{retried:?}");
        if command == "status" {
            assert!(message.contains("--help"));
        } else {
            let value: Value = serde_json::from_slice(&retried.stdout).unwrap();
            assert_eq!(value["schema_version"], 1, "{value:#}");
            assert_eq!(value["command"], format!("work {command}"));
            assert!(value["finish_ready"].is_boolean());
        }
        if command != "check" {
            assert_eq!(before, journal(repo.path()));
        }
    }
}

#[test]
fn summary_before_a_command_returns_executable_scoped_help() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    for args in [
        vec!["work", "--summary", "status", "--json"],
        vec!["work", "--json", "--summary", "status"],
        vec!["--json", "--summary", "work", "status"],
    ] {
        let before = journal(repo.path());
        let output = invoke(repo.path(), &args);
        let message = error_message(&output, true);
        assert!(message.ends_with("--help"), "{message}");
        assert!(!message.lines().last().unwrap().contains("--summary"));
        assert!(apply(repo.path(), &message).status.success(), "{message}");
        assert_eq!(before, journal(repo.path()));
    }
}

#[test]
fn info_summary_recovery_respects_semantic_projection_restrictions() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    for subject in [
        vec![],
        vec!["--commands"],
        vec!["components"],
        vec!["profiles"],
        vec!["profile", "verify"],
        vec!["go-version"],
        vec!["freshness"],
    ] {
        let mut args = vec!["info"];
        args.extend(subject);
        args.extend(["--summary", "--json"]);
        let before = journal(repo.path());
        let output = invoke(repo.path(), &args);
        let message = error_message(&output, true);
        assert!(message.ends_with("jig info --help"), "{args:?}: {message}");
        assert!(!message.contains("Suggested retry"), "{message}");
        assert!(apply(repo.path(), &message).status.success(), "{message}");
        assert_eq!(before, journal(repo.path()));
    }
}

#[test]
fn info_summary_recovery_preserves_supported_target_subjects() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    for subject in [
        vec!["workspace"],
        vec!["component", "api"],
        vec!["targets"],
        vec!["target", "api:test"],
    ] {
        let mut args = vec!["info"];
        args.extend(subject);
        let mut invalid = args.clone();
        invalid.extend(["--summary", "--json"]);
        let before = journal(repo.path());
        let output = invoke(repo.path(), &invalid);
        let message = error_message(&output, true);
        assert!(message.contains("Suggested retry"), "{message}");
        let retried = apply(repo.path(), &message);
        assert!(retried.status.success(), "{message}\n{retried:?}");
        args.extend(["--projection", "agent-v1", "--json"]);
        let canonical = invoke(repo.path(), &args);
        assert!(canonical.status.success(), "{canonical:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&retried.stdout).unwrap(),
            serde_json::from_slice::<Value>(&canonical.stdout).unwrap()
        );
        assert_eq!(before, journal(repo.path()));
    }
}

#[test]
fn native_tool_recovery_executes_the_requested_target_only_after_retry() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    let plan = open(repo.path(), "Example native retry");
    let before = journal(repo.path());
    let output = invoke(
        repo.path(),
        &[
            "work",
            "check",
            "--plan-id",
            &plan,
            "--tool",
            "api:test",
            "--json",
        ],
    );
    let message = error_message(&output, true);
    assert_eq!(before, journal(repo.path()));
    assert!(message.contains("scripts/jig check api:test --plan-id"));
    let retried = apply(repo.path(), &message);
    assert!(retried.status.success(), "{message}\n{retried:?}");
    let records = String::from_utf8(journal(repo.path()).unwrap()).unwrap();
    assert!(
        records
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(
                |receipt| receipt["target"] == json!({"component":"api","action":"test"})
                    && receipt["exit_status"] == 0
            )
    );
    let before = journal(repo.path());
    let output = invoke(
        repo.path(),
        &[
            "work",
            "check",
            "--plan-id",
            &plan,
            "--tool",
            "api:test",
            "--tool",
            "api:missing",
            "--json",
        ],
    );
    let message = error_message(&output, true);
    assert!(message.contains("no unambiguous equivalent"));
    assert!(apply(repo.path(), &message).status.success());
    assert_eq!(before, journal(repo.path()));
}

#[test]
fn bare_contract_recovery_runs_real_validation_and_preserves_its_failure() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    fs::remove_file(repo.path().join(".mcp.json")).unwrap();
    let before = journal(repo.path());
    let output = jig()
        .current_dir(repo.path())
        .args(["contract", "--json"])
        .output()
        .unwrap();
    let message = error_message(&output, true);
    assert!(message.contains("jig check contract --json"), "{message}");
    assert_eq!(before, journal(repo.path()));
    let retried = apply(repo.path(), &message);
    // The fixture deliberately lost its MCP wiring. Recovery must expose that real validation
    // failure, not stop at parse success or substitute a successful stub.
    assert_eq!(retried.status.code(), Some(1), "{message}\n{retried:?}");
    let value: Value = serde_json::from_slice(&retried.stdout).unwrap();
    assert_eq!(value["executed"], true);
    assert_eq!(value["run"]["conclusion"], "failure");
    assert!(
        value["results"][0]["response"]["result"]["stderr"]
            .as_str()
            .unwrap()
            .contains("Missing .mcp.json"),
        "{value:#}"
    );
    assert_eq!(
        value["run"]["targets"][0]["target"],
        json!({"component":"repo","action":"contract"})
    );
}

#[test]
fn top_level_contract_recovery_preserves_arguments_and_never_executes_the_invalid_attempt() {
    let repo = tempdir().unwrap();
    fixture(repo.path());
    let before = journal(repo.path());
    let output = invoke(repo.path(), &["contract", "--help", "--json"]);
    let message = error_message(&output, true);
    assert!(
        message.contains("jig check contract --help --json"),
        "{message}"
    );
    assert_eq!(before, journal(repo.path()));
    assert!(apply(repo.path(), &message).status.success());
    assert_eq!(before, journal(repo.path()));
}
