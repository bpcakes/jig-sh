#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt;
use serde_json::{Value, json};
use tempfile::{NamedTempFile, tempdir};
use wait_timeout::ChildExt;

const HELPER_ENV: &str = "JIG_DEV_SIGNAL_TEST_HELPER";
const READY_ENV: &str = "JIG_DEV_SIGNAL_TEST_READY";
const STARTED_ENV: &str = "JIG_DEV_SIGNAL_TEST_STARTED";
const BEFORE_LISTEN_ENV: &str = "JIG_DEV_SIGNAL_TEST_BEFORE_LISTEN";
const LISTEN_GATE_ENV: &str = "JIG_DEV_SIGNAL_TEST_LISTEN_GATE";
const EXIT_ENV: &str = "JIG_DEV_SIGNAL_TEST_EXIT";
const STUBBORN_DESCENDANT_ENV: &str = "JIG_DEV_SIGNAL_TEST_STUBBORN_DESCENDANT";
const FIRST_TERM_ENV: &str = "JIG_DEV_SIGNAL_TEST_FIRST_TERM";
const SIGNAL_EFFECT_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_TREE_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(20);
static SIGNAL_TEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct SignalCase {
    signal: libc::c_int,
    exit_status: i32,
    label: &'static str,
}

#[derive(Clone, Copy)]
enum ForegroundMode {
    Dev,
    ProxyRun,
}

impl ForegroundMode {
    const fn command_name(self) -> &'static str {
        match self {
            Self::Dev => "jig dev",
            Self::ProxyRun => "jig proxy run",
        }
    }
}

#[test]
fn env_port_helper() {
    if std::env::var_os(HELPER_ENV).is_none() {
        return;
    }

    let host = std::env::var("HOST").expect("Jig supplies HOST to env-port apps");
    let port = std::env::var("PORT")
        .expect("Jig supplies PORT to env-port apps")
        .parse::<u16>()
        .expect("PORT is a u16");
    let ready = std::env::var_os(READY_ENV).expect("outer test supplies readiness path");
    let helper_identity = VerifiedProcessIdentity::capture(
        std::process::id()
            .try_into()
            .expect("helper pid fits pid_t"),
    )
    .expect("capture helper process identity");
    if let Some(before_listen) = std::env::var_os(BEFORE_LISTEN_ENV) {
        publish_started_marker(
            Path::new(&before_listen),
            std::slice::from_ref(&helper_identity),
        );
        let gate =
            std::env::var_os(LISTEN_GATE_ENV).expect("before-listen helper requires a listen gate");
        while !Path::new(&gate).exists() {
            thread::sleep(Duration::from_millis(10));
        }
    }
    let listener = TcpListener::bind((host.as_str(), port)).expect("helper binds allocated port");
    if let Some(started) = std::env::var_os(STARTED_ENV) {
        publish_started_marker(Path::new(&started), std::slice::from_ref(&helper_identity));
    }
    let descendant_script = if std::env::var_os(STUBBORN_DESCENDANT_ENV).is_some() {
        "trap 'printf term > \"$1\"' TERM; : > \"$2\"; while :; do sleep 1; done"
    } else {
        "sleep 60"
    };
    let mut descendant_command = Command::new("sh");
    descendant_command.args(["-c", descendant_script]);
    let descendant_ready = if std::env::var_os(STUBBORN_DESCENDANT_ENV).is_some() {
        let first_term = PathBuf::from(
            std::env::var_os(FIRST_TERM_ENV)
                .expect("stubborn descendant test supplies first-TERM marker"),
        );
        let ready = first_term.with_extension("ready");
        descendant_command.arg("sh").arg(&first_term).arg(&ready);
        Some(ready)
    } else {
        None
    };
    let descendant = descendant_command.spawn().expect("spawn helper descendant");
    let descendant_identity = VerifiedProcessIdentity::capture(
        descendant
            .id()
            .try_into()
            .expect("descendant pid fits pid_t"),
    )
    .expect("capture helper descendant identity");
    // This helper deliberately remains alive until Jig terminates its whole
    // process group. The outer test owns liveness/reap assertions; waiting here
    // would prevent the helper from publishing readiness.
    std::mem::forget(descendant);
    if let Some(started) = std::env::var_os(STARTED_ENV) {
        publish_started_marker(
            Path::new(&started),
            &[helper_identity.clone(), descendant_identity.clone()],
        );
    }
    if let Some(descendant_ready) = descendant_ready {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !descendant_ready.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            descendant_ready.exists(),
            "stubborn descendant did not install its TERM handler"
        );
    }
    let ready = Path::new(&ready);
    let ready_tmp = ready.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(
        &ready_tmp,
        format!(
            "{} {} {port} {} {}\n",
            helper_identity.pid,
            helper_identity.start_token,
            descendant_identity.pid,
            descendant_identity.start_token
        ),
    )
    .expect("helper writes temporary readiness marker");
    fs::rename(&ready_tmp, ready).expect("helper atomically publishes readiness marker");

    loop {
        let _ = &listener;
        if std::env::var_os(EXIT_ENV).is_some_and(|path| Path::new(&path).exists()) {
            eprintln!("late-signal failure tail");
            std::process::exit(42);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn dev_process_list_identifies_its_repo() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = tempdir().expect("create process-identity test repo");
    let ready_path = repo.path().join("helper-ready");
    let started_path = repo.path().join("helper-started");
    write_repo_fixture(repo.path());

    let stdout_file = NamedTempFile::new().expect("create stdout capture");
    let stderr_file = NamedTempFile::new().expect("create stderr capture");
    let child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .process_group(0)
        .args(["dev", "--no-proxy"])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready_path)
        .env(STARTED_ENV, &started_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.reopen().unwrap()))
        .stderr(Stdio::from(stderr_file.reopen().unwrap()))
        .spawn()
        .expect("spawn identifiable jig dev");
    let mut child = ForegroundChildGuard::new(child, started_path);
    wait_for_file_with_output(
        &ready_path,
        &mut child,
        Duration::from_secs(10),
        stdout_file.path(),
        stderr_file.path(),
    );
    let (helper, _port, descendant) = read_helper_marker(&ready_path);

    let output = Command::new("ps")
        .args(["-p", &child.id().to_string(), "-o", "command="])
        .output()
        .expect("inspect jig dev process command");
    assert!(
        output.status.success(),
        "ps failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let command = String::from_utf8(output.stdout).expect("ps command is UTF-8");
    let root = fs::canonicalize(repo.path()).expect("canonicalize test repo");
    let expected = format!(
        "jig dev --jig-project=signal-test@{}",
        root.to_string_lossy()
    );
    assert!(
        command.contains(&expected),
        "jig dev process did not identify its repo\nexpected: {expected}\nactual: {command}"
    );

    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait for identifiable jig dev")
        .expect("identifiable jig dev exits after SIGINT");
    assert_eq!(status.code(), Some(130));
    assert_verified_process_tree_exited(
        &[("helper", &helper), ("descendant", &descendant)],
        PROCESS_TREE_EXIT_TIMEOUT,
    );
    child.disarm();
}

