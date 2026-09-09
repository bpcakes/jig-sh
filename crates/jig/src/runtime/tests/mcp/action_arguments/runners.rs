use super::*;

fn argv_fixture() -> tempfile::TempDir {
    let temp = fixture();
    let root = temp.path();
    let script = root.join("capture ; $(touch injected)");
    fs::write(&script, "#!/usr/bin/env python3\nimport json, sys, os\nprint(json.dumps([sys.argv[1:], os.environ.get('EXAMPLE_VALUE'), os.getcwd()]))\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contract_version"] = json!(8);
    for action in manifest["actions"].as_array_mut().unwrap() {
        if action["runner"]["kind"] == "command" {
            action["runner"]["kind"] = json!("shell");
        }
        if action["target"]["action"] == "generate" {
            action["runner"] = json!({
                "kind": "argv", "program": script.to_str().unwrap(),
                "args": ["literal * ; $HOME", {"argument": "message"}, {"argument": "optional"}, "tail"],
                "environment": {"EXAMPLE_VALUE": "literal $(touch environment-injected)"}
            });
            action["legacy_aliases"] = json!([tool::TEST]);
        } else {
            action["legacy_aliases"] = json!([]);
        }
    }
    manifest["tools"] =
        json!([{"name": tool::TEST, "kind": "command", "description": "Capture argv"}]);
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    temp
}

