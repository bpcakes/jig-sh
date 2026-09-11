#![cfg(any(target_os = "linux", target_os = "macos"))]

mod support;

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::TempDir;
use wait_timeout::ChildExt;

const FIXTURE_ENV: &str = "JIG_DEV_LAUNCHER_LOSS_FIXTURE";
const ROLE_ENV: &str = "JIG_DEV_LAUNCHER_LOSS_ROLE";
const TIMEOUT: Duration = Duration::from_secs(12);
const POLL: Duration = Duration::from_millis(20);

#[test]
fn launcher_loss_helper() {
    let Some(directory) = std::env::var_os(FIXTURE_ENV).map(PathBuf::from) else {
        return;
    };
    let role = std::env::var(ROLE_ENV).expect("fixture supplies helper role");
    unsafe {
        // SAFETY: this isolated helper installs the standard ignore disposition.
        // Both generations resist TERM so cleanup must reach the whole group.
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
    }
    let identity = ProcessIdentity::capture(std::process::id());
    publish(&directory.join(&role), &identity);
    let mut descendant = (role != "server").then(|| {
        Command::new(std::env::current_exe().expect("resolve fixture executable"))
            .args(["--exact", "launcher_loss_helper", "--nocapture"])
            .env(ROLE_ENV, "server")
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn server grandchild")
    });
    let listener = (role == "server").then(|| {
        let port = std::env::var("PORT").unwrap_or_else(|_| "0".into());
        let listener =
            TcpListener::bind(format!("127.0.0.1:{port}")).expect("server binds its assigned port");
        publish(
            &directory.join("ready"),
            &listener
                .local_addr()
                .expect("inspect listener address")
                .port(),
        );
        listener
    });
    // Failure cleanup has independent authority over these test helpers through
    // this release file. The hard limit also bounds a crashed test harness.
    let deadline = Instant::now() + Duration::from_secs(45);
    while !directory.join("release").exists() && Instant::now() < deadline {
        thread::sleep(POLL);
    }
    drop(listener);
    if let Some(child) = &mut descendant
        && child
            .wait_timeout(Duration::from_secs(2))
            .unwrap()
            .is_none()
    {
        child.kill().expect("kill fixture's directly owned server");
        child.wait().expect("reap fixture's directly owned server");
    }
}

#[test]
fn launcher_loss_initialization_failure_helper() {
    let Some(directory) = std::env::var_os(FIXTURE_ENV).map(PathBuf::from) else {
        return;
    };
    publish(
        &directory.join("initializing"),
        &ProcessIdentity::capture(std::process::id()),
    );
    let deadline = Instant::now() + Duration::from_secs(45);
    while !directory.join("fail-initialization").exists()
        && !directory.join("release").exists()
        && Instant::now() < deadline
    {
        thread::sleep(POLL);
    }
    eprintln!("fixture initialization failed");
    std::process::exit(1);
}

#[test]
fn killing_dev_launcher_cleans_wrapper_and_server_and_allows_next_launch() {
    let fixture = Fixture::new(false);
    let mut launch = fixture.launch("first", false);
    let port = launch.wait_for_marker::<u16>("ready");
    let wrapper = launch.wait_for_marker::<ProcessIdentity>("wrapper");
    let server = launch.wait_for_marker::<ProcessIdentity>("server");
    let session = fixture.wait_for_session();
    let worker = worker_identity(&session, &launch);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());

    launch.kill_launcher(false);

    wait_for_exit(&[&wrapper, &server, &worker]);
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "server listener survived launcher SIGKILL"
    );
    fixture.wait_for_empty_registry();
    let mut next = fixture.launch("next", true);
    next.wait_for_marker::<u16>("ready");
    let stopped = fixture.management("stop");
    assert_eq!(stopped["ok"], true);
    assert_eq!(stopped["stopped_apps"], 1);
    assert!(
        next.wait().success(),
        "replacement did not stop successfully"
    );
    fixture.wait_for_empty_registry();
}

#[test]
fn killing_launcher_group_during_preflight_cleans_checker_descendants() {
    let fixture = Fixture::new(true);
    let mut launch = fixture.launch("preflight", false);
    let checker = launch.wait_for_marker::<ProcessIdentity>("checker");
    let server = launch.wait_for_marker::<ProcessIdentity>("server");
    let session = fixture.wait_for_session();
    let worker = worker_identity(&session, &launch);
    assert_eq!(session["status"], "starting");
    assert_eq!(session["apps"][0]["pid"], Value::Null);

    launch.kill_launcher(true);

    wait_for_exit(&[&checker, &server, &worker]);
    fixture.wait_for_empty_registry();
    assert!(
        !launch.directory.join("wrapper").exists(),
        "the application started after launcher loss during preflight"
    );
}