#[test]
fn jig_foreground_commands_have_structured_signal_exits_and_clean_children() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for case in [
        SignalCase {
            signal: libc::SIGINT,
            exit_status: 130,
            label: "SIGINT",
        },
        SignalCase {
            signal: libc::SIGHUP,
            exit_status: 129,
            label: "SIGHUP",
        },
        SignalCase {
            signal: libc::SIGTERM,
            exit_status: 143,
            label: "SIGTERM",
        },
    ] {
        for mode in [ForegroundMode::Dev, ForegroundMode::ProxyRun] {
            run_signal_case(case, mode, false);
            run_signal_case(case, mode, true);
        }
    }
}

#[test]
fn sigint_during_frontend_preflight_stops_checker_tree_before_app_launch() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = tempdir().expect("create preflight signal repo");
    let checker_ready = repo.path().join("checker-ready");
    let app_started = repo.path().join("app-started");
    let app_ready = repo.path().join("app-ready");
    write_preflight_signal_fixture(repo.path());

    let stdout_file = NamedTempFile::new().expect("create stdout capture");
    let stderr_file = NamedTempFile::new().expect("create stderr capture");
    let child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .process_group(0)
        .args(["--json", "dev", "--no-proxy"])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env("JIG_PREFLIGHT_SIGNAL_READY", &checker_ready)
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &app_ready)
        .env(STARTED_ENV, &app_started)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.reopen().unwrap()))
        .stderr(Stdio::from(stderr_file.reopen().unwrap()))
        .spawn()
        .expect("spawn jig dev preflight");
    let mut child = ForegroundChildGuard::new(child, app_started.clone());
    wait_for_file_with_output(
        &checker_ready,
        &mut child,
        Duration::from_secs(10),
        stdout_file.path(),
        stderr_file.path(),
    );
    let marker = fs::read_to_string(&checker_ready).expect("read checker process marker");
    let mut fields = marker.split_whitespace();
    let checker_pid = fields.next().unwrap().parse::<libc::pid_t>().unwrap();
    let descendant_pid = fields.next().unwrap().parse::<libc::pid_t>().unwrap();
    let checker =
        VerifiedProcessIdentity::capture(checker_pid).expect("capture preflight checker identity");
    let descendant = VerifiedProcessIdentity::capture(descendant_pid)
        .expect("capture preflight checker descendant identity");

    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = child
        .wait_timeout(Duration::from_secs(5))
        .expect("wait for preflight interruption")
        .unwrap_or_else(|| {
            let _ = child.kill();
            let _ = child.wait();
            terminate_verified_process(&checker);
            terminate_verified_process(&descendant);
            panic!("jig dev did not stop after preflight SIGINT");
        });
    let checker_stopped = wait_for_verified_exit(&checker, Duration::from_secs(5));
    let descendant_stopped = wait_for_verified_exit(&descendant, Duration::from_secs(5));
    if !checker_stopped {
        terminate_verified_process(&checker);
    }
    if !descendant_stopped {
        terminate_verified_process(&descendant);
    }
    child.disarm();

    assert_eq!(status.code(), Some(130));
    assert!(checker_stopped, "preflight checker survived SIGINT cleanup");
    assert!(
        descendant_stopped,
        "preflight checker descendant survived SIGINT cleanup"
    );
    assert!(
        !app_started.exists(),
        "development app started after SIGINT"
    );
    let output: Value = serde_json::from_slice(
        &fs::read(stdout_file.path()).expect("read structured preflight output"),
    )
    .expect("preflight interruption emits one JSON result");
    assert_eq!(output["ok"], false);
    assert_eq!(output["interrupted"], true);
    assert_eq!(output["exit_status"], 130);
    assert_eq!(output["termination_signal"], "SIGINT");
    assert_eq!(output["routes"], json!([]));
}

