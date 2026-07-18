#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use tempfile::tempdir;

#[test]
fn one_mcp_process_can_run_agent_doctor_twice() {
    let repo = tempdir().expect("create agent-doctor fixture");
    let codex_home = repo.path().join("codex-home");
    let codex = repo.path().join("codex");
    let probe_count = repo.path().join("probe-count");
    let startup = repo.path().join("startup-poison.sh");
    let startup_marker = repo.path().join("startup-poison-ran");

    fs::create_dir_all(repo.path().join(".agent")).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        repo.path().join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "agent-doctor-mcp"
default_branch = "main"
jig_version = "{}"

[commands]
custom_check_command = "printf 'fixture check\\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"

[[agent_tooling.codex.marketplaces]]
id = "test-skills"
source = "example/test-skills"
"#,
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    fs::write(
        repo.path().join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": env!("CARGO_PKG_VERSION"),
            "required_commands": ["custom_check_command"],
            "tools": [{
                "name": "jig.custom_check",
                "kind": "command",
                "description": "Fixture check.",
                "command": "custom_check_command",
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.test-skills]
source_type = "git"
source = "https://github.com/example/test-skills.git"
"#,
    )
    .unwrap();
    fs::write(
        &startup,
        "printf poison > \"$JIG_CODEX_PROBE_STARTUP_MARKER\"\nexit 91\n",
    )
    .unwrap();
    fs::write(
        &codex,
        r#"#!/usr/bin/env bash
[ "$*" = "plugin marketplace add --help" ] || exit 76
printf x >> "$JIG_CODEX_PROBE_COUNT"
"#,
    )
    .unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .arg("mcp")
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", &codex_home)
        .env("BASH_ENV", &startup)
        .env("JIG_CODEX_PROBE_STARTUP_MARKER", &startup_marker)
        .env("JIG_CODEX_PROBE_COUNT", &probe_count)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn production Jig MCP binary");
    {
        let mut stdin = child.stdin.take().unwrap();
        for id in [1, 2] {
            serde_json::to_writer(
                &mut stdin,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": {
                        "name": "jig.agent_doctor",
                        "arguments": {},
                    },
                }),
            )
            .unwrap();
            stdin.write_all(b"\n").unwrap();
        }
    }

    let output = child.wait_with_output().expect("wait for Jig MCP binary");
    assert!(
        output.status.success(),
        "MCP binary exited with {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2, "unexpected MCP stdout:\n{stdout}");
    for (index, response) in responses.iter().enumerate() {
        let doctor = &response["result"]["structuredContent"];
        assert_eq!(response["id"], json!(index + 1));
        assert_eq!(doctor["codex"]["available"], true, "{doctor:#}");
        assert!(doctor["codex"]["probe_error"].is_null(), "{doctor:#}");
    }
    assert_eq!(fs::read_to_string(&probe_count).unwrap(), "xx");
    assert!(
        !startup_marker.exists(),
        "Codex probe inherited and executed BASH_ENV"
    );
}
