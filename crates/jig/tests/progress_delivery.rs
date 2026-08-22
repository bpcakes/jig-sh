#![cfg(target_os = "linux")]

use std::fs;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::tempdir;
#[test]
fn blocked_progress_pipe_does_not_retain_stderr_or_block_error_exit() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"
rust_test_command = "dd if=/dev/zero bs=262144 count=1 2>/dev/null; exit 7"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        r#"{
  "contract_version": 2,
  "jig_version": "0.2.0-beta.1",
  "tool_namespace": "jig",
  "required_commands": ["rust_test_command"],
  "tools": [{
    "name": "jig.test",
    "kind": "command",
    "description": "Run configured tests.",
    "command": "rust_test_command"
  }]
}"#,
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["check", "test", "--no-receipt"])
        .current_dir(temp.path())
        .env("NO_COLOR", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let blocked_stderr = child.stderr.take().unwrap();
    // SAFETY: the descriptor is live and remains owned by `blocked_stderr`.
    // Reducing this test-only pipe makes the production 64 KiB progress
    // preview reliably hit its delivery deadline on every Linux host.
    let resized = unsafe { libc::fcntl(blocked_stderr.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) };
    assert!(resized >= 4096, "failed to constrain test pipe capacity");

    // Poll `waitpid(WNOHANG)` directly instead of installing another SIGCHLD
    // waiter inside Nextest's child-heavy full suite. This outer watchdog only
    // detects the historical infinite stderr-lock wait; production delivery
    // still abandons after 250ms.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("jig retained or wrote through blocked stderr after progress abandonment");
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    assert!(!status.success());
    drop(blocked_stderr);
}