#[test]
fn second_signal_while_route_publication_is_locked_is_prompt_and_sticky() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = tempdir().expect("create route-publication signal repo");
    let state_dir = repo.path().join("state");
    let before_listen = repo.path().join("before-listen");
    let listen_gate = repo.path().join("listen-gate");
    let ready_path = repo.path().join("helper-ready");
    let started_path = repo.path().join("helper-started");
    let first_term = repo.path().join("first-term");
    let proxy_port = 0;
    let proxy_port_arg = proxy_port.to_string();
    write_repo_fixture_with_proxy(repo.path(), true);

    let stdout_file = NamedTempFile::new().expect("create stdout capture");
    let stderr_file = NamedTempFile::new().expect("create stderr capture");
    let child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .process_group(0)
        .args(["--json", "dev", "--state-dir"])
        .arg(&state_dir)
        .args(["--http-port", &proxy_port_arg])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready_path)
        .env(STARTED_ENV, &started_path)
        .env(BEFORE_LISTEN_ENV, &before_listen)
        .env(LISTEN_GATE_ENV, &listen_gate)
        .env(STUBBORN_DESCENDANT_ENV, "1")
        .env(FIRST_TERM_ENV, &first_term)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.reopen().unwrap()))
        .stderr(Stdio::from(stderr_file.reopen().unwrap()))
        .spawn()
        .expect("spawn proxied jig dev");
    let mut child = ForegroundChildGuard::new(child, started_path);
    let proxy_guard = ProxyRuntimeGuard::new(repo.path(), &state_dir, proxy_port);
    wait_for_file_with_output(
        &before_listen,
        &mut child,
        Duration::from_secs(10),
        stdout_file.path(),
        stderr_file.path(),
    );

    let route_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_dir.join("routes.lock"))
        .expect("open route lock after startup preflight");
    route_lock.lock_exclusive().expect("hold route lock");
    fs::write(&listen_gate, b"listen\n").expect("release helper to bind");
    wait_for_file_with_output(
        &ready_path,
        &mut child,
        Duration::from_secs(10),
        stdout_file.path(),
        stderr_file.path(),
    );
    let (helper, _port, descendant) = read_helper_marker(&ready_path);

    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    wait_for_file_with_output(
        &first_term,
        &mut child,
        SIGNAL_EFFECT_TIMEOUT,
        stdout_file.path(),
        stderr_file.path(),
    );
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );

    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait for interrupted route publication")
        .unwrap_or_else(|| {
            FileExt::unlock(&route_lock).unwrap();
            panic!("route publication remained blocked after repeated signals")
        });
    assert_eq!(status.code(), Some(130));
    assert_verified_process_tree_exited(
        &[("helper", &helper), ("descendant", &descendant)],
        PROCESS_TREE_EXIT_TIMEOUT,
    );
    assert!(
        !fs::read_to_string(state_dir.join("routes.json"))
            .is_ok_and(|routes| routes.contains("signal-helper.signal-test.localhost")),
        "interrupted pre-publication wait wrote a route"
    );
    let output: Value = serde_json::from_slice(
        &fs::read(stdout_file.path()).expect("read route-publication JSON output"),
    )
    .expect("route-publication interruption emits one JSON result");
    assert_eq!(output["interrupted"], true);
    assert_eq!(output["exit_status"], 130);
    assert_eq!(output["termination_signal"], "SIGINT");
    assert_eq!(output["routes"], json!([]));
    FileExt::unlock(&route_lock).unwrap();
    child.disarm();

    let stop = proxy_guard.stop();
    assert!(stop.success(), "proxy stop exited with {stop}");
}

