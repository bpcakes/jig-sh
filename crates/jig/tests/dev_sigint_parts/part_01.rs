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
use support::tempdir;
use tempfile::NamedTempFile;
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
