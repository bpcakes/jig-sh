
#[cfg(unix)]
#[test]
fn sqlx_driver_probe_invokes_shim_safely_and_times_out() {
    let _env = lock_env();
    let _secret = EnvVarGuard::set("JIG_DOCTOR_TEST_SECRET", "must-not-be-inherited");
    let _database_url = EnvVarGuard::set("DATABASE_URL", "postgres://must-not-be-inherited");
    let temp = tempdir().unwrap();
    let supported = temp.path().join("cargo-sqlx-supported");
    write_test_executable(
        &supported,
        "#!/bin/sh\n[ -z \"${JIG_DOCTOR_TEST_SECRET+x}\" ] || exit 8\n[ -z \"${DATABASE_URL+x}\" ] || exit 8\n[ \"$HOME\" = \"$USERPROFILE\" ] || exit 8\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 8\n[ \"$HOME\" = \"$TMP\" ] || exit 8\n[ \"$HOME\" = \"$TEMP\" ] || exit 8\n[ \"$LC_ALL\" = C ] || exit 8\n[ \"$NO_COLOR\" = 1 ] || exit 8\n[ \"$1\" = sqlx ] || exit 9\nprintf '%s\\n' 'error: unknown value \"jig-doctor-invalid\" for ssl_mode'\nexit 1\n",
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &supported,
            SqlxProbeStyle::CargoSubcommand,
            SqlxDriver::Postgres,
            Duration::from_secs(1)
        ),
        SqlxDriverProbe::Compatible
    );

    let direct = temp.path().join("sqlx-supported");
    write_test_executable(
        &direct,
        "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nexit 0\n",
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &direct,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(1),
        ),
        SqlxDriverProbe::Compatible
    );

    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let unrelated = tempdir().unwrap();
    let tools = fs::canonicalize(tools.path()).unwrap();
    let path_limited = tools.join("sqlx-path-limited");
    write_test_executable(
        &path_limited,
        &format!(
            "#!/bin/sh\n[ \"$PATH\" = '{}' ] || exit 8\nexit 0\n",
            tools.display()
        ),
    );
    let broad_path = env::join_paths([tools.as_path(), unrelated.path()]).unwrap();
    assert_eq!(
        probe_sqlx_driver_with_timeout_and_environment(
            &path_limited,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(1),
            repo.path(),
            &DoctorEnvironment {
                search_path: Some(broad_path),
                ..DoctorEnvironment::default()
            },
        ),
        SqlxDriverProbe::Compatible
    );

    let hanging = temp.path().join("cargo-sqlx-hanging");
    write_test_executable(&hanging, "#!/bin/sh\nwhile :; do :; done\n");
    assert!(matches!(
        probe_sqlx_driver_with_timeout(
            &hanging,
            SqlxProbeStyle::CargoSubcommand,
            SqlxDriver::Sqlite,
            Duration::from_millis(20)
        ),
        SqlxDriverProbe::Indeterminate(reason) if reason.contains("timed out")
    ));

    let noisy = temp.path().join("cargo-sqlx-noisy");
    write_test_executable(
        &noisy,
        "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nexit 0\n",
    );
    let noisy_probe = probe_sqlx_driver_with_timeout(
        &noisy,
        SqlxProbeStyle::CargoSubcommand,
        SqlxDriver::Sqlite,
        Duration::from_secs(2),
    );
    assert!(
        matches!(
            &noisy_probe,
            SqlxDriverProbe::Indeterminate(reason) if reason.contains("capture limit")
        ),
        "unexpected noisy probe result: {noisy_probe:?}"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sqlx_driver_probe_reaps_descendants_on_completion_and_timeout() {
    let temp = tempdir().unwrap();

    let completed_marker = temp.path().join("completed-descendant");
    let completed = temp.path().join("sqlx-completed");
    write_test_executable(
        &completed,
        &owned_test_descendant_script(&completed_marker, "exit 0"),
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &completed,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(2),
        ),
        SqlxDriverProbe::Compatible
    );
    let completed_descendant = read_test_process_identity(&completed_marker);

    let timeout_marker = temp.path().join("timeout-descendant");
    let hanging = temp.path().join("sqlx-timeout-tree");
    write_test_executable(
        &hanging,
        &owned_test_descendant_script(&timeout_marker, "while :; do :; done"),
    );
    let timeout_probe = probe_sqlx_driver_with_timeout(
        &hanging,
        SqlxProbeStyle::Direct,
        SqlxDriver::Sqlite,
        Duration::from_millis(300),
    );
    assert!(
        matches!(
            &timeout_probe,
            SqlxDriverProbe::Indeterminate(reason) if reason == "the driver probe timed out"
        ),
        "unexpected timeout probe result: {timeout_probe:?}"
    );
    let timeout_descendant = read_test_process_identity(&timeout_marker);

    for descendant in [completed_descendant, timeout_descendant] {
        assert_test_process_stopped(&descendant);
    }
}