fn run_signal_case(case: SignalCase, mode: ForegroundMode, json_output: bool) {
    let repo = tempdir().expect("create test repo");
    let state_dir = repo.path().join("state");
    let ready_path = repo.path().join("helper-ready");
    let started_path = repo.path().join("helper-started");
    write_repo_fixture(repo.path());

    let stdout_file = NamedTempFile::new().expect("create stdout capture");
    let stderr_file = NamedTempFile::new().expect("create stderr capture");
    let stdout = stdout_file.reopen().expect("reopen stdout capture");
    let stderr = stderr_file.reopen().expect("reopen stderr capture");

    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    if json_output {
        command.arg("--json");
    }
    command.process_group(0);
    match mode {
        ForegroundMode::Dev => {
            command
                .args(["dev", "--no-proxy", "--state-dir"])
                .arg(&state_dir);
        }
        ForegroundMode::ProxyRun => {
            command
                .args(["proxy", "run", "signal-helper", "--no-proxy", "--state-dir"])
                .arg(&state_dir)
                .arg("--")
                .arg(std::env::current_exe().expect("resolve test binary"))
                .args(["--exact", "env_port_helper", "--nocapture"]);
        }
    }
    let child = command
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready_path)
        .env(STARTED_ENV, &started_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {}: {error}", mode.command_name()));
    let mut child = ForegroundChildGuard::new(child, started_path);

    wait_for_file_with_output(
        &ready_path,
        &mut child,
        Duration::from_secs(10),
        stdout_file.path(),
        stderr_file.path(),
    );
    let (helper_pid, _helper_port, descendant_pid) = read_helper_marker(&ready_path);

    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, case.signal) };
    if signal_result != 0 {
        let error = std::io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        terminate_verified_process(&helper_pid);
        terminate_verified_process(&descendant_pid);
        panic!("send {} to {}: {error}", case.label, mode.command_name());
    }

    let Some(status) = child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait for jig dev")
    else {
        let _ = child.kill();
        let _ = child.wait();
        terminate_verified_process(&helper_pid);
        terminate_verified_process(&descendant_pid);
        panic!("{} did not stop after {}", mode.command_name(), case.label);
    };

    let helper_stopped = wait_for_verified_exit(&helper_pid, Duration::from_secs(5));
    let descendant_stopped = wait_for_verified_exit(&descendant_pid, Duration::from_secs(5));
    if !helper_stopped {
        terminate_verified_process(&helper_pid);
    }
    if !descendant_stopped {
        terminate_verified_process(&descendant_pid);
    }
    assert!(
        helper_stopped,
        "{} left helper process {helper_pid} running",
        mode.command_name()
    );
    assert!(
        descendant_stopped,
        "{} left helper descendant {descendant_pid} running",
        mode.command_name()
    );
    child.disarm();

    let stdout = fs::read_to_string(stdout_file.path()).expect("read stdout capture");
    let stderr = fs::read_to_string(stderr_file.path()).expect("read stderr capture");
    assert_eq!(status.code(), Some(case.exit_status));
    assert!(!stdout.contains("Development session interrupted"));
    assert!(!stderr.contains("Development session interrupted"));
    assert!(!stdout.contains("Foreground process interrupted"));
    assert!(!stderr.contains("Foreground process interrupted"));

    if json_output {
        let output: Value = serde_json::from_str(&stdout).expect("stdout is one JSON result");
        assert_eq!(output["ok"], false);
        assert_eq!(output["interrupted"], true);
        assert_eq!(output["exit_status"], case.exit_status);
        assert_eq!(output["exit_signal"], case.signal);
        assert_eq!(output["termination_signal"], case.label);
        match mode {
            ForegroundMode::Dev => {
                assert!(output["first_exit"].is_null());
                assert_eq!(output["routes"], json!([]));
            }
            ForegroundMode::ProxyRun => {
                assert_eq!(output["app"], "signal-helper");
                assert_eq!(output["hostname"], "signal-helper.signal-test.localhost");
                assert!(output["port"].is_null());
            }
        }
    } else {
        let heading = match mode {
            ForegroundMode::Dev => "Dev",
            ForegroundMode::ProxyRun => "Proxy",
        };
        assert_eq!(
            stdout
                .matches(&format!("{heading}: stopped ({})", case.label))
                .count(),
            1
        );
        match mode {
            ForegroundMode::Dev => assert!(!stdout.contains("  Routes:")),
            ForegroundMode::ProxyRun => assert!(stdout.contains("  App: signal-helper")),
        }
    }
}

#[test]
fn a_second_termination_signal_forces_a_prompt_exit() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = tempdir().expect("create test repo");
    let state_dir = repo.path().join("state");
    let ready_path = repo.path().join("helper-ready");
    let started_path = repo.path().join("helper-started");
    let first_term_path = repo.path().join("first-term");
    let proxy_port = 0;
    let proxy_port_arg = proxy_port.to_string();
    write_repo_fixture_with_proxy(repo.path(), true);

    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command.process_group(0);
    let child = command
        .args(["dev", "--state-dir"])
        .arg(&state_dir)
        .args(["--http-port", &proxy_port_arg])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready_path)
        .env(STARTED_ENV, &started_path)
        .env(STUBBORN_DESCENDANT_ENV, "1")
        .env(FIRST_TERM_ENV, &first_term_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jig dev");
    let mut child = ForegroundChildGuard::new(child, started_path);
    let proxy_guard = ProxyRuntimeGuard::new(repo.path(), &state_dir, proxy_port);

    wait_for_file(&ready_path, &mut child, Duration::from_secs(10));
    let (helper_pid, _helper_port, descendant_pid) = read_helper_marker(&ready_path);
    wait_for_route(
        &state_dir.join("routes.json"),
        &mut child,
        Duration::from_secs(10),
    );
    let route_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_dir.join("routes.lock"))
        .expect("open route lock");
    route_lock.lock_exclusive().expect("hold route lock");
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    wait_for_file(&first_term_path, &mut child, SIGNAL_EFFECT_TIMEOUT);
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );

    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait for forced Jig exit")
        .unwrap_or_else(|| {
            FileExt::unlock(&route_lock).unwrap();
            let _ = child.kill();
            let _ = child.wait();
            terminate_verified_process(&helper_pid);
            terminate_verified_process(&descendant_pid);
            panic!("second SIGINT did not force a prompt Jig exit");
        });
    assert_eq!(status.code(), Some(130));
    FileExt::unlock(&route_lock).unwrap();
    let helper_stopped = wait_for_verified_exit(&helper_pid, Duration::from_secs(5));
    let descendant_stopped = wait_for_verified_exit(&descendant_pid, Duration::from_secs(5));
    if !helper_stopped {
        terminate_verified_process(&helper_pid);
    }
    if !descendant_stopped {
        terminate_verified_process(&descendant_pid);
    }
    assert!(
        helper_stopped,
        "forced cleanup left helper {helper_pid} running"
    );
    assert!(
        descendant_stopped,
        "forced cleanup left descendant {descendant_pid} running"
    );
    child.disarm();
    let stop = proxy_guard.stop();
    assert!(stop.success(), "proxy stop exited with {stop}");
}

