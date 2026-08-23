#![cfg(any(target_os = "linux", target_os = "macos"))]

mod support;

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::tempdir;
use wait_timeout::ChildExt;

struct AgentDoctorFixture {
    repo: tempfile::TempDir,
    codex_home: PathBuf,
    codex: PathBuf,
    probe_count: PathBuf,
    startup_marker: PathBuf,
}

impl AgentDoctorFixture {
    fn new() -> Self {
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
bootstrap_command = "scripts/setup-child.sh"

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
                "required_commands": ["custom_check_command", "bootstrap_command"],
                "tools": [
                    {
                        "name": "jig.custom_check",
                        "kind": "command",
                        "description": "Fixture check.",
                        "command": "custom_check_command",
                    },
                    {
                        "name": "jig.bootstrap",
                        "kind": "command",
                        "description": "Fixture bootstrap.",
                        "command": "bootstrap_command",
                    },
                ],
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
        fs::create_dir_all(repo.path().join("scripts")).unwrap();
        let bootstrap = repo.path().join("scripts/setup-child.sh");
        fs::write(&bootstrap, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&bootstrap, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            repo,
            codex_home,
            codex,
            probe_count,
            startup_marker,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
        command
            .current_dir(self.repo.path())
            .env_remove("JIG_REPO_ROOT")
            .env_remove("JIG_INVOKE_CWD")
            .env("JIG_CODEX_BIN", &self.codex)
            .env("CODEX_HOME", &self.codex_home)
            .env("BASH_ENV", self.repo.path().join("startup-poison.sh"))
            .env("JIG_CODEX_PROBE_STARTUP_MARKER", &self.startup_marker)
            .env("JIG_CODEX_PROBE_COUNT", &self.probe_count)
            .env("NO_COLOR", "1");
        command
    }

    fn assert_probe_count(&self, expected: &str) {
        assert_eq!(fs::read_to_string(&self.probe_count).unwrap(), expected);
        assert!(
            !self.startup_marker.exists(),
            "Codex probe inherited and executed BASH_ENV"
        );
    }

    fn install_blocking_bootstrap(&self) -> (PathBuf, PathBuf) {
        let started = self.repo.path().join("setup-child-started");
        let delayed = self.repo.path().join("setup-child-delayed");
        let bootstrap = self.repo.path().join("scripts/setup-child.sh");
        fs::write(
            &bootstrap,
            r#"#!/bin/sh
	printf 'setup child progress sentinel\n'
	i=0
	while [ "$i" -lt 5000 ]; do
	  printf 'bounded progress payload\n'
	  i=$((i + 1))
	done
	printf started > "$JIG_SETUP_CHILD_STARTED"
	(sleep 1; printf leaked > "$JIG_SETUP_DELAYED_MARKER") &
	wait
"#,
        )
        .unwrap();
        fs::set_permissions(&bootstrap, fs::Permissions::from_mode(0o755)).unwrap();
        (started, delayed)
    }
}

fn wait_for_output(mut child: Child, timeout: Duration) -> Output {
    let status = match child.wait_timeout(timeout).expect("wait for Jig binary") {
        Some(status) => status,
        None => {
            child.kill().expect("kill timed-out Jig binary");
            child.wait().expect("reap timed-out Jig binary");
            panic!("Jig binary did not exit within {timeout:?}");
        }
    };
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}

#[test]
fn cli_agent_doctor_reuses_outer_signal_session() {
    let fixture = AgentDoctorFixture::new();
    let mut command = fixture.command();
    let child = command
        .args(["--json", "agent", "doctor"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn production Jig CLI binary");

    let output = wait_for_output(child, Duration::from_secs(5));
    assert!(
        output.status.success(),
        "CLI binary exited with {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let doctor: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doctor["codex"]["available"], true, "{doctor:#}");
    assert!(doctor["codex"]["probe_error"].is_null(), "{doctor:#}");
    fixture.assert_probe_count("x");
}

#[test]
fn interrupted_setup_reaps_its_owned_bootstrap_tree() {
    let fixture = AgentDoctorFixture::new();
    let (started, delayed) = fixture.install_blocking_bootstrap();
    let mut command = fixture.command();
    command.env_remove("BASH_ENV");
    let mut child = command
        .arg("setup")
        .env("JIG_SETUP_CHILD_STARTED", &started)
        .env("JIG_SETUP_DELAYED_MARKER", &delayed)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn production Jig setup binary");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !started.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            let mut stdout = Vec::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_end(&mut stdout)
                .unwrap();
            let mut stderr = Vec::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_end(&mut stderr)
                .unwrap();
            panic!(
                "setup exited with {status} before its bootstrap child started\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            );
        }
        assert!(Instant::now() < deadline, "setup bootstrap did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: `child.id()` names the still-live process checked above, and
    // SIGINT is the public interruption contract exercised by this test.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);

    let output = wait_for_output(child, Duration::from_secs(5));
    assert!(
        !output.status.success(),
        "interrupted setup unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("setup child progress sentinel"),
        "interrupted setup discarded buffered child progress\nstderr:\n{stderr}"
    );
    std::thread::sleep(Duration::from_millis(1_250));
    assert!(
        !delayed.exists(),
        "setup interruption left a bootstrap descendant running"
    );
}

#[test]
fn one_mcp_process_can_run_agent_doctor_twice() {
    let fixture = AgentDoctorFixture::new();

    let mut command = fixture.command();
    let mut child = command
        .arg("mcp")
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
    fixture.assert_probe_count("xx");
}