#[cfg(unix)]
// The scoped signal session must remain active until the explicit finish path
// restores handlers and re-delivers any recorded signal.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_driver_probe_sigint_helper() {
    let Some(executable) = std::env::var_os("JIG_SQLX_PROBE_SIGINT_HELPER") else {
        return;
    };
    let signal_session = DoctorSignalSession::start().unwrap();
    let cancelled = || signal_session.cancelled();
    let result = probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        Path::new(&executable),
        SqlxProbeStyle::Direct,
        SqlxDriver::Sqlite,
        Duration::from_secs(30),
        Path::new("/"),
        &DoctorEnvironment::default(),
        Some(&cancelled),
    );
    let _ = finish_doctor_signal_session(signal_session);
    panic!("SIGINT was not re-delivered after probe cleanup: {result:?}");
}

#[cfg(unix)]
#[test]
fn sqlx_probe_signal_finish_fails_closed_when_restoration_fails() {
    let signals = DoctorSignals {
        first: Some(libc::SIGINT),
        mask: doctor_signal_bit(libc::SIGINT),
    };
    assert_eq!(
        doctor_signal_finish_action(signals, true),
        DoctorSignalFinishAction::Redeliver(signals)
    );
    assert_eq!(
        doctor_signal_finish_action(signals, false),
        DoctorSignalFinishAction::Exit(128 + libc::SIGINT)
    );
    assert_eq!(
        doctor_signal_finish_action(DoctorSignals::default(), false),
        DoctorSignalFinishAction::Continue
    );
}

#[cfg(unix)]
// Session ownership deliberately spans signal delivery through restoration.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration() {
    const HELPER: &str = "JIG_SQLX_PROBE_MIXED_SIGNAL_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert!(status.success(), "mixed-signal helper exited with {status}");
        return;
    }

    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.store(0, Ordering::SeqCst);
    for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
        // SAFETY: zero initializes the sigaction storage before its fields
        // and mask are populated below.
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
        action.sa_flags = 0;
        // SAFETY: the mask is writable storage owned by this test.
        assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
        // SAFETY: action is initialized and the helper subprocess owns its
        // process-wide dispositions for the remainder of this test.
        assert_eq!(
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) },
            0
        );
    }

    let session = DoctorSignalSession::start().unwrap();
    for signal in [
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGINT,
        libc::SIGHUP,
        libc::SIGTERM,
    ] {
        // SAFETY: each supported signal is handled synchronously by the
        // active scoped recorder in this isolated helper subprocess.
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        0,
        "a signal reached its prior disposition before session retirement",
    );

    finish_doctor_signal_session(session).unwrap();
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        3,
    );
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.load(Ordering::SeqCst),
        1 | (2 << 2) | (3 << 4),
    );
}