#[test]
fn a_different_later_signal_forces_cleanup_without_replacing_the_first() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = tempdir().expect("create test repo");
    let state_dir = repo.path().join("state");
    let ready_path = repo.path().join("helper-ready");
    let started_path = repo.path().join("helper-started");
    let first_term_path = repo.path().join("first-term");
    let proxy_port = 0;
    let proxy_port_arg = proxy_port.to_string();
    write_repo_fixture_with_proxy(repo.path(), true);

    let child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .process_group(0)
        .args(["dev", "--state-dir"])
        .arg(&state_dir)
        .args(["--http-port", &proxy_port_arg])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready_path)
        .env(STARTED_ENV, &started_path)
        .env(STUBBORN_DESCENDANT_ENV, "1")
        .env(FIRST_TERM_ENV, &first_term_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jig dev");
    let mut child = ForegroundChildGuard::new(child, started_path);
    let proxy_guard = ProxyRuntimeGuard::new(repo.path(), &state_dir, proxy_port);

    wait_for_file(&ready_path, &mut child, Duration::from_secs(10));
    let (helper_pid, _helper_port, descendant_pid) = read_helper_marker(&ready_path);
    wait_for_route(
        &state_dir.join("routes.json"),
        &mut child,
        Duration::from_secs(10),
    );
    let route_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_dir.join("routes.lock"))
        .expect("open route lock");
    route_lock.lock_exclusive().expect("hold route lock");
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    wait_for_file(&first_term_path, &mut child, SIGNAL_EFFECT_TIMEOUT);
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );

    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait for mixed-signal Jig exit")
        .unwrap_or_else(|| {
            FileExt::unlock(&route_lock).unwrap();
            let _ = child.kill();
            let _ = child.wait();
            terminate_verified_process(&helper_pid);
            terminate_verified_process(&descendant_pid);
            panic!("mixed-signal Jig session did not exit");
        });
    assert_eq!(status.code(), Some(130));
    FileExt::unlock(&route_lock).unwrap();
    let helper_stopped = wait_for_verified_exit(&helper_pid, Duration::from_secs(5));
    let descendant_stopped = wait_for_verified_exit(&descendant_pid, Duration::from_secs(5));
    if !helper_stopped {
        terminate_verified_process(&helper_pid);
    }
    if !descendant_stopped {
        terminate_verified_process(&descendant_pid);
    }
    assert!(helper_stopped, "mixed-signal cleanup left helper running");
    assert!(
        descendant_stopped,
        "mixed-signal cleanup left descendant running"
    );
    child.disarm();
    let stop = proxy_guard.stop();
    assert!(stop.success(), "proxy stop exited with {stop}");
}

