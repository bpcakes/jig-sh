#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;
use wait_timeout::ChildExt;

struct Fixture {
    root: TempDir,
    plan: String,
}

impl Fixture {
    fn new() -> Self {
        Self::with_preflight("printf 'preflight\\n' >> .agent/launches; test ! -f .agent/reject")
    }

    fn with_preflight(preflight_command: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in [".agent", "scripts", "src"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        fs::write(root.path().join("src/example.txt"), "valid\n").unwrap();
        executable(
            &root.path().join("scripts/check-local"),
            include_str!("../../../scripts/check-local"),
        );
        executable(
            &root.path().join("scripts/jig"),
            "#!/bin/sh\nexec \"$EXAMPLE_JIG_BIN\" \"$@\"\n",
        );
        let components = json!([{"id":"example","root":".","adapters":[]}]);
        let actions = ["preflight", "test"].map(|name| {
            let command = if name == "preflight" {
                preflight_command
            } else {
                "printf 'test\\n' >> .agent/launches; test ! -f .agent/reject-test"
            };
            json!({
                "target":{"component":"example","action":name},
                "intent":"check","effects":["read_only","process"],
                "inputs":["src/**"],"source_state":"worktree",
                "runner":{"kind":"argv","program":"sh","args":["-c",command]}
            })
        });
        let preflight = json!({"component":"example","action":"preflight"});
        let test = json!({"component":"example","action":"test"});
        let profiles = json!([
            {"id":"preflight","targets":[preflight]},
            {"id":"verify","targets":[preflight,test]}
        ]);
        let config = json!({
            "_src_path":"embedded:jig-sh", "_commit":"", "repo_name":"ExampleProject",
            "default_branch":"main",
            "repository":{"components":components,"actions":actions,
                "profiles":profiles,"default_check_profile":"verify"},
            "work":{"iteration_profile":"preflight",
                "gates":[{"id":"verify","kind":"evidence","profile":"verify"}]}
        });
        fs::write(
            root.path().join(".jig.toml"),
            toml::to_string(&toml::Value::try_from(&config).unwrap()).unwrap(),
        )
        .unwrap();
        fs::write(
            root.path().join(".agent/jig-contract.json"),
            serde_json::to_vec_pretty(&json!({
                "contract_version":8,"tool_namespace":"jig","required_commands":[],
                "tools":[],"components":components,"actions":actions,"profiles":profiles,
                "default_check_profile":"verify"
            }))
            .unwrap(),
        )
        .unwrap();
        let mut fixture = Self {
            root,
            plan: String::new(),
        };
        fixture.git(&["init", "-q"]);
        fixture.git(&["add", "."]);
        fixture.git(&["commit", "-qm", "Example baseline"]);
        let output = fixture
            .jig()
            .args([
                "work",
                "start",
                "--title",
                "Example validation",
                "--body",
                "Check behavior",
                "--print-plan-id",
            ])
            .output()
            .unwrap();
        success(&output);
        fixture.plan = String::from_utf8(output.stdout).unwrap().trim().into();
        fixture
    }

    fn environment(&self, command: &mut Command) {
        command
            .current_dir(self.root.path())
            .stdin(Stdio::null())
            .env("EXAMPLE_JIG_BIN", env!("CARGO_BIN_EXE_jig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("NO_COLOR", "1")
            .env_remove("JIG_REPO_ROOT")
            .env_remove("JIG_INVOKE_CWD");
    }

    fn jig(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
        self.environment(&mut command);
        command
    }

    fn local(&self) -> Command {
        let mut command = Command::new(self.root.path().join("scripts/check-local"));
        self.environment(&mut command);
        command.args(["--plan-id", &self.plan]);
        command
    }

    fn git(&self, args: &[&str]) {
        let mut command = Command::new("git");
        self.environment(&mut command);
        success(
            &command
                .args([
                    "-c",
                    "user.name=Example Agent",
                    "-c",
                    "user.email=example@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap(),
        );
    }

    fn launches(&self) -> String {
        fs::read_to_string(self.root.path().join(".agent/launches")).unwrap_or_default()
    }

    fn receipts(&self) -> Vec<Value> {
        fs::read_to_string(self.root.path().join(".agent/state/receipts.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|receipt: &Value| receipt["target"].is_object())
            .collect()
    }
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn preflight_failure_prevents_full_suite_and_records_failure() {
    let fixture = Fixture::new();
    fs::write(fixture.root.path().join(".agent/reject"), "").unwrap();
    let output = fixture.local().output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fixture.launches(), "preflight\n");
    assert_eq!(fixture.receipts().len(), 1);
    assert_ne!(fixture.receipts()[0]["exit_status"], 0);
}

#[test]
fn final_reuses_preflight_and_unchanged_evidence_but_source_edit_reruns() {
    let fixture = Fixture::new();
    success(&fixture.local().output().unwrap());
    assert_eq!(fixture.launches(), "preflight\ntest\n");
    let receipts = fixture.receipts();
    assert_eq!(receipts.len(), 2);
    fixture.git(&["add", ".agent"]);
    fixture.git(&["commit", "-qm", "Example evidence only"]);
    success(&fixture.local().output().unwrap());
    assert_eq!(fixture.launches(), "preflight\ntest\n");
    assert_eq!(fixture.receipts(), receipts);
    fs::write(fixture.root.path().join("src/example.txt"), "changed\n").unwrap();
    success(&fixture.local().output().unwrap());
    assert_eq!(fixture.launches(), "preflight\ntest\npreflight\ntest\n");
    assert_eq!(fixture.receipts().len(), 4);
}

#[test]
fn full_suite_failure_is_not_hidden_by_successful_preflight() {
    let fixture = Fixture::new();
    fs::write(fixture.root.path().join(".agent/reject-test"), "").unwrap();
    let output = fixture.local().output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fixture.launches(), "preflight\ntest\n");
    assert_eq!(fixture.receipts().len(), 2);
    assert_ne!(fixture.receipts()[1]["exit_status"], 0);
}

#[test]
fn invalid_arguments_do_not_start_checks() {
    let fixture = Fixture::new();
    let output = fixture.local().arg("--unknown").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(fixture.launches().is_empty());
}

#[test]
fn cancellation_reaches_preflight_and_never_starts_final() {
    let fixture = Fixture::new();
    // A waiting launcher isolates the wrapper's signal forwarding. Jig's own
    // process-tree supervision is covered by its integration tests.
    executable(
        &fixture.root.path().join("scripts/jig"),
        "#!/usr/bin/env python3\nimport pathlib, signal, sys, time\n\
         def stop(sig, frame):\n    pathlib.Path('.agent/canceled').touch()\n    raise SystemExit(143)\n\
         signal.signal(signal.SIGTERM, stop)\n\
         with open('.agent/phases', 'a') as phases: phases.write(sys.argv[-1] + '\\n')\n\
         pathlib.Path('.agent/entered').touch()\n\
         while True: time.sleep(0.02)\n",
    );
    let mut child = fixture.local().stdout(Stdio::null()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !fixture.root.path().join(".agent/entered").exists() {
        assert!(Instant::now() < deadline, "launcher did not start");
        assert!(child.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: child remains owned and unreaped while receiving the signal.
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let status = child.wait_timeout(Duration::from_secs(15)).unwrap();
    if status.is_none() {
        // Forward another termination before killing the wrapper on failure.
        // SAFETY: child is still owned and unreaped.
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        let _ = child.wait_timeout(Duration::from_secs(5));
        let _ = child.kill();
        let _ = child.wait();
    }
    assert_eq!(status.and_then(|status| status.code()), Some(143));
    assert!(fixture.root.path().join(".agent/canceled").exists());
    assert_eq!(
        fs::read_to_string(fixture.root.path().join(".agent/phases")).unwrap(),
        "iteration\n"
    );
    assert!(fixture.launches().is_empty());
}

#[path = "local_validation/cancellation.rs"]
mod cancellation;
