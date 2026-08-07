#![cfg(any(target_os = "linux", target_os = "macos"))]

mod support;

use std::fs;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::tempdir;
use wait_timeout::ChildExt;

const HELPER_ENV: &str = "JIG_DEV_LIFECYCLE_TEST_HELPER";
const READY_ENV: &str = "JIG_DEV_LIFECYCLE_TEST_READY";
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(12);
static LIFECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn lifecycle_env_port_helper() {
    if std::env::var_os(HELPER_ENV).is_none() {
        return;
    }

    let host = std::env::var("HOST").expect("Jig supplies HOST to env-port apps");
    let port = std::env::var("PORT")
        .expect("Jig supplies PORT to env-port apps")
        .parse::<u16>()
        .expect("PORT is a u16");
    let ready = PathBuf::from(
        std::env::var_os(READY_ENV).expect("outer lifecycle test supplies readiness path"),
    );
    let listener = TcpListener::bind((host.as_str(), port)).expect("helper binds allocated port");
    let temporary = ready.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, format!("{} {port}\n", std::process::id()))
        .expect("write temporary helper marker");
    fs::rename(temporary, ready).expect("publish helper marker");

    loop {
        let _ = &listener;
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn dev_status_stop_and_replace_manage_repo_scoped_sessions() {
    let _guard = LIFECYCLE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = tempdir().expect("create lifecycle test root");
    let repo_a = temp.path().join("repo-a");
    let repo_b = temp.path().join("repo-b");
    let state_dir = temp.path().join("proxy-state");
    fs::create_dir(&repo_a).expect("create first repo");
    fs::create_dir(&repo_b).expect("create second repo");
    write_repo_fixture(&repo_a, "lifecycle-one");
    write_repo_fixture(&repo_b, "lifecycle-two");

    let first_ready = temp.path().join("first-ready");
    let mut first = ForegroundDev::spawn(&repo_a, &state_dir, &first_ready, false);
    first.wait_until_ready(&first_ready);

    let status = wait_for_running_status(&repo_a, &state_dir);
    assert_eq!(status["ok"], true);
    assert_eq!(status["command"], "dev status");
    assert_eq!(status["running"], true);
    assert_eq!(status["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(status["sessions"][0]["status"], "running");
    assert_eq!(status["sessions"][0]["apps"][0]["name"], "lifecycle-helper");
    assert_eq!(status["sessions"][0]["apps"][0]["alive"], true);

    let conflict = run_dev_to_completion(&repo_a, &state_dir, false);
    assert!(!conflict.status.success());
    assert!(conflict.stderr.contains("jig dev stop"));
    assert!(conflict.stderr.contains("jig dev --replace"));
    assert!(
        first.is_running(),
        "a rejected same-repo launch must leave the registered session alive"
    );

    let other_ready = temp.path().join("other-ready");
    let mut other = ForegroundDev::spawn(&repo_b, &state_dir, &other_ready, true);
    other.wait_until_ready(&other_ready);
    assert!(
        first.is_running(),
        "same app names in different repositories must coexist"
    );
    assert!(other.is_running());

    let replacement_ready = temp.path().join("replacement-ready");
    let mut replacement = ForegroundDev::spawn(&repo_a, &state_dir, &replacement_ready, true);
    replacement.wait_until_ready(&replacement_ready);
    first.wait_for_success("replaced foreground dev");
    assert!(
        other.is_running(),
        "same-repository replacement must not stop another repository"
    );

    let replaced_status = run_json(&repo_a, ["dev", "status", "--state-dir"], Some(&state_dir));
    assert_eq!(replaced_status["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(
        replaced_status["sessions"][0]["supervisor_pid"],
        u64::from(replacement.id())
    );

    let stopped = run_json(&repo_a, ["dev", "stop", "--state-dir"], Some(&state_dir));
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["matched_sessions"], 1);
    assert_eq!(stopped["stopped_sessions"], 1);
    assert_eq!(stopped["stopped_apps"], 1);
    assert_eq!(stopped["sessions"], json!([]));
    assert_eq!(stopped["warnings"], json!([]));
    replacement.wait_for_success("stopped foreground dev");
    assert!(
        other.is_running(),
        "repo-scoped stop must leave another repository running"
    );

    let final_status = run_json(&repo_a, ["dev", "status", "--state-dir"], Some(&state_dir));
    assert_eq!(final_status["running"], false);
    assert_eq!(final_status["sessions"], json!([]));

    let repeated = run_json(&repo_a, ["dev", "stop", "--state-dir"], Some(&state_dir));
    assert_eq!(repeated["ok"], true);
    assert_eq!(repeated["matched_sessions"], 0);
    assert_eq!(repeated["stopped_sessions"], 0);

    let other_stopped = run_json(&repo_b, ["dev", "stop", "--state-dir"], Some(&state_dir));
    assert_eq!(other_stopped["ok"], true);
    assert_eq!(other_stopped["matched_sessions"], 1);
    other.wait_for_success("stopped other-repository dev");
}

struct CommandOutput {
    status: ExitStatus,
    stderr: String,
}

fn run_dev_to_completion(repo: &Path, state_dir: &Path, replace: bool) -> CommandOutput {
    let stdout = repo.join(if replace {
        "replace-conflict.stdout"
    } else {
        "same-repo-conflict.stdout"
    });
    let stderr = repo.join(if replace {
        "replace-conflict.stderr"
    } else {
        "same-repo-conflict.stderr"
    });
    let mut command = base_command(repo);
    command
        .arg("dev")
        .arg("--no-proxy")
        .arg("--state-dir")
        .arg(state_dir);
    if replace {
        command.arg("--replace");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
        .stderr(Stdio::from(fs::File::create(&stderr).unwrap()))
        .spawn()
        .expect("spawn conflicting dev command");
    let status = child
        .wait_timeout(COMMAND_TIMEOUT)
        .expect("wait for conflicting dev command")
        .unwrap_or_else(|| {
            let _ = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
            child
                .wait()
                .expect("reap timed-out conflicting dev command")
        });
    CommandOutput {
        status,
        stderr: fs::read_to_string(stderr).expect("read conflicting dev stderr"),
    }
}

fn run_json<const N: usize>(repo: &Path, args: [&str; N], trailing_path: Option<&Path>) -> Value {
    let mut command = base_command(repo);
    command.arg("--json").args(args);
    if let Some(path) = trailing_path {
        command.arg(path);
    }
    let output = command.output().expect("run lifecycle JSON command");
    assert!(
        output.status.success(),
        "lifecycle JSON command failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse lifecycle JSON output: {error}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn wait_for_running_status(repo: &Path, state_dir: &Path) -> Value {
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        let status = run_json(repo, ["dev", "status", "--state-dir"], Some(state_dir));
        if status["sessions"][0]["status"] == "running" {
            return status;
        }
        let now = Instant::now();
        assert!(
            now < deadline,
            "timed out waiting for a registered session to reach running status: {status}"
        );
        thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

fn base_command(repo: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .current_dir(repo)
        .env_remove("JIG_REPO_ROOT")
        .env("NO_COLOR", "1");
    command
}

struct ForegroundDev {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
    armed: bool,
}

impl ForegroundDev {
    fn spawn(repo: &Path, state_dir: &Path, ready: &Path, replace: bool) -> Self {
        let label = if replace { "replacement" } else { "first" };
        let stdout = repo.join(format!("{label}.stdout"));
        let stderr = repo.join(format!("{label}.stderr"));
        let mut command = base_command(repo);
        command
            .process_group(0)
            .arg("--json")
            .arg("dev")
            .arg("--no-proxy")
            .arg("--state-dir")
            .arg(state_dir)
            .env(HELPER_ENV, "1")
            .env(READY_ENV, ready)
            .stdin(Stdio::null())
            .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
            .stderr(Stdio::from(fs::File::create(&stderr).unwrap()));
        if replace {
            command.arg("--replace");
        }
        let child = command.spawn().expect("spawn foreground jig dev");
        Self {
            child,
            stdout,
            stderr,
            armed: true,
        }
    }

    fn id(&self) -> u32 {
        self.child.id()
    }

    fn is_running(&mut self) -> bool {
        self.child
            .try_wait()
            .expect("inspect foreground jig dev")
            .is_none()
    }

    fn wait_until_ready(&mut self, ready: &Path) {
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            if ready.exists() {
                return;
            }
            if let Some(status) = self.child.try_wait().expect("inspect foreground jig dev") {
                self.armed = false;
                panic!(
                    "foreground jig dev exited with {status} before readiness\nstdout:\n{}\nstderr:\n{}",
                    self.read_stdout(),
                    self.read_stderr()
                );
            }
            let now = Instant::now();
            if now >= deadline {
                panic!(
                    "timed out waiting for foreground jig dev readiness\nstdout:\n{}\nstderr:\n{}",
                    self.read_stdout(),
                    self.read_stderr()
                );
            }
            thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
        }
    }

    fn wait_for_success(&mut self, action: &str) {
        let status = self
            .child
            .wait_timeout(COMMAND_TIMEOUT)
            .expect("wait for foreground jig dev")
            .unwrap_or_else(|| panic!("{action} did not exit within {COMMAND_TIMEOUT:?}"));
        self.armed = false;
        assert!(
            status.success(),
            "{action} exited with {status}\nstdout:\n{}\nstderr:\n{}",
            self.read_stdout(),
            self.read_stderr()
        );
    }

    fn read_stdout(&self) -> String {
        fs::read_to_string(&self.stdout).unwrap_or_default()
    }

    fn read_stderr(&self) -> String {
        fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

impl Drop for ForegroundDev {
    fn drop(&mut self) {
        if !self.armed || self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        if self
            .child
            .wait_timeout(Duration::from_secs(5))
            .ok()
            .flatten()
            .is_none()
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn write_repo_fixture(root: &Path, repo_name: &str) {
    let test_exe = serde_json::to_string(&std::env::current_exe().expect("resolve test binary"))
        .expect("quote test binary path");
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/jig-dev-lifecycle-template"
_commit = "test"
repo_name = "{repo_name}"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "true"

[dev]

[[dev.apps]]
name = "lifecycle-helper"
kind = "env-port"
dir = "."
argv = [{test_exe}, "--exact", "lifecycle_env_port_helper", "--nocapture"]
host = "127.0.0.1"
proxy = false

[agent_tooling.codex]
marketplaces = []
"#,
            env!("CARGO_PKG_VERSION"),
        ),
    )
    .expect("write lifecycle Jig config");
    fs::create_dir(root.join(".agent")).expect("create agent directory");
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": env!("CARGO_PKG_VERSION"),
            "required_commands": ["bootstrap_command"],
            "tools": [{
                "name": "jig.bootstrap",
                "kind": "command",
                "description": "Run the configured project bootstrap command.",
                "command": "bootstrap_command",
            }],
        }))
        .expect("serialize lifecycle Jig contract"),
    )
    .expect("write lifecycle Jig contract");
    fs::write(root.join(".mcp.json"), "{}\n").expect("write MCP config");
}