#[test]
fn a_second_signal_after_child_exit_accelerates_cleanup_without_replacing_the_result() {
    let _guard = SIGNAL_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = tempdir().expect("create test repo");
    let state_dir = repo.path().join("state");
    let ready_path = repo.path().join("helper-ready");
    let started_path = repo.path().join("helper-started");
    let exit_path = repo.path().join("helper-exit");
    let proxy_port = 0;
    let proxy_port_arg = proxy_port.to_string();
    write_repo_fixture_with_proxy(repo.path(), true);

    let stdout_file = NamedTempFile::new().expect("create stdout capture");
    let stderr_file = NamedTempFile::new().expect("create stderr capture");
    let child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .process_group(0)
        .args(["--json", "dev", "--state-dir"])
        .arg(&state_dir)
        .args(["--http-port", &proxy_port_arg])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .env(HELPER_ENV, "1")
        .env(READY_ENV, &ready_path)
        .env(STARTED_ENV, &started_path)
        .env(EXIT_ENV, &exit_path)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.reopen().unwrap()))
        .stderr(Stdio::from(stderr_file.reopen().unwrap()))
        .spawn()
        .expect("spawn proxied jig dev");
    let mut child = ForegroundChildGuard::new(child, started_path);
    let proxy_guard = ProxyRuntimeGuard::new(repo.path(), &state_dir, proxy_port);

    wait_for_file_with_output(
        &ready_path,
        &mut child,
        Duration::from_secs(10),
        stdout_file.path(),
        stderr_file.path(),
    );
    let (helper_pid, _helper_port, descendant_pid) = read_helper_marker(&ready_path);
    wait_for_route(
        &state_dir.join("routes.json"),
        &mut child,
        Duration::from_secs(10),
    );

    let route_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_dir.join("routes.lock"))
        .expect("open route lock");
    route_lock.lock_exclusive().expect("hold route lock");
    fs::write(&exit_path, b"exit\n").expect("request helper exit");
    assert!(
        wait_for_verified_exit(&helper_pid, Duration::from_secs(5)),
        "helper did not exit on request"
    );
    // Primary-outcome selection precedes descendant cleanup. Observing both
    // processes gone is therefore a deterministic finalization barrier.
    assert!(
        wait_for_verified_exit(&descendant_pid, Duration::from_secs(5)),
        "helper descendant did not exit during primary-outcome cleanup"
    );
    // Queue distinct standard signals so both remain observable even if the
    // child is descheduled. The held route lock is the semantic barrier: only
    // the later-signal forced path can finish before its normal lock deadline.
    for signal in [libc::SIGINT, libc::SIGTERM] {
        assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
    }

    let status = child
        .wait_timeout(Duration::from_secs(10))
        .expect("wait for result-preserving cleanup")
        .unwrap_or_else(|| {
            FileExt::unlock(&route_lock).unwrap();
            let _ = child.kill();
            let _ = child.wait();
            terminate_verified_process(&helper_pid);
            terminate_verified_process(&descendant_pid);
            panic!("second late signal did not break the contended route-lock wait");
        });
    FileExt::unlock(&route_lock).unwrap();

    let stdout = fs::read_to_string(stdout_file.path()).expect("read stdout capture");
    let stderr = fs::read_to_string(stderr_file.path()).expect("read stderr capture");
    let output: Value = serde_json::from_str(&stdout).expect("stdout is one JSON result");
    assert_eq!(status.code(), Some(1));
    assert_eq!(output["first_exit"]["exit_status"], 42);
    assert_ne!(output["interrupted"], true);
    assert!(stderr.contains("late-signal failure tail"));
    child.disarm();

    let list = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["--json", "proxy", "list", "--state-dir"])
        .arg(&state_dir)
        .args(["--http-port", &proxy_port_arg])
        .current_dir(repo.path())
        .env_remove("JIG_REPO_ROOT")
        .output()
        .expect("list live proxy routes");
    assert!(list.status.success());
    let list: Value = serde_json::from_slice(&list.stdout).expect("parse proxy list");
    assert_eq!(list["routes"], json!([]));

    let stop = proxy_guard.stop();
    assert!(stop.success(), "proxy stop exited with {stop}");
}

fn write_repo_fixture(root: &Path) {
    write_repo_fixture_with_proxy(root, false);
}

fn write_preflight_signal_fixture(root: &Path) {
    write_repo_fixture(root);
    let test_exe = serde_json::to_string(&std::env::current_exe().expect("resolve test binary"))
        .expect("quote test binary path");
    fs::create_dir_all(root.join("scripts")).expect("create scripts directory");
    fs::create_dir_all(root.join("web")).expect("create frontend directory");
    fs::write(
        root.join("scripts/check-webapps.sh"),
        r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" != "dependencies-ready" ] || [ "$#" -ne 2 ]; then
  exit 2
fi
sh -c 'trap "" INT HUP TERM; while :; do sleep 1; done' &
descendant="$!"
marker_tmp="$JIG_PREFLIGHT_SIGNAL_READY.tmp.$$"
printf '%s %s\n' "$$" "$descendant" > "$marker_tmp"
mv "$marker_tmp" "$JIG_PREFLIGHT_SIGNAL_READY"
while :; do sleep 1; done
"#,
    )
    .expect("write hanging dependency checker");
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/jig-dev-signal-template"
_commit = "test"
repo_name = "signal-test"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "true"

[[frontend_apps]]
name = "web"
dir = "web"
coverage_threshold = 80

[dev]

[[dev.apps]]
name = "web"
kind = "env-port"
dir = "web"
argv = [{test_exe}, "--exact", "env_port_helper", "--nocapture"]
host = "127.0.0.1"
proxy = false

[agent_tooling.codex]
marketplaces = []
"#,
            env!("CARGO_PKG_VERSION"),
        ),
    )
    .expect("write preflight Jig config");
}

fn wait_for_route(path: &Path, child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if fs::read_to_string(path)
            .is_ok_and(|contents| contents.contains("signal-helper.signal-test.localhost"))
        {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect route publication") {
            panic!("jig dev exited with {status} before publishing its route");
        }
        let now = Instant::now();
        if now >= deadline {
            panic!("timed out waiting for route in {}", path.display());
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

fn write_repo_fixture_with_proxy(root: &Path, proxy: bool) {
    let test_exe = serde_json::to_string(&std::env::current_exe().expect("resolve test binary"))
        .expect("quote test binary path");
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/jig-dev-signal-template"
_commit = "test"
repo_name = "signal-test"
default_branch = "main"
jig_version = "{}"
bootstrap_command = "true"

[dev]

[[dev.apps]]
name = "signal-helper"
kind = "env-port"
dir = "."
argv = [{test_exe}, "--exact", "env_port_helper", "--nocapture"]
host = "127.0.0.1"
proxy = {proxy}

[agent_tooling.codex]
marketplaces = []
"#,
            env!("CARGO_PKG_VERSION"),
        ),
    )
    .expect("write Jig config");
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
        .expect("serialize Jig contract"),
    )
    .expect("write Jig contract");
    fs::write(root.join(".mcp.json"), "{}\n").expect("write MCP config");
}