#[cfg(unix)]
// Session ownership deliberately spans signal delivery through restoration.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_does_not_swallow_later_default_termination() {
    use std::os::unix::process::ExitStatusExt;

    const HELPER: &str = "JIG_SQLX_PROBE_LATER_DEFAULT_SIGNAL_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_does_not_swallow_later_default_termination",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert_eq!(
            status.signal(),
            Some(libc::SIGTERM),
            "later-default-signal helper returned unexpected status {status}",
        );
        return;
    }

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut ignored = unsafe { std::mem::zeroed::<libc::sigaction>() };
    ignored.sa_sigaction = libc::SIG_IGN;
    ignored.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this helper process.
    assert_eq!(unsafe { libc::sigemptyset(&mut ignored.sa_mask) }, 0);
    // SAFETY: ignored is fully initialized and this isolated helper owns
    // its process-wide SIGINT disposition.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, &ignored, std::ptr::null_mut()) },
        0,
    );
    install_default_doctor_signal_handler(libc::SIGTERM).unwrap();

    let session = DoctorSignalSession::start().unwrap();
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the active scoped session has installed a handler for
        // each supported signal in this isolated helper process.
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }
    finish_doctor_signal_session(session).unwrap();
    panic!("the later default SIGTERM disposition was swallowed");
}

#[cfg(unix)]
#[test]
fn sqlx_probe_signal_session_drop_restores_previous_handlers() {
    const HELPER: &str = "JIG_SQLX_PROBE_DROP_RESTORE_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::sqlx_probe_signal_session_drop_restores_previous_handlers",
                "--nocapture",
            ])
            .env(HELPER, "1")
            .status()
            .unwrap();
        assert!(status.success(), "drop-restore helper exited with {status}");
        return;
    }

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut ignored = unsafe { std::mem::zeroed::<libc::sigaction>() };
    ignored.sa_sigaction = libc::SIG_IGN;
    ignored.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this helper process.
    assert_eq!(unsafe { libc::sigemptyset(&mut ignored.sa_mask) }, 0);
    // SAFETY: ignored is fully initialized and this isolated helper owns its
    // process-wide SIGINT disposition.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, &ignored, std::ptr::null_mut()) },
        0,
    );

    {
        let _session = DoctorSignalSession::start().unwrap();
    }

    // SAFETY: current points to writable storage and a null action requests
    // the process's current disposition without changing it.
    let mut current = unsafe { std::mem::zeroed::<libc::sigaction>() };
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current) },
        0,
    );
    assert_eq!(current.sa_sigaction, libc::SIG_IGN);
}

#[cfg(unix)]
// These guards serialize generations and are consumed only by explicit finish.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation() {
    use std::sync::mpsc;

    const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BARRIER_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert!(
            status.success(),
            "reusable barrier helper exited with {status}"
        );
        return;
    }

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this isolated helper.
    assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
    // SAFETY: this subprocess owns its SIGTERM disposition for the test.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) },
        0
    );

    let (ready_tx, ready_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let owner = std::thread::spawn(move || {
        let session = DoctorSignalSession::start().unwrap();
        ready_tx.send(session.generation()).unwrap();
        finish_rx.recv().unwrap();
        finished_tx
            .send(finish_doctor_signal_session(session).is_ok())
            .unwrap();
    });
    let first_generation = ready_rx.recv().unwrap();

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(true, Ordering::SeqCst);
    let handler = std::thread::spawn(|| record_doctor_signal(libc::SIGTERM));
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED.load(Ordering::SeqCst) {
        assert!(Instant::now() < pause_deadline, "handler did not pause");
        std::thread::yield_now();
    }
    finish_tx.send(()).unwrap();

    let (next_tx, next_rx) = mpsc::channel();
    let next = std::thread::spawn(move || {
        let session = DoctorSignalSession::start().unwrap();
        let generation = session.generation();
        let redelivered = SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst);
        let finished = finish_doctor_signal_session(session).is_ok();
        next_tx.send((generation, redelivered, finished)).unwrap();
    });
    assert!(
        next_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "a second signal-session attempt bypassed the active owner"
    );

    SQLX_PROBE_TEST_RELEASE_HANDLER.store(true, Ordering::SeqCst);
    handler.join().unwrap();
    assert!(finished_rx.recv().unwrap());
    owner.join().unwrap();

    let (next_generation, redelivered, next_finished) =
        next_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    next.join().unwrap();
    assert!(next_generation > first_generation);
    assert_eq!(redelivered, 1, "the next owner entered before redelivery");
    assert!(next_finished);

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER.store(false, Ordering::SeqCst);
}