#[test]
fn queued_launcher_signals_preserve_force_and_first_exit_for_a_paused_worker() {
    let fixture = Fixture::new(false);
    let mut launch = fixture.launch("queued-signals", false);
    launch.wait_for_marker::<u16>("ready");
    let wrapper = launch.wait_for_marker::<ProcessIdentity>("wrapper");
    let server = launch.wait_for_marker::<ProcessIdentity>("server");
    let worker = worker_identity(&fixture.wait_for_session(), &launch);
    let state_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.root.path().join("proxy-state/routes.lock"))
        .expect("open shared session-state lock");
    state_lock
        .lock_exclusive()
        .expect("hold session retirement");
    let paused = PausedWorker::new(&worker);
    for signal in [libc::SIGINT, libc::SIGTERM] {
        assert_eq!(
            unsafe { libc::kill(launch.child.id() as libc::pid_t, signal) },
            0
        );
        // Give the running frontend separate polling turns while its worker is
        // stopped. Repeated forwarding of one OS signal would coalesce here.
        thread::sleep(Duration::from_millis(250));
    }
    drop(paused);

    // A lost escalation spends up to 30 seconds on this lock. Confirm forced
    // exit while it remains held, without a tight process-scheduling deadline.
    let status = launch
        .child
        .wait_timeout(Duration::from_secs(6))
        .unwrap()
        .expect("queued escalation must bypass contended session retirement");
    assert_eq!(status.code(), Some(130));
    let output: Value = serde_json::from_slice(
        &fs::read(launch.directory.join("stdout")).expect("read queued-signal output"),
    )
    .expect("queued signals emit one JSON result");
    assert_eq!(output["termination_signal"], "SIGINT");
    assert_eq!(output["exit_status"], 130);
    wait_for_exit(&[&wrapper, &server, &worker]);
    FileExt::unlock(&state_lock).unwrap();
    // Forced process cleanup may deliberately retain contended session state.
    assert_eq!(fixture.management("stop")["ok"], true);
    fixture.wait_for_empty_registry();
}

#[test]
fn replacement_recovery_survives_app_initialization_failure_without_stale_session() {
    let fixture = Fixture::new(false);
    let mut first = fixture.launch("orphan", false);
    first.wait_for_marker::<u16>("ready");
    let wrapper = first.wait_for_marker::<ProcessIdentity>("wrapper");
    let server = first.wait_for_marker::<ProcessIdentity>("server");
    let session = fixture.wait_for_session();
    let worker = worker_identity(&session, &first);
    // Deliberately kill the owning worker to exercise the documented boundary.
    // Test helpers then exit cooperatively, leaving a safely recoverable record.
    assert!(worker.is_live());
    assert_eq!(unsafe { libc::kill(worker.pid, libc::SIGKILL) }, 0);
    assert_eq!(first.wait().code(), Some(137));
    let lost: Value = serde_json::from_slice(
        &fs::read(first.directory.join("stdout")).expect("read lost-worker output"),
    )
    .expect("lost worker emits one JSON error");
    assert_eq!(lost["ok"], false);
    assert_eq!(lost["exit_status"], 137);
    assert_eq!(lost["error"]["kind"], "dev_supervisor_lost");
    first.release_helpers();
    wait_for_exit(&[&wrapper, &server, &worker]);
    fixture.write_config(false, true);

    let mut replacement = fixture.launch("failed-replacement", true);
    let initializing = replacement.wait_for_marker::<ProcessIdentity>("initializing");
    wait_until("initializing app registration", || {
        fixture.management("status")["sessions"][0]["apps"][0]["pid"] == initializing.pid
    });
    fs::write(replacement.directory.join("fail-initialization"), "fail\n").unwrap();
    assert_eq!(replacement.wait().code(), Some(1));
    let output: Value = serde_json::from_slice(
        &fs::read(replacement.directory.join("stdout")).expect("read replacement output"),
    )
    .expect("replacement failure emits one JSON value");
    assert_eq!(output["ok"], false);
    assert_eq!(output["exit_status"], 1);
    assert_eq!(output["recoveries"].as_array().unwrap().len(), 1);
    assert_eq!(output["recoveries"][0]["session_id"], session["session_id"]);
    assert_eq!(output["recoveries"][0]["kind"], "dead-orphan-retired");
    assert!(
        output["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exited before listening"),
        "new app failure was obscured: {output}"
    );
    assert!(
        fs::read_to_string(replacement.directory.join("stderr"))
            .unwrap()
            .contains("fixture initialization failed"),
        "the failed app's captured initialization error was not reported"
    );
    assert!(!replacement.directory.join("ready").exists());
    wait_for_exit(&[&initializing]);
    fixture.wait_for_empty_registry();
}

struct PausedWorker<'a>(&'a ProcessIdentity);