fn wait_for_file(path: &Path, child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect jig dev readiness") {
            panic!(
                "jig dev exited with {status} before publishing {}",
                path.display()
            );
        }
        let now = Instant::now();
        if now >= deadline {
            panic!("timed out waiting for {}", path.display());
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

fn wait_for_file_with_output(
    path: &Path,
    child: &mut Child,
    timeout: Duration,
    stdout: &Path,
    stderr: &Path,
) {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("inspect jig dev readiness") {
            panic!(
                "jig dev exited with {status} before publishing {}\nstdout:\n{}\nstderr:\n{}",
                path.display(),
                fs::read_to_string(stdout).unwrap_or_default(),
                fs::read_to_string(stderr).unwrap_or_default()
            );
        }
        let now = Instant::now();
        if now >= deadline {
            panic!(
                "timed out waiting for {}\nstdout:\n{}\nstderr:\n{}",
                path.display(),
                fs::read_to_string(stdout).unwrap_or_default(),
                fs::read_to_string(stderr).unwrap_or_default()
            );
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

#[derive(Clone, Debug)]
struct VerifiedProcessIdentity {
    pid: libc::pid_t,
    start_token: String,
}

#[derive(Debug)]
struct ProcessSnapshot {
    start_token: String,
    zombie: bool,
}

impl VerifiedProcessIdentity {
    fn capture(pid: libc::pid_t) -> Option<Self> {
        let snapshot = process_snapshot(pid)?;
        if snapshot.zombie {
            return None;
        }
        Some(Self {
            pid,
            start_token: snapshot.start_token,
        })
    }

    fn from_marker(pid: &str, start_token: &str) -> Option<Self> {
        let pid = pid.parse::<libc::pid_t>().ok()?;
        if pid <= 0 || start_token.is_empty() {
            return None;
        }
        Some(Self {
            pid,
            start_token: start_token.to_owned(),
        })
    }

    fn matching_snapshot(&self) -> Option<ProcessSnapshot> {
        let snapshot = process_snapshot(self.pid)?;
        (snapshot.start_token == self.start_token).then_some(snapshot)
    }

    fn is_live(&self) -> bool {
        self.matching_snapshot()
            .is_some_and(|snapshot| !snapshot.zombie)
    }

    fn owns_process_group(&self) -> bool {
        if self.matching_snapshot().is_none() {
            return false;
        }
        unsafe {
            // SAFETY: pid is positive and identity-verified immediately before
            // this non-mutating process-group query.
            libc::getpgid(self.pid) == self.pid
        }
    }
}

#[test]
fn marker_derived_cleanup_identity_fails_closed_on_token_mismatch() {
    let mut identity = VerifiedProcessIdentity::capture(
        std::process::id()
            .try_into()
            .expect("test process pid fits pid_t"),
    )
    .expect("capture test process identity");
    assert!(identity.is_live());

    identity.start_token.push_str(":mismatch");

    assert!(!identity.is_live());
    assert!(!identity.owns_process_group());
}

impl std::fmt::Display for VerifiedProcessIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.pid.fmt(formatter)
    }
}

#[cfg(target_os = "linux")]
fn process_snapshot(pid: libc::pid_t) -> Option<ProcessSnapshot> {
    if pid <= 0 {
        return None;
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields = fields.split_whitespace().collect::<Vec<_>>();
    Some(ProcessSnapshot {
        start_token: format!("linux:{}", fields.get(19)?),
        zombie: matches!(*fields.first()?, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "macos")]
fn process_snapshot(pid: libc::pid_t) -> Option<ProcessSnapshot> {
    if pid <= 0 {
        return None;
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes = unsafe {
        // SAFETY: info is writable proc_bsdinfo storage and proc_pidinfo writes
        // at most the supplied, checked buffer length.
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size.try_into().ok()?,
        )
    };
    if bytes < size.try_into().ok()? {
        return None;
    }
    let info = unsafe {
        // SAFETY: proc_pidinfo reported a complete proc_bsdinfo result.
        info.assume_init()
    };
    Some(ProcessSnapshot {
        start_token: format!("macos:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        zombie: info.pbi_status == libc::SZOMB,
    })
}

fn publish_started_marker(path: &Path, identities: &[VerifiedProcessIdentity]) {
    let marker = identities
        .iter()
        .map(|identity| format!("{} {}", identity.pid, identity.start_token))
        .collect::<Vec<_>>()
        .join(" ");
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, format!("{marker}\n")).expect("write temporary started marker");
    fs::rename(&temporary, path).expect("atomically publish started marker");
}

fn parse_started_marker(marker: &str) -> Option<Vec<VerifiedProcessIdentity>> {
    let mut fields = marker.split_whitespace();
    let mut identities = Vec::new();
    while let Some(pid) = fields.next() {
        let start_token = fields.next()?;
        identities.push(VerifiedProcessIdentity::from_marker(pid, start_token)?);
    }
    (!identities.is_empty()).then_some(identities)
}

struct ForegroundChildGuard {
    child: Child,
    process_group: libc::pid_t,
    started_path: PathBuf,
    armed: bool,
}

impl ForegroundChildGuard {
    fn new(child: Child, started_path: PathBuf) -> Self {
        Self {
            process_group: child.id().try_into().expect("child pid fits pid_t"),
            child,
            started_path,
            armed: true,
        }
    }

    const fn disarm(&mut self) {
        self.armed = false;
    }

    fn terminate_detached_helper_group(&self) {
        let Ok(marker) = fs::read_to_string(&self.started_path) else {
            return;
        };
        let Some(identities) = parse_started_marker(&marker) else {
            return;
        };
        if let Some(group_leader) = identities.first() {
            terminate_verified_group(group_leader);
        }
        for descendant in identities.iter().skip(1) {
            terminate_verified_process(descendant);
        }
    }
}

impl std::ops::Deref for ForegroundChildGuard {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ForegroundChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for ForegroundChildGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let running = self.child.try_wait().is_ok_and(|status| status.is_none());
        if running {
            // The unreaped Child value pins this direct PID while these signals
            // are sent; unlike marker-derived PIDs, it cannot have been reused.
            let _ = unsafe { libc::kill(self.process_group, libc::SIGINT) };
            let stopped = self
                .child
                .wait_timeout(Duration::from_secs(5))
                .ok()
                .flatten()
                .is_some();
            if !stopped {
                self.terminate_detached_helper_group();
                let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        self.terminate_detached_helper_group();
    }
}

struct ProxyRuntimeGuard {
    repo: PathBuf,
    state_dir: PathBuf,
    http_port: u16,
    armed: bool,
}

impl ProxyRuntimeGuard {
    fn new(repo: &Path, state_dir: &Path, http_port: u16) -> Self {
        Self {
            repo: repo.to_path_buf(),
            state_dir: state_dir.to_path_buf(),
            http_port,
            armed: true,
        }
    }

    fn stop(mut self) -> std::process::ExitStatus {
        self.armed = false;
        self.stop_inner().expect("stop background proxy")
    }

    fn stop_inner(&self) -> std::io::Result<std::process::ExitStatus> {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .args(["proxy", "stop", "--state-dir"])
            .arg(&self.state_dir)
            .args(["--http-port", &self.http_port.to_string()])
            .current_dir(&self.repo)
            .env_remove("JIG_REPO_ROOT")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    }
}

impl Drop for ProxyRuntimeGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.stop_inner();
        }
    }
}

fn read_helper_marker(path: &Path) -> (VerifiedProcessIdentity, u16, VerifiedProcessIdentity) {
    let marker = fs::read_to_string(path).expect("read helper marker");
    let mut fields = marker.split_whitespace();
    let helper = VerifiedProcessIdentity::from_marker(
        fields.next().expect("helper marker pid"),
        fields.next().expect("helper marker start token"),
    )
    .expect("helper marker has a valid identity");
    let port = fields
        .next()
        .expect("helper marker port")
        .parse()
        .expect("helper marker port is numeric");
    let descendant = VerifiedProcessIdentity::from_marker(
        fields.next().expect("helper marker descendant pid"),
        fields.next().expect("helper marker descendant start token"),
    )
    .expect("helper descendant marker has a valid identity");
    assert!(fields.next().is_none(), "helper marker has extra fields");
    assert!(helper.is_live(), "helper marker identity does not match");
    assert!(
        descendant.is_live(),
        "helper descendant marker identity does not match"
    );
    (helper, port, descendant)
}

fn wait_for_verified_exit(identity: &VerifiedProcessIdentity, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !identity.is_live() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    !identity.is_live()
}

fn assert_verified_process_tree_exited(
    processes: &[(&str, &VerifiedProcessIdentity)],
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        if processes.iter().all(|(_, identity)| !identity.is_live()) {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            let live = processes
                .iter()
                .filter(|(_, identity)| identity.is_live())
                .map(|(label, identity)| format!("{label} {}", identity.pid))
                .collect::<Vec<_>>()
                .join(", ");
            if live.is_empty() {
                return;
            }
            panic!("verified process tree remained live after {timeout:?}: {live}");
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

fn terminate_verified_process(identity: &VerifiedProcessIdentity) {
    if identity.is_live() {
        let _ = unsafe {
            // SAFETY: the positive pid and start token were reverified above.
            libc::kill(identity.pid, libc::SIGTERM)
        };
    }
    if wait_for_verified_exit(identity, Duration::from_millis(250)) {
        return;
    }
    if identity.is_live() {
        let _ = unsafe {
            // SAFETY: the positive pid and start token were reverified above.
            libc::kill(identity.pid, libc::SIGKILL)
        };
    }
    let _ = wait_for_verified_exit(identity, Duration::from_secs(2));
}

fn terminate_verified_group(identity: &VerifiedProcessIdentity) {
    if !identity.owns_process_group() {
        return;
    }
    let _ = unsafe {
        // SAFETY: getpgid and the process start token verified that this
        // negative pid still names the helper-owned process group.
        libc::kill(-identity.pid, libc::SIGTERM)
    };
    thread::sleep(Duration::from_millis(250));
    if identity.owns_process_group() {
        let _ = unsafe {
            // SAFETY: the group leader's pinned identity was reverified above.
            libc::kill(-identity.pid, libc::SIGKILL)
        };
    }
    let _ = wait_for_verified_exit(identity, Duration::from_secs(2));
}