#[cfg(unix)]
// These guards pin the generation until delayed callbacks are accounted for.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation() {
    const HELPER: &str = "JIG_SQLX_PROBE_DELAYED_ENTRY_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert!(
            status.success(),
            "delayed-entry helper exited with {status}"
        );
        return;
    }

    SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this isolated helper.
    assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
    // SAFETY: this subprocess owns its SIGTERM disposition for the test.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) },
        0
    );

    let first = DoctorSignalSession::start().unwrap();
    let first_generation = first.generation();
    let delayed = std::thread::spawn(|| record_doctor_signal(libc::SIGTERM));
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < pause_deadline,
            "handler did not pause before claiming a generation"
        );
        std::thread::yield_now();
    }

    finish_doctor_signal_session(first).unwrap();
    let second = DoctorSignalSession::start().unwrap();
    let second_generation = second.generation();
    assert!(second_generation > first_generation);

    SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
    delayed.join().unwrap();
    assert!(
        second.cancelled(),
        "delayed callback did not join the active generation"
    );
    SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    finish_doctor_signal_session(second).unwrap();
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        1
    );

    let third = DoctorSignalSession::start().unwrap();
    assert!(third.generation() > second_generation);
    finish_doctor_signal_session(third).unwrap();
}

#[cfg(unix)]
// The guard must outlive the paused handler until fail-closed retirement.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal() {
    use std::sync::mpsc;

    const HELPER: &str = "JIG_SQLX_PROBE_RECORDED_TIMEOUT_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert_eq!(
            status.code(),
            Some(128 + libc::SIGTERM),
            "recorded-timeout helper returned unexpected status {status}"
        );
        return;
    }

    SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD.store(true, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT.store(true, Ordering::SeqCst);
    SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.store(false, Ordering::SeqCst);

    let session = DoctorSignalSession::start().unwrap();
    let (handler_done_tx, handler_done_rx) = mpsc::channel();
    let handler = std::thread::spawn(move || {
        record_doctor_signal(libc::SIGTERM);
        handler_done_tx.send(()).unwrap();
    });
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < pause_deadline,
            "handler did not pause after recording"
        );
        std::thread::yield_now();
    }

    let coordinator = std::thread::spawn(move || {
        let timeout_deadline = Instant::now() + Duration::from_secs(2);
        while !SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < timeout_deadline,
                "signal retirement did not reach its quiescence timeout"
            );
            std::thread::yield_now();
        }
        SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.store(true, Ordering::SeqCst);
        handler_done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("recorded handler did not complete before poison publication");
        SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.store(true, Ordering::SeqCst);
    });

    let result = finish_doctor_signal_session(session);
    coordinator.join().unwrap();
    handler.join().unwrap();
    panic!("recorded signal was not claimed by fail-closed retirement: {result:?}");
}

#[cfg(unix)]
#[test]
fn inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal() {
    const HELPER: &str = "JIG_SQLX_PROBE_INACTIVE_HANDLER_HELPER";
    if std::env::var_os(HELPER).is_some() {
        DOCTOR_ACTIVE_GENERATION.store(0, Ordering::SeqCst);
        DOCTOR_SIGNAL_GENERATION.store(0, Ordering::SeqCst);
        record_doctor_signal(libc::SIGTERM);
        panic!("an inactive SQLx probe handler swallowed SIGTERM");
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal",
            "--nocapture",
        ])
        .env(HELPER, "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(128 + libc::SIGTERM));
}