#[test]
fn argv_literal_program_positions_and_alias_preserve_bytes() {
    let temp = argv_fixture();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let literal = "  é ' \" * ; $(touch injected)\nx=y  ";
    let plan = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({
            "selectors": ["api:generate"],
            "arguments": {"api:generate": {"message": literal, "optional": ""}}
        }),
    )
    .unwrap();
    let mut tampered = plan["plan"].clone();
    tampered["targets"][0]["runner"]["program"] = json!("false");
    assert!(
        call_tool(
            &ctx,
            tool::EXECUTE_RUN,
            json!({"plan": tampered, "approved_effects": ["worktree"]})
        )
        .is_err()
    );
    let output = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": plan["plan"], "approved_effects": ["worktree"]}),
    )
    .unwrap();
    assert_eq!(output["ok"], true, "{output:#}");
    let terminal = wait_for_repository_run(&ctx, output["run_id"].as_str().unwrap());
    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "success",
        "{terminal:#}"
    );
    let receipts = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
    let receipt: Value = serde_json::from_str(receipts.lines().last().unwrap()).unwrap();
    let captured: Value =
        serde_json::from_str(receipt["stdout_preview"].as_str().unwrap()).unwrap();
    assert_eq!(
        captured[0],
        json!(["literal * ; $HOME", literal, "", "tail"])
    );
    let alias = execute_alias(
        &ctx,
        tool::TEST,
        json!({"message": literal, "optional": ""}),
    )
    .unwrap();
    let captured: Value =
        serde_json::from_str(alias["result"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(
        captured,
        json!([
            ["literal * ; $HOME", literal, "", "tail"],
            "literal $(touch environment-injected)",
            temp.path().canonicalize().unwrap().to_str().unwrap()
        ])
    );
    let alias = execute_alias(&ctx, tool::TEST, json!({"message": literal})).unwrap();
    let captured: Value =
        serde_json::from_str(alias["result"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(captured[0], json!(["literal * ; $HOME", literal, "tail"]));
    assert!(!temp.path().join("injected").exists());
    assert!(!temp.path().join("environment-injected").exists());
}

#[test]
fn argv_changed_source_rejects_prepared_plan() {
    let temp = argv_fixture();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({"selectors": ["api:generate"], "arguments": {"api:generate": {"message": "value"}}}),
    )
    .unwrap();
    fs::write(
        temp.path().join("capture ; $(touch injected)"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    assert!(
        call_tool(
            &ctx,
            tool::EXECUTE_RUN,
            json!({"plan": plan["plan"], "approved_effects": ["worktree"]})
        )
        .is_err()
    );
}

fn execute_alias(ctx: &RepoContext, name: &str, values: Value) -> anyhow::Result<Value> {
    crate::runtime::tool_execution::execute_manifest_tool_request_with_observer(
        ctx,
        name,
        values,
        ToolRequest::default(),
        &mut crate::execution::NoopExecutionObserver,
    )
}

fn change_action(root: &std::path::Path, edit: impl FnOnce(&mut Value)) {
    let path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let action = manifest["actions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|action| action["target"]["action"] == "generate")
        .unwrap();
    edit(action);
    fs::write(path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    let path = root.join(".jig.toml");
    let mut source: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    source["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    fs::write(path, toml::to_string(&source).unwrap()).unwrap();
}

fn start_run(ctx: &RepoContext) -> Value {
    let plan = call_tool(ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"], "arguments": {"api:generate": {"message": "example"}}})).unwrap();
    assert!(call_tool(ctx, tool::EXECUTE_RUN, json!({"plan": plan["plan"]})).is_err());
    call_tool(
        ctx,
        tool::EXECUTE_RUN,
        json!({"plan": plan["plan"], "approved_effects": ["worktree"]}),
    )
    .unwrap()
}

#[test]
fn argv_timeout_and_nonzero_results_remain_jig_owned() {
    for (body, timeout, conclusion, exit) in [
        ("import time; time.sleep(30)", 1, "timed_out", None),
        (
            "import sys; print('ordinary failure'); sys.exit(17)",
            10,
            "failure",
            Some(17),
        ),
    ] {
        let temp = argv_fixture();
        fs::write(
            temp.path().join("capture ; $(touch injected)"),
            format!("#!/usr/bin/env python3\n{body}\n"),
        )
        .unwrap();
        change_action(temp.path(), |action| {
            action["timeout_seconds"] = json!(timeout);
        });
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let accepted = start_run(&ctx);
        let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
        assert_eq!(
            terminal["result"]["run"]["result"]["conclusion"], conclusion,
            "{terminal:#}"
        );
        let receipts = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
        let receipt: Value = serde_json::from_str(receipts.lines().last().unwrap()).unwrap();
        if let Some(exit) = exit {
            assert_eq!(receipt["exit_status"], exit);
        }
        assert!(receipt["invoked_command_key"].is_null());
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn argv_running_cancellation_stops_the_owned_process_and_descendant() {
    let temp = argv_fixture();
    fs::write(temp.path().join("capture ; $(touch injected)"), "#!/usr/bin/env python3\nimport pathlib, subprocess, sys, time\nchild = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\npathlib.Path('ready.tmp').write_text(str(child.pid))\npathlib.Path('ready.tmp').rename('ready')\ntime.sleep(30)\npathlib.Path('escaped').write_text('escaped')\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let accepted = start_run(&ctx);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !temp.path().join("ready").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let pid = fs::read_to_string(temp.path().join("ready"))
        .unwrap()
        .parse()
        .unwrap();
    let descendant = crate::test_process::TestProcessIdentity::capture(pid)
        .expect("descendant must be alive before cancellation");
    call_tool(
        &ctx,
        tool::CANCEL_RUN,
        json!({"run_id": accepted["run_id"]}),
    )
    .unwrap();
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "cancelled",
        "{terminal:#}"
    );
    crate::test_process::assert_test_process_stopped(&descendant);
    assert!(!temp.path().join("escaped").exists());
}

#[test]
fn argv_never_falls_back_to_shell_for_executable_text() {
    for path_lookup in [false, true] {
        let temp = argv_fixture();
        let program = temp.path().join("capture ; $(touch injected)");
        fs::write(
            &program,
            "printf 'IMPLICIT SHELL RAN'\ntouch implicit-shell-ran\n",
        )
        .unwrap();
        if path_lookup {
            change_action(temp.path(), |action| {
                action["runner"]["program"] = json!(program.file_name().unwrap().to_str().unwrap());
                action["runner"]["environment"]["PATH"] = json!(temp.path().to_str().unwrap());
            });
        }
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let accepted = start_run(&ctx);
        let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
        assert_eq!(
            terminal["result"]["run"]["result"]["conclusion"], "blocked",
            "{terminal:#}"
        );
        let alias = execute_alias(&ctx, tool::TEST, json!({"message":"example"})).unwrap_err();
        assert!(
            format!("{alias:#}").contains("Exec format error"),
            "{alias:#}"
        );
        let diagnostics = terminal.to_string();
        assert!(diagnostics.contains("Argv runner '"), "{terminal:#}");
        assert!(
            diagnostics.contains("capture ; $(touch injected)"),
            "{terminal:#}"
        );
        assert!(!temp.path().join("implicit-shell-ran").exists());
        assert!(!temp.path().join("injected").exists());
    }
}

#[test]
fn argv_path_lookup_uses_the_declared_environment_and_working_directory() {
    let temp = argv_fixture();
    change_action(temp.path(), |action| {
        action["runner"]["program"] = json!("capture ; $(touch injected)");
        action["runner"]["working_directory"] = json!("api");
        // The relative first entry must be resolved from the action cwd. The
        // interpreter in the checked-in shebang still needs the ordinary PATH.
        action["runner"]["environment"]["PATH"] =
            json!(format!("..:{}", std::env::var("PATH").unwrap()));
    });
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = execute_alias(&ctx, tool::TEST, json!({"message":"example"})).unwrap();
    let captured: Value =
        serde_json::from_str(output["result"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(
        captured[2],
        json!(temp.path().join("api").to_str().unwrap())
    );
    assert_eq!(captured[0], json!(["literal * ; $HOME", "example", "tail"]));
}

#[test]
fn argv_results_use_the_declared_parser_and_output_limit() {
    let temp = argv_fixture();
    fs::write(temp.path().join("capture ; $(touch injected)"), "#!/usr/bin/env python3\nprint('{\"severity\":\"warning\",\"message\":\"Example finding\"}')\n").unwrap();
    change_action(temp.path(), |action| {
        action["result_parser"] = json!("json_lines");
    });
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let accepted = start_run(&ctx);
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
    let target = &terminal["result"]["run"]["result"]["targets"][0];
    assert_eq!(
        target["findings"][0]["message"], "Example finding",
        "{target:#}"
    );
    assert_eq!(target["conclusion"], "success");

    fs::write(
        temp.path().join("capture ; $(touch injected)"),
        "#!/usr/bin/env python3\nprint('x'*8192)\n",
    )
    .unwrap();
    let path = temp.path().join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config.as_table_mut().unwrap().insert(
        "execution".into(),
        toml::Value::try_from(json!({"command_output_limit_bytes":128})).unwrap(),
    );
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let accepted = start_run(&ctx);
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
    let target = &terminal["result"]["run"]["result"]["targets"][0];
    assert_eq!(target["conclusion"], "failure", "{terminal:#}");
    assert!(target.to_string().contains("Argv runner '"), "{target:#}");
    assert!(
        target["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["source"] == "execution_policy"
                && finding["message"]
                    .as_str()
                    .unwrap()
                    .contains("128 byte stdout capture limit"))
    );
}

#[test]
fn shell_output_limit_diagnostic_retains_its_command_key() {
    let temp = fixture();
    change_action(temp.path(), |action| {
        action["runner"] = json!({"kind": "shell", "command": "generate_command"});
        action["arguments"] = json!({});
    });
    let path = temp.path().join(".jig.toml");
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    config["commands"]["generate_command"] = toml::Value::String(
        "while :; do printf 'Example output exceeding capture limit\\n'; done".into(),
    );
    config.as_table_mut().unwrap().insert(
        "execution".into(),
        toml::Value::try_from(json!({"command_output_limit_bytes":128})).unwrap(),
    );
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": plan["plan"], "approved_effects": ["worktree"]}),
    )
    .unwrap();
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
    let target = &terminal["result"]["run"]["result"]["targets"][0];
    assert_eq!(target["conclusion"], "failure", "{terminal:#}");
    assert!(
        target["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["message"]
                .as_str()
                .unwrap()
                .contains("Command runner 'generate_command' for target 'api:generate' exceeded")),
        "{target:#}"
    );
}
