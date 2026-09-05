#![cfg(unix)]

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use jig_ui::dashboard::{PLAN_ROOT_FIELDS, RECORDER_ROOT_FIELDS};
use serde_json::Value;

#[path = "shared/pty.rs"]
mod pty_support;
mod support;

fn jig(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .current_dir(root)
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .args(args)
        .output()
        .unwrap()
}

fn fixture() -> tempfile::TempDir {
    let root = support::tempdir().unwrap();
    fs::write(
        root.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "0.2.0-beta.1"

[commands]
contract_check_command = "true"

[[status.providers]]
id = "example.provider"
argv = ["sh", "provider.sh"]
timeout_seconds = 60
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("provider.sh"),
        "#!/bin/sh\nprintf '%s' \"$$\" > provider-pid\ntouch provider-started\nwhile :; do sleep 1; done\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".agent")).unwrap();
    fs::write(
        root.path().join(".agent/jig-contract.json"),
        r#"{
  "contract_version": 3,
  "tool_namespace": "jig",
  "jig_version": "0.2.0-beta.1",
  "required_commands": ["contract_check_command"],
  "tools": [{
    "name": "jig.contract_check",
    "kind": "command",
    "description": "Validate fixture contract.",
    "command": "contract_check_command"
  }]
}
"#,
    )
    .unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "fixture@example.com"][..],
        &["config", "user.name", "Fixture"][..],
        &["add", "."][..],
        &["commit", "-q", "-m", "fixture"][..],
    ] {
        assert!(
            Command::new("git")
                .current_dir(root.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    root
}

fn one_json_document(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    parse_one_document(&stdout)
}

fn assert_exact_root_fields(value: &Value, expected: &[&str]) {
    let actual = value
        .as_object()
        .expect("snapshot root must be an object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn recorder_json_exits_without_binding_or_running_providers() {
    let root = fixture();

    let output = jig(root.path(), &["ui", "--json", "--timeline-limit", "1"]);
    let value = one_json_document(&output);

    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "ui");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["snapshot_kind"], "recorder");
    assert_eq!(value["timeline_limit"], 1);
    assert_exact_root_fields(&value, RECORDER_ROOT_FIELDS);
    assert!(!root.path().join("provider-started").exists());
}

#[test]
fn both_interactive_entrypoints_share_the_terminal_requirement() {
    let root = fixture();
    let ui = jig(root.path(), &["ui"]);
    let status = jig(root.path(), &["status", "--tui"]);

    assert!(!ui.status.success());
    assert!(!status.status.success());
    assert_eq!(ui.stderr, status.stderr);
    let error = String::from_utf8(ui.stderr).unwrap();
    assert!(error.contains("`Jig dashboard` requires terminal input and output"));
    assert!(error.contains("jig ui --json"));
    assert!(error.contains("jig status --json"));
    assert!(!root.path().join("provider-started").exists());
}

#[test]
fn plan_json_uses_the_plan_schema_and_missing_plans_use_standard_errors() {
    let root = fixture();
    let started = jig(
        root.path(),
        &[
            "work",
            "start",
            "--title",
            "Example plan",
            "--body",
            "# Example plan",
            "--print-plan-id",
        ],
    );
    assert!(
        started.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&started.stdout),
        String::from_utf8_lossy(&started.stderr)
    );
    let plan_id = String::from_utf8(started.stdout).unwrap();
    let plan_id = plan_id.trim();

    let value = one_json_document(&jig(root.path(), &["ui", "--plan", plan_id, "--json"]));
    assert_eq!(value["command"], "ui");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["snapshot_kind"], "plan");
    assert_eq!(value["plan"]["plan_id"], plan_id);
    assert_exact_root_fields(&value, PLAN_ROOT_FIELDS);
    assert!(!root.path().join("provider-started").exists());

    let missing = jig(root.path(), &["--json", "ui", "--plan", "plan_missing"]);
    assert!(!missing.status.success());
    let error = one_error_document(&missing);
    assert_eq!(error["ok"], false);
    assert_eq!(error["command"], "ui");
    assert_eq!(error["error"]["kind"], "command_failed");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("plan_missing")
    );
}

fn one_error_document(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    parse_one_document(&stdout)
}

