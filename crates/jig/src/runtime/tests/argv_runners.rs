use super::*;
use crate::command::RepositoryRunRequest;

const ARGV_CAPTURE: &str = "argv-capture.json";
const PROGRAM: &str = "capture ; $(touch injected)";
const ALIAS: &str = "jig.test";

fn argv_fixture() -> tempfile::TempDir {
    let temp = tempdir().unwrap();
    let root = temp.path();
    write_v6_evidence_fixture_repo(root, "");
    super::foreground_run::add_v6_generate_action(root);
    let script = root.join(PROGRAM);
    // Successful runs discard captured output, so also persist it for assertions.
    fs::write(&script, format!("#!/usr/bin/env python3\nimport json, sys, os\ncapture = json.dumps([sys.argv[1:], os.environ.get('EXAMPLE_VALUE'), os.getcwd()])\nprint(capture)\nopen({ARGV_CAPTURE:?}, 'w').write(capture)\n")).unwrap();
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
        action["legacy_aliases"] = json!([]);
        if action["target"]["action"] == "generate" {
            action["arguments"] = json!({
                "message": {"type": "string", "required": true, "max_bytes": 64},
                "optional": {"type": "string", "allow_empty": true, "max_bytes": 8}
            });
            action["runner"] = json!({
                "kind": "argv", "program": script.to_str().unwrap(),
                "args": ["literal * ; $HOME", {"argument": "message"}, {"argument": "optional"}, "tail"],
                "environment": {"EXAMPLE_VALUE": "literal $(touch environment-injected)"}
            });
            action["legacy_aliases"] = json!([ALIAS]);
        }
    }
    manifest["tools"] = json!([{"name": ALIAS, "kind": "command", "description": "Capture argv"}]);
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    sync_actions(root, &manifest);
    init_git_repo(root);
    temp
}

fn sync_actions(root: &std::path::Path, manifest: &Value) {
    let path = root.join(".jig.toml");
    let mut source: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    source["repository"]["actions"] = toml::Value::try_from(&manifest["actions"]).unwrap();
    fs::write(path, toml::to_string(&source).unwrap()).unwrap();
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
    sync_actions(root, &manifest);
}

fn request(values: Value) -> RepositoryRunRequest {
    let arguments = values
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| format!("api:generate:{key}={}", value.as_str().unwrap()))
        .collect();
    RepositoryRunRequest {
        arguments: jig_repository::arguments::parse_cli(arguments).unwrap(),
        selectors: vec!["api:generate".into()],
        profile: None,
        affected_base: None,
        comparison: None,
        explain: false,
        fail_fast: false,
        approved_effects: vec![jig_contract::ActionEffect::Worktree],
    }
}

fn run(ctx: &RepoContext, values: Value) -> Value {
    crate::runtime::dispatch(ctx, RuntimeCommand::Run(request(values))).unwrap()
}

fn execute_alias(ctx: &RepoContext, values: Value) -> anyhow::Result<Value> {
    crate::runtime::tool_execution::execute_manifest_tool_with_observer(
        ctx,
        ALIAS,
        values,
        &mut jig_execution::NoopExecutionObserver,
    )
}

#[test]
fn argv_literal_program_positions_and_alias_preserve_bytes() {
    let temp = argv_fixture();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let literal = "  é ' \" * ; $(touch injected)\nx=y  ";
    for optional in [Some(""), None] {
        let mut values = json!({"message": literal});
        let mut expected = vec!["literal * ; $HOME", literal];
        if let Some(optional) = optional {
            values["optional"] = json!(optional);
            expected.push(optional);
        }
        expected.push("tail");
        let expected = json!([
            expected,
            "literal $(touch environment-injected)",
            temp.path().canonicalize().unwrap().to_str().unwrap()
        ]);
        let output = run(&ctx, values.clone());
        assert_eq!(output["ok"], true, "{output:#}");
        assert_eq!(output["run"]["conclusion"], "success");
        let captured: Value =
            serde_json::from_str(&fs::read_to_string(temp.path().join(ARGV_CAPTURE)).unwrap())
                .unwrap();
        assert_eq!(captured, expected);
        let alias = execute_alias(&ctx, values).unwrap();
        let captured: Value =
            serde_json::from_str(alias["result"]["stdout"].as_str().unwrap()).unwrap();
        assert_eq!(captured, expected);
    }
    assert!(!temp.path().join("injected").exists());
    assert!(!temp.path().join("environment-injected").exists());
}

#[test]
fn argv_never_falls_back_to_shell_for_executable_text() {
    for path_lookup in [false, true] {
        let temp = argv_fixture();
        fs::write(
            temp.path().join(PROGRAM),
            "printf 'IMPLICIT SHELL RAN'\ntouch implicit-shell-ran\n",
        )
        .unwrap();
        if path_lookup {
            change_action(temp.path(), |action| {
                action["runner"]["program"] = json!(PROGRAM);
                action["runner"]["environment"]["PATH"] = json!(temp.path().to_str().unwrap());
            });
        }
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let output = run(&ctx, json!({"message": "example"}));
        assert_eq!(output["ok"], false, "{output:#}");
        assert_eq!(output["run"]["conclusion"], "blocked", "{output:#}");
        let alias = execute_alias(&ctx, json!({"message": "example"})).unwrap_err();
        assert!(
            format!("{alias:#}").contains("Exec format error"),
            "{alias:#}"
        );
        let diagnostics = output.to_string();
        assert!(diagnostics.contains("Argv runner '"), "{output:#}");
        assert!(diagnostics.contains(PROGRAM), "{output:#}");
        assert!(!temp.path().join("implicit-shell-ran").exists());
        assert!(!temp.path().join("injected").exists());
    }
}

