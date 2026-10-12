use super::*;

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