impl<'a> PausedWorker<'a> {
    fn new(worker: &'a ProcessIdentity) -> Self {
        assert!(worker.is_live());
        let paused = Self(worker);
        assert_eq!(unsafe { libc::kill(worker.pid, libc::SIGSTOP) }, 0);
        wait_until("owning worker to stop", || {
            Command::new("ps")
                .args(["-p", &worker.pid.to_string(), "-o", "stat="])
                .output()
                .is_ok_and(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .starts_with('T')
                })
        });
        paused
    }
}

impl Drop for PausedWorker<'_> {
    fn drop(&mut self) {
        if self.0.is_live() {
            let _ = unsafe { libc::kill(self.0.pid, libc::SIGCONT) };
        }
    }
}

fn worker_identity(session: &Value, launch: &Launch) -> ProcessIdentity {
    let pid = session["supervisor_pid"].as_u64().unwrap() as u32;
    assert_ne!(
        pid,
        launch.child.id(),
        "CLI must have a separate owning worker"
    );
    let identity = ProcessIdentity::capture(pid);
    assert!(identity.is_live());
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "ppid="])
        .output()
        .expect("inspect worker parent");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        launch.child.id().to_string(),
        "registered supervisor must be the launcher's directly owned worker"
    );
    assert_eq!(
        unsafe { libc::getsid(identity.pid) },
        identity.pid,
        "worker must survive termination of the launcher's process group"
    );
    identity
}

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new(preflight: bool) -> Self {
        let fixture = Self {
            root: support::tempdir().expect("create isolated launcher-loss fixture"),
        };
        let root = fixture.root.path();
        fs::create_dir(root.join(".agent")).unwrap();
        fs::write(root.join(".mcp.json"), "{}\n").unwrap();
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_vec(&json!({
                "contract_version": 3,
                "tool_namespace": "jig",
                "jig_version": env!("CARGO_PKG_VERSION"),
                "required_commands": ["bootstrap_command"],
                "tools": [{
                    "name": "jig.bootstrap", "kind": "command",
                    "description": "Bootstrap the generic fixture.",
                    "command": "bootstrap_command"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        if preflight {
            fs::create_dir(root.join("web")).unwrap();
            fs::create_dir(root.join("scripts")).unwrap();
            fs::write(
                root.join("scripts/check-webapps.sh"),
                "#!/usr/bin/env bash\nset -euo pipefail\nexport JIG_DEV_LAUNCHER_LOSS_ROLE=checker\nexec \"$JIG_DEV_LAUNCHER_LOSS_EXE\" --exact launcher_loss_helper --nocapture\n",
            )
            .unwrap();
        }
        fixture.write_config(preflight, false);
        fixture
    }

    fn write_config(&self, preflight: bool, fail_initialization: bool) {
        let executable = serde_json::to_string(&std::env::current_exe().unwrap()).unwrap();
        let helper = if fail_initialization {
            "launcher_loss_initialization_failure_helper"
        } else {
            "launcher_loss_helper"
        };
        let frontend = if preflight {
            "[[frontend_apps]]\nname = \"example-api\"\ndir = \"web\"\ncoverage_threshold = 80\n"
        } else {
            ""
        };
        let directory = if preflight { "web" } else { "." };
        fs::write(
            self.root.path().join(".jig.toml"),
            format!(
                r#"_src_path = "/tmp/jig-launcher-loss-template"
_commit = "test"
repo_name = "ExampleProject"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "true"
{frontend}
[dev]
[[dev.apps]]
name = "example-api"
kind = "env-port"
dir = "{directory}"
argv = [{executable}, "--exact", "{helper}", "--nocapture"]
host = "127.0.0.1"
proxy = {fail_initialization}
[agent_tooling.codex]
marketplaces = []
"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
        command
            .current_dir(self.root.path())
            .env_remove("JIG_REPO_ROOT")
            .env("NO_COLOR", "1")
            .args(["--json", "dev"]);
        command
    }

    fn launch(&self, label: &str, replace: bool) -> Launch {
        let directory = self.root.path().join(label);
        fs::create_dir(&directory).unwrap();
        let mut command = self.command();
        command
            .process_group(0)
            .args(["--http-port", "0", "--state-dir"])
            .arg(self.root.path().join("proxy-state"))
            .env(FIXTURE_ENV, &directory)
            .env(ROLE_ENV, "wrapper")
            .env(
                "JIG_DEV_LAUNCHER_LOSS_EXE",
                std::env::current_exe().unwrap(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::from(
                fs::File::create(directory.join("stdout")).unwrap(),
            ))
            .stderr(Stdio::from(
                fs::File::create(directory.join("stderr")).unwrap(),
            ));
        if replace {
            command.arg("--replace");
        }
        Launch {
            child: command.spawn().expect("spawn fixture Jig launcher"),
            directory,
        }
    }

    fn management(&self, subcommand: &str) -> Value {
        let output = self
            .command()
            .args([subcommand, "--state-dir"])
            .arg(self.root.path().join("proxy-state"))
            .output()
            .expect("run isolated session management");
        assert!(
            output.status.success(),
            "management failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("parse management JSON")
    }

    fn wait_for_session(&self) -> Value {
        let mut session = Value::Null;
        wait_until("session registration", || {
            session = self.management("status")["sessions"][0].clone();
            !session.is_null()
        });
        session
    }

    fn wait_for_empty_registry(&self) {
        wait_until("automatic session retirement", || {
            self.management("status")["sessions"] == json!([])
        });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A readiness failure can leave the shared proxy running. Keep its
        // cleanup armed for assertion failures and confine it to fixture state.
        let _ = Command::new(env!("CARGO_BIN_EXE_jig"))
            .current_dir(self.root.path())
            .env_remove("JIG_REPO_ROOT")
            .args(["proxy", "stop", "--http-port", "0", "--state-dir"])
            .arg(self.root.path().join("proxy-state"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

struct Launch {
    child: Child,
    directory: PathBuf,
}

impl Launch {
    fn wait_for_marker<T: serde::de::DeserializeOwned>(&mut self, name: &str) -> T {
        let path = self.directory.join(name);
        wait_until(name, || {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "launcher exited before {name}: {}",
                fs::read_to_string(self.directory.join("stderr")).unwrap_or_default()
            );
            path.exists()
        });
        serde_json::from_slice(&fs::read(path).unwrap()).expect("parse helper marker")
    }

    fn kill_launcher(&mut self, process_group: bool) {
        let pid = self.child.id() as libc::pid_t;
        assert!(self.child.try_wait().unwrap().is_none());
        // The test owns this unreaped direct child; its group was set at spawn.
        let target = if process_group { -pid } else { pid };
        assert_eq!(unsafe { libc::kill(target, libc::SIGKILL) }, 0);
        assert_eq!(self.wait().signal(), Some(libc::SIGKILL));
    }

    fn wait(&mut self) -> ExitStatus {
        self.child
            .wait_timeout(TIMEOUT)
            .expect("wait for launcher")
            .unwrap_or_else(|| {
                panic!(
                    "launcher did not exit: {}",
                    fs::read_to_string(self.directory.join("stderr")).unwrap_or_default()
                )
            })
    }

    fn release_helpers(&self) {
        fs::write(self.directory.join("release"), "release\n").unwrap();
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        let _ = fs::write(self.directory.join("release"), "release\n");
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
            if self.child.wait_timeout(TIMEOUT).ok().flatten().is_none() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        // Keep the release file present until every helper that published its
        // identity has observed it, including after the launcher was reaped.
        let deadline = Instant::now() + TIMEOUT;
        while ["wrapper", "checker", "server", "initializing"]
            .iter()
            .any(|name| {
                fs::read(self.directory.join(name))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<ProcessIdentity>(&bytes).ok())
                    .is_some_and(|identity| identity.is_live())
            })
            && Instant::now() < deadline
        {
            thread::sleep(POLL);
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ProcessIdentity {
    pid: libc::pid_t,
    start_token: String,
}

impl ProcessIdentity {
    fn capture(pid: u32) -> Self {
        let pid = pid.try_into().expect("fixture PID fits pid_t");
        Self {
            pid,
            start_token: process_start_token(pid).expect("capture live fixture process identity"),
        }
    }

    fn is_live(&self) -> bool {
        process_start_token(self.pid).as_deref() == Some(&self.start_token)
    }
}

#[cfg(target_os = "linux")]
fn process_start_token(pid: libc::pid_t) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let mut fields = fields.split_whitespace();
    if matches!(fields.next()?, "Z" | "X" | "x") {
        return None;
    }
    Some(format!("linux:{}", fields.nth(18)?))
}

#[cfg(target_os = "macos")]
fn process_start_token(pid: libc::pid_t) -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes = unsafe {
        // SAFETY: proc_pidinfo receives a writable buffer of the stated size.
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size.try_into().ok()?,
        )
    };
    if bytes != size.try_into().ok()? {
        return None;
    }
    // SAFETY: proc_pidinfo reported a complete initialized result.
    let info = unsafe { info.assume_init() };
    (info.pbi_status != libc::SZOMB)
        .then(|| format!("macos:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec))
}

fn publish(path: &Path, value: &impl Serialize) {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec(value).unwrap()).unwrap();
    fs::rename(temporary, path).unwrap();
}

fn wait_for_exit(processes: &[&ProcessIdentity]) {
    wait_until(&format!("process tree exit: {processes:?}"), || {
        processes.iter().all(|identity| !identity.is_live())
    });
}

fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        thread::sleep(POLL);
    }
}
