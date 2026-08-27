#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;
use wait_timeout::ChildExt;

#[test]
fn state_diagnose_keeps_native_sigint_instead_of_installing_an_unused_observer() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
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
        r#"{
  "contract_version": 2,
  "jig_version": "0.2.0-beta.1",
  "tool_namespace": "jig",
  "required_commands": ["contract_check_command"],
  "tools": []
}"#,
    )
    .unwrap();
    let fake_bin = temp.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let git = fake_bin.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
printf '%s' "$$" > "$JIG_TEST_GIT_PID"
: > "$JIG_TEST_GIT_STARTED"
exec sleep 60
"#,
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();

    let started_marker = temp.path().join("git-started");
    let git_pid_path = temp.path().join("git-pid");
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut search_paths = vec![fake_bin];
    search_paths.extend(std::env::split_paths(&inherited_path));
    let path = std::env::join_paths(search_paths).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["state", "diagnose", "--deep"])
        .current_dir(temp.path())
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("PATH", path)
        .env("JIG_TEST_GIT_STARTED", &started_marker)
        .env("JIG_TEST_GIT_PID", &git_pid_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while !started_marker.exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "state diagnose exited with {status} before its blocking probe: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !started_marker.exists() {
        let _ = child.kill();
        let _ = child.wait();
        kill_recorded_process(&git_pid_path);
        panic!("state diagnose did not reach its blocking probe");
    }

    // SAFETY: `child.id()` is the live Jig process observed above.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    let status = child
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            kill_recorded_process(&git_pid_path);
            panic!("state diagnose swallowed SIGINT in an unused cooperative observer")
        });

    kill_recorded_process(&git_pid_path);
    assert!(!status.success());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn schema_check_cancels_its_git_probe_before_redelivering_sigint() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent/state")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"
sqlx_enabled = true
schema_dump_enabled = true
rust_migration_dir = "migrations"
schema_dump_command = "touch .schema-dump-finished"
rust_test_command = "true"

[execution]
command_timeout_seconds = 30
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        r#"{
  "contract_version": 2,
  "jig_version": "0.2.0-beta.1",
  "tool_namespace": "jig",
  "required_commands": ["rust_test_command", "schema_dump_command"],
  "tools": [
    {
      "name": "jig.schema_check",
      "kind": "native",
      "description": "Schema check."
    }
  ]
}"#,
    )
    .unwrap();

    let fake_bin = temp.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let git = fake_bin.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
test "${1:-}" = "--literal-pathspecs"
test "${2:-}" = "status"
test -f "$JIG_TEST_SCHEMA_DUMP_FINISHED"
printf '%s' "$$" > "$JIG_TEST_GIT_PID"
: > "$JIG_TEST_GIT_STARTED"
(sleep 1; : > "$JIG_TEST_GIT_DESCENDANT_SURVIVED") &
wait
"#,
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();

    let started_marker = temp.path().join("git-started");
    let git_pid_path = temp.path().join("git-pid");
    let descendant_marker = temp.path().join("git-descendant-survived");
    let dump_marker = temp.path().join(".schema-dump-finished");
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let mut search_paths = vec![fake_bin];
    search_paths.extend(std::env::split_paths(&inherited_path));
    let path = std::env::join_paths(search_paths).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["check", "schema", "--no-receipt"])
        .current_dir(temp.path())
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("PATH", path)
        .env("JIG_TEST_SCHEMA_DUMP_FINISHED", &dump_marker)
        .env("JIG_TEST_GIT_STARTED", &started_marker)
        .env("JIG_TEST_GIT_PID", &git_pid_path)
        .env("JIG_TEST_GIT_DESCENDANT_SURVIVED", &descendant_marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while !started_marker.exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "schema check exited with {status} before its Git probe: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !started_marker.exists() {
        let _ = child.kill();
        let _ = child.wait();
        kill_recorded_process_group(&git_pid_path);
        panic!("schema check did not reach its blocking Git status probe");
    }

    // SAFETY: `child.id()` is the live Jig process observed above.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    let status = child
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            kill_recorded_process_group(&git_pid_path);
            panic!("schema check did not cancel its Git probe after SIGINT")
        });

    kill_recorded_process_group(&git_pid_path);
    assert!(
        status.signal() == Some(libc::SIGINT) || status.code() == Some(128 + libc::SIGINT),
        "schema check exited with unexpected status {status}"
    );
    std::thread::sleep(Duration::from_millis(1_250));
    assert!(
        !descendant_marker.exists(),
        "schema Git probe left a descendant running after cancellation"
    );
}

fn kill_recorded_process(pid_path: &std::path::Path) {
    if let Ok(pid) = fs::read_to_string(pid_path)
        .unwrap_or_default()
        .parse::<i32>()
    {
        // SAFETY: this is the test-owned `exec sleep` PID captured before the
        // parent was signalled. ESRCH is harmless if it already exited.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

fn kill_recorded_process_group(pid_path: &std::path::Path) {
    if let Ok(pid) = fs::read_to_string(pid_path)
        .unwrap_or_default()
        .parse::<i32>()
    {
        // SAFETY: the schema Git probe is process-group-owned by Jig. These
        // best-effort signals prevent a failed assertion from leaking the
        // test-owned fake Git process or its descendant.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
            libc::kill(pid, libc::SIGKILL);
        };
    }
}