#[cfg(unix)]
#[test]
fn poisoned_sqlx_probe_session_lock_blocks_future_sessions() {
    const HELPER: &str = "JIG_SQLX_PROBE_POISONED_LOCK_HELPER";
    if std::env::var_os(HELPER).is_some() {
        let poisoner = std::thread::spawn(|| {
            let _guard = DOCTOR_SIGNAL_SESSION.lock().unwrap();
            panic!("poison the signal-session mutex");
        });
        assert!(poisoner.join().is_err());
        let error = DoctorSignalSession::start()
            .err()
            .expect("poisoned mutex must reject a new signal session");
        assert!(error.to_string().contains("mutex is poisoned"));
        return;
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::poisoned_sqlx_probe_session_lock_blocks_future_sessions",
            "--nocapture",
        ])
        .env(HELPER, "1")
        .status()
        .unwrap();
    assert!(status.success(), "poison helper exited with {status}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sqlx_driver_probe_sigint_reaps_descendants_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    let descendant_marker = temp.path().join("probe-descendant");
    let probe = temp.path().join("sqlx-sigint-tree");
    write_test_executable(
        &probe,
        &owned_test_descendant_script(&descendant_marker, "while :; do :; done"),
    );
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::sqlx_driver_probe_sigint_helper",
            "--nocapture",
        ])
        .env("JIG_SQLX_PROBE_SIGINT_HELPER", &probe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: the test owns this live helper PID and sends a standard
    // termination signal solely to that subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("SIGINT helper did not terminate after probe cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}

#[test]
fn proxy_list_command_preserves_the_portable_launcher_plan() {
    let temp = tempdir().unwrap();
    let (launcher, command) = proxy_list_command(temp.path()).unwrap();
    let args = command
        .get_args()
        .map(OsStr::to_os_string)
        .collect::<Vec<_>>();

    assert!(launcher.is_absolute());
    assert_eq!(command.get_current_dir(), Some(temp.path()));
    for key in crate::shell::BASH_CONTROL_ENVIRONMENT_KEYS {
        assert!(
            command
                .get_envs()
                .any(|(candidate, value)| candidate == OsStr::new(key) && value.is_none()),
            "{key} was not removed from the launcher-backed proxy diagnostic"
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(command.get_program(), OsStr::new("bash"));
        assert_eq!(
            args,
            vec![
                launcher.into_os_string(),
                OsString::from("proxy"),
                OsString::from("list"),
                OsString::from("--json"),
            ]
        );
    }
    #[cfg(not(windows))]
    {
        assert_eq!(command.get_program(), launcher.as_os_str());
        assert_eq!(
            args,
            vec![
                OsString::from("proxy"),
                OsString::from("list"),
                OsString::from("--json"),
            ]
        );
    }
}

#[cfg(windows)]
#[test]
fn proxy_list_command_converts_verbatim_roots_for_bash_and_its_working_directory() {
    let (launcher, command) = proxy_list_command(Path::new(r"\\?\C:\repo")).unwrap();
    let args = command
        .get_args()
        .map(OsStr::to_os_string)
        .collect::<Vec<_>>();

    assert_eq!(launcher, PathBuf::from(r"C:\repo\scripts\jig"));
    assert_eq!(command.get_program(), OsStr::new("bash"));
    assert_eq!(command.get_current_dir(), Some(Path::new(r"C:\repo")));
    assert_eq!(args[0], launcher.as_os_str());

    let (unc_launcher, unc_command) =
        proxy_list_command(Path::new(r"\\?\UNC\server\share\repo")).unwrap();
    assert_eq!(
        unc_launcher,
        PathBuf::from(r"\\server\share\repo\scripts\jig")
    );
    assert_eq!(
        unc_command.get_current_dir(),
        Some(Path::new(r"\\server\share\repo"))
    );
    assert_eq!(
        unc_command.get_args().next(),
        Some(unc_launcher.as_os_str())
    );
}