#[test]
fn argv_path_lookup_uses_the_declared_environment_and_working_directory() {
    let temp = argv_fixture();
    change_action(temp.path(), |action| {
        action["runner"]["program"] = json!(PROGRAM);
        action["runner"]["working_directory"] = json!("api");
        // Resolve the relative PATH entry from the action cwd; the shebang
        // interpreter also needs the ordinary PATH.
        action["runner"]["environment"]["PATH"] =
            json!(format!("..:{}", std::env::var("PATH").unwrap()));
    });
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let expected = json!([
        ["literal * ; $HOME", "example", "tail"],
        "literal $(touch environment-injected)",
        temp.path()
            .join("api")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    ]);
    let output = run(&ctx, json!({"message": "example"}));
    assert_eq!(output["ok"], true, "{output:#}");
    let captured: Value = serde_json::from_str(
        &fs::read_to_string(temp.path().join("api").join(ARGV_CAPTURE)).unwrap(),
    )
    .unwrap();
    assert_eq!(captured, expected);
    let alias = execute_alias(&ctx, json!({"message": "example"})).unwrap();
    let captured: Value =
        serde_json::from_str(alias["result"]["stdout"].as_str().unwrap()).unwrap();
    assert_eq!(captured, expected);
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
            temp.path().join(PROGRAM),
            format!("#!/usr/bin/env python3\n{body}\n"),
        )
        .unwrap();
        change_action(temp.path(), |action| {
            action["timeout_seconds"] = json!(timeout);
        });
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let output = run(&ctx, json!({"message": "example"}));
        assert_eq!(output["ok"], false, "{output:#}");
        assert_eq!(output["run"]["conclusion"], conclusion, "{output:#}");
        let target = &output["run"]["targets"][0];
        assert_eq!(target["exit_code"], json!(exit), "{target:#}");
        if exit.is_some() {
            assert!(
                target["output_tail"]["stdout"]
                    .as_str()
                    .unwrap()
                    .contains("ordinary failure"),
                "{target:#}"
            );
        }
    }
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn argv_running_cancellation_stops_the_owned_process_and_descendant() {
    #[derive(Default)]
    struct CancelOnDescendant {
        output: Vec<u8>,
        descendant: Option<crate::test_process::TestProcessIdentity>,
    }
    impl jig_execution::ExecutionObserver for CancelOnDescendant {
        fn event(&mut self, event: jig_execution::ExecutionEvent<'_>) {
            if self.descendant.is_some() {
                return;
            }
            let jig_execution::ExecutionEvent::Output { bytes, .. } = event else {
                return;
            };
            self.output.extend_from_slice(bytes);
            if let Some(end) = self.output.iter().position(|byte| *byte == b'\n') {
                let pid = std::str::from_utf8(&self.output[..end])
                    .unwrap()
                    .parse()
                    .unwrap();
                self.descendant = Some(
                    crate::test_process::TestProcessIdentity::capture(pid)
                        .expect("descendant must be alive before cancellation"),
                );
            }
        }
    }
    impl jig_execution::ExecutionCancellation for CancelOnDescendant {
        fn cancelled(&self) -> bool {
            self.descendant.is_some()
        }
    }
    let temp = argv_fixture();
    fs::write(temp.path().join(PROGRAM), "#!/usr/bin/env python3\nimport pathlib, subprocess, sys, time\nchild = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\nprint(child.pid, flush=True)\ntime.sleep(30)\npathlib.Path('escaped').write_text('escaped')\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut observer = CancelOnDescendant::default();
    let output = dispatch_with_observer(
        &ctx,
        RuntimeCommand::Run(request(json!({"message": "example"}))),
        &mut observer,
    )
    .unwrap();
    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["run"]["conclusion"], "cancelled", "{output:#}");
    crate::test_process::assert_test_process_stopped(&observer.descendant.unwrap());
    assert!(!temp.path().join("escaped").exists());
}

#[test]
fn argv_results_use_the_declared_parser_and_output_limit() {
    let temp = argv_fixture();
    fs::write(temp.path().join(PROGRAM), "#!/usr/bin/env python3\nprint('{\"severity\":\"warning\",\"message\":\"Example finding\"}')\n").unwrap();
    change_action(temp.path(), |action| {
        action["result_parser"] = json!("json_lines");
    });
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = run(&ctx, json!({"message": "example"}));
    let target = &output["run"]["targets"][0];
    assert_eq!(
        target["findings"][0]["message"], "Example finding",
        "{target:#}"
    );
    assert_eq!(target["conclusion"], "success");

    fs::write(
        temp.path().join(PROGRAM),
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
    let output = run(&ctx, json!({"message": "example"}));
    let target = &output["run"]["targets"][0];
    assert_eq!(target["conclusion"], "failure", "{output:#}");
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
