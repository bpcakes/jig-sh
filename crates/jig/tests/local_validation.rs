#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt;
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

#[test]
fn sighup_cleans_real_runtime_action_before_wrapper_returns() {
    for foreground_group in [false, true] {
        exercise_runtime_cancellation(foreground_group, false);
    }
}

#[test]
fn forced_runtime_termination_reports_unconfirmed_descendant_cleanup() {
    exercise_runtime_cancellation(false, true);
}

fn exercise_runtime_cancellation(foreground_group: bool, stalled: bool) {
    let fixture = Fixture::with_preflight("exec python3 scripts/waiting-check");
    executable(
        &fixture.root.path().join("scripts/jig"),
        "#!/bin/sh\nprintf '%s\\n' \"$$\" > .agent/runtime.pid\nexec \"$EXAMPLE_JIG_BIN\" \"$@\"\n",
    );
    fs::write(
        fixture.root.path().join("scripts/waiting-check"),
        r#"import fcntl, pathlib, time
root = pathlib.Path('.agent')
with (root / 'action.lock').open('w') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    (root / 'launches').write_text('preflight\n')
    (root / 'heartbeat').write_text(str(time.monotonic_ns()))
    (root / 'ready').touch()
    deadline = time.monotonic() + 30
    while not (root / 'release').exists() and time.monotonic() < deadline:
        (root / 'heartbeat').write_text(str(time.monotonic_ns()))
        time.sleep(0.02)
"#,
    )
    .unwrap();
    let mut child = fixture
        .local()
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(fs::File::create(fixture.root.path().join(".agent/stderr")).unwrap())
        .spawn()
        .unwrap();
    let started = wait_for_runtime_action(&fixture, &mut child);
    let runtime_pid = fs::read_to_string(fixture.root.path().join(".agent/runtime.pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<libc::pid_t>().ok());
    let suspended = stalled
        && started
        && runtime_pid.is_some_and(|pid| {
            // The waiting action keeps the runtime alive; the wrapper retains
            // its unreaped phase leader until cancellation cleanup completes.
            unsafe { libc::kill(pid, libc::SIGSTOP) == 0 }
        });
    let signal = if stalled { libc::SIGTERM } else { libc::SIGHUP };
    let sent = started && {
        let pid = child.id() as libc::pid_t;
        let target = if foreground_group { -pid } else { pid };
        // SAFETY: the owned, unreaped wrapper pins this PID/process group.
        unsafe { libc::kill(target, signal) == 0 }
    };
    let status = child.wait_timeout(Duration::from_secs(15)).unwrap();
    // Observe cleanup before releasing the fixture ourselves, including
    // when a broken wrapper exits immediately with its action still alive.
    let lock_released = fs::File::open(fixture.root.path().join(".agent/action.lock"))
        .is_ok_and(|lock| FileExt::try_lock_exclusive(&lock).unwrap_or(false));
    let heartbeat_timeout = if stalled {
        Duration::from_secs(5)
    } else {
        Duration::from_millis(100)
    };
    let stopped = !heartbeat_changes(&fixture, heartbeat_timeout);
    let launches = fixture.launches();
    let journal = fixture.root.path().join(".agent/state/runs.jsonl");
    let events = fs::read_to_string(&journal).unwrap_or_default();
    let stderr = fs::read_to_string(fixture.root.path().join(".agent/stderr")).unwrap();
    if status.is_none() {
        stop_wrapper_after_timeout(&mut child, runtime_pid.filter(|_| suspended));
    }
    release_runtime_action(&fixture, started, stalled);
    assert!(started, "real runtime action did not start");
    assert!(sent, "could not send cancellation");
    assert_eq!(launches, "preflight\n");
    if stalled {
        assert!(suspended, "could not suspend runtime");
        assert_eq!(status.and_then(|status| status.code()), Some(1), "{stderr}");
        assert!(
            stderr.contains("descendant cleanup could not be confirmed"),
            "{stderr}"
        );
        assert!(
            stderr.contains("child processes may still be running"),
            "{stderr}"
        );
        assert!(!lock_released, "fixture did not leave an action behind");
        assert!(
            !stopped,
            "fixture action did not survive forced termination"
        );
        assert!(
            FileExt::try_lock_exclusive(
                &fs::File::open(fixture.root.path().join(".agent/action.lock")).unwrap()
            )
            .unwrap(),
            "fixture cleanup did not release the action lock"
        );
        return;
    }
    assert_eq!(
        status.and_then(|status| status.code()),
        Some(129),
        "{stderr}"
    );
    assert!(
        lock_released,
        "action still held its lock after cancellation"
    );
    assert!(stopped, "action heartbeat continued after cancellation");
    assert!(events.lines().any(|line| {
        let event: Value = serde_json::from_str(line).unwrap();
        event["event"] == "completed" && event["conclusion"] == "cancelled"
    }));
}

fn wait_for_runtime_action(fixture: &Fixture, child: &mut Child) -> bool {
    let ready = fixture.root.path().join(".agent/ready");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    ready.exists() && child.try_wait().unwrap().is_none()
}

fn stop_wrapper_after_timeout(child: &mut Child, suspended_runtime: Option<libc::pid_t>) {
    if let Some(pid) = suspended_runtime {
        // Resume the still-owned phase leader so it can clean up on a test
        // failure before we release the action and wrapper.
        unsafe { libc::kill(pid, libc::SIGCONT) };
        let _ = child.wait_timeout(Duration::from_secs(5));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn heartbeat_changes(fixture: &Fixture, timeout: Duration) -> bool {
    let path = fixture.root.path().join(".agent/heartbeat");
    let initial = fs::read(&path).ok();
    let deadline = Instant::now() + timeout;
    // A live action need not be scheduled within a single short sleep on CI.
    // Poll for positive progress; ignore the empty interval during a write.
    while Instant::now() < deadline {
        if fs::read(&path).is_ok_and(|current| !current.is_empty() && Some(current) != initial) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

fn release_runtime_action(fixture: &Fixture, started: bool, stalled: bool) {
    // Let surviving fixture work finish before its temporary directory is removed.
    fs::write(fixture.root.path().join(".agent/release"), "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while started && Instant::now() < deadline {
        let action_done = fs::File::open(fixture.root.path().join(".agent/action.lock"))
            .is_ok_and(|lock| FileExt::try_lock_exclusive(&lock).unwrap_or(false));
        let events = fs::read_to_string(fixture.root.path().join(".agent/state/runs.jsonl"))
            .unwrap_or_default();
        if action_done
            && (stalled
                || events.lines().any(|line| {
                    serde_json::from_str::<Value>(line)
                        .is_ok_and(|event| event["event"] == "completed")
                }))
        {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
}