fn parse_one_document(stdout: &str) -> Value {
    let mut documents = serde_json::Deserializer::from_str(stdout).into_iter::<Value>();
    let document = documents
        .next()
        .unwrap_or_else(|| panic!("missing JSON output: {stdout:?}"))
        .unwrap();
    assert!(
        documents.next().is_none(),
        "multiple JSON documents: {stdout:?}"
    );
    document
}

#[test]
fn retired_port_is_a_usage_error_before_repository_or_terminal_setup() {
    for (args, json_output) in [
        (&["ui", "--port", "0"][..], false),
        (&["--json", "ui", "--port", "0"][..], true),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jig"))
            .env_remove("JIG_REPO_ROOT")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(combined.contains("browser server"));
        assert!(combined.contains("jig ui --json"));
        if json_output {
            let error = parse_one_document(&String::from_utf8(output.stdout).unwrap());
            assert_eq!(error["ok"], false);
            assert_eq!(error["error"]["kind"], "usage");
            assert_eq!(error["exit_status"], 2);
        }
    }
}

#[test]
fn json_refresh_flags_fail_as_usage_before_repository_loading() {
    for flag in ["--refresh-seconds", "--status-refresh-seconds"] {
        let output = Command::new(env!("CARGO_BIN_EXE_jig"))
            .env_remove("JIG_REPO_ROOT")
            .args(["--json", "ui", flag, "10"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = parse_one_document(&String::from_utf8(output.stdout).unwrap());
        assert_eq!(error["error"]["kind"], "usage");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_jig"))
        .env_remove("JIG_REPO_ROOT")
        .args([
            "--json",
            "ui",
            "--plan",
            "plan_example",
            "--timeline-limit",
            "10",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = parse_one_document(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(error["error"]["kind"], "usage");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--timeline-limit")
    );
}

#[test]
fn ui_json_repository_failures_keep_the_command_identity() {
    let outside_repo = support::tempdir().unwrap();
    let output = jig(outside_repo.path(), &["--json", "ui"]);
    assert!(!output.status.success());
    let error = one_error_document(&output);
    assert_eq!(error["command"], "ui");
    assert_eq!(error["error"]["kind"], "command_failed");

    let root = fixture();
    let root_arg = root.path().to_str().unwrap();
    let output = jig(
        root.path(),
        &[
            "--json",
            "--__launcher-contract-version",
            "999",
            "--__launcher-profile",
            "runtime",
            "--__launcher-repo-root",
            root_arg,
            "ui",
        ],
    );
    assert!(!output.status.success());
    let error = one_error_document(&output);
    assert_eq!(error["command"], "ui");
    assert_eq!(error["error"]["kind"], "command_failed");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("contract version")
    );
}

#[test]
fn product_cutover_does_not_change_the_generated_contract_epoch() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.3.0");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let contract: Value =
        serde_json::from_slice(&fs::read(root.join(".agent/jig-contract.json")).unwrap()).unwrap();
    assert_eq!(contract["contract_version"], 7);
    let launcher = fs::read_to_string(root.join("scripts/jig")).unwrap();
    assert!(launcher.contains("CONTRACT_VERSION=\"7\""));
}

#[test]
fn interactive_ui_starts_on_work_and_opens_the_requested_plan() {
    let root = fixture();
    let started = jig(
        root.path(),
        &[
            "work",
            "start",
            "--title",
            "Example plan detail",
            "--body",
            "# Example plan body",
            "--print-plan-id",
        ],
    );
    assert!(started.status.success());
    let plan_id = String::from_utf8(started.stdout).unwrap();

    let (mut master, mut child) = dashboard_child(root.path(), &["ui", "--plan", plan_id.trim()]);
    let mut terminal_output = Vec::new();
    wait_for_output(
        &mut child,
        &mut master,
        &mut terminal_output,
        b"Example plan detail",
    );
    master.write_all(b"q").unwrap();
    let status = pty_support::wait_for_child_while_draining(
        &mut child,
        &mut master,
        &mut terminal_output,
        Duration::from_secs(10),
    )
    .expect("dashboard did not stop after q");
    assert!(status.success());
    assert!(
        terminal_output
            .windows(b"Work".len())
            .any(|window| window == b"Work")
    );
    assert!(
        terminal_output
            .windows(b"\x1b[?1049l".len())
            .any(|window| window == b"\x1b[?1049l")
    );
}

#[test]
fn termination_signals_cancel_status_and_restore_the_terminal() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        assert_signal_cleanup(signal);
    }
}

fn assert_signal_cleanup(signal: libc::c_int) {
    let root = fixture();
    let (mut master, mut child) = dashboard_child(root.path(), &["status", "--tui"]);
    wait_for_path(&mut child, &root.path().join("provider-started"));

    // SAFETY: `child.id()` is the live Jig process observed above.
    assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
    let mut terminal_output = Vec::new();
    let status = pty_support::wait_for_child_while_draining(
        &mut child,
        &mut master,
        &mut terminal_output,
        Duration::from_secs(10),
    )
    .expect("dashboard did not stop after its termination signal");
    assert_eq!(status.signal(), Some(signal));
    let provider_pid: libc::pid_t = fs::read_to_string(root.path().join("provider-pid"))
        .unwrap()
        .parse()
        .unwrap();
    // SAFETY: signal zero performs a liveness check without delivering a signal.
    assert_eq!(unsafe { libc::kill(provider_pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    assert!(
        terminal_output
            .windows(b"\x1b[?1049l".len())
            .any(|window| window == b"\x1b[?1049l"),
        "alternate screen was not restored: {:?}",
        String::from_utf8_lossy(&terminal_output)
    );
}

fn dashboard_child(root: &Path, args: &[&str]) -> (fs::File, pty_support::ChildGuard) {
    let (master, slave) = pseudo_terminal(120, 30).unwrap();
    let stdin = slave.try_clone().unwrap();
    let stdout = slave.try_clone().unwrap();
    let stderr = slave.try_clone().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(root)
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("TERM", "xterm-256color")
        .args(args)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    make_stdin_controlling_terminal(&mut command);
    let child = pty_support::ChildGuard::new(command.spawn().unwrap());
    set_nonblocking(&master);
    (master, child)
}

fn wait_for_output(
    child: &mut pty_support::ChildGuard,
    master: &mut fs::File,
    output: &mut Vec<u8>,
    needle: &[u8],
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !output.windows(needle.len()).any(|window| window == needle) && Instant::now() < deadline
    {
        pty_support::read_available(master, output);
        if let Some(status) = child.try_wait().unwrap() {
            panic!("dashboard exited with {status} before rendering {needle:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        output.windows(needle.len()).any(|window| window == needle),
        "dashboard did not render {:?}: {:?}",
        String::from_utf8_lossy(needle),
        String::from_utf8_lossy(output)
    );
}

fn wait_for_path(child: &mut pty_support::ChildGuard, path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("dashboard exited with {status} before provider startup");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "dashboard did not start its provider");
}

fn pseudo_terminal(columns: u16, rows: u16) -> std::io::Result<(fs::File, fs::File)> {
    // SAFETY: each successful descriptor is immediately wrapped exactly once.
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `master` is a newly owned descriptor.
    let master = unsafe { fs::File::from_raw_fd(master) };
    // SAFETY: the descriptor is a live PTY master owned by `master`.
    if unsafe { libc::grantpt(master.as_raw_fd()) } != 0
        || unsafe { libc::unlockpt(master.as_raw_fd()) } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `ptsname` returns storage managed by libc for this live master.
    let name = unsafe { libc::ptsname(master.as_raw_fd()) };
    if name.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `name` is the NUL-terminated slave path returned above.
    let slave = unsafe { libc::open(name, libc::O_RDWR | libc::O_NOCTTY) };
    if slave < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `slave` is a newly owned descriptor.
    let slave = unsafe { fs::File::from_raw_fd(slave) };
    let size = libc::winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `slave` is live and `size` points to initialized storage.
    if unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    set_close_on_exec(&master)?;
    set_close_on_exec(&slave)?;
    Ok((master, slave))
}

fn set_close_on_exec(file: &fs::File) -> std::io::Result<()> {
    // SAFETY: fcntl reads and updates descriptor flags for this live file descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    if flags == -1
        // SAFETY: no library preconditions beyond the live descriptor above.
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn make_stdin_controlling_terminal(command: &mut Command) {
    // SAFETY: the child closure runs after stdio remapping and invokes only
    // async-signal-safe session and terminal ioctls before exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as libc::c_ulong, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn set_nonblocking(file: &fs::File) {
    // SAFETY: fcntl reads and updates flags for this live file descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_ne!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        -1
    );
}
