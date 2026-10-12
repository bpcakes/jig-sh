#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::sync::atomic::Ordering;
#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
use crate::signal_supervision::SignalSession;
#[cfg(unix)]
use crate::signal_supervision::session::finish_signal_session;
#[cfg(unix)]
use crate::signal_supervision::session::{
    ACTIVE_SIGNAL_GENERATION, RecordedSignals, SIGNAL_GENERATION, SIGNAL_SESSION,
    SQLX_PROBE_TEST_HANDLER_PAUSED, SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD,
    SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM, SQLX_PROBE_TEST_PAUSE_HANDLER,
    SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD, SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM,
    SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT, SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT,
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT, SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER,
    SQLX_PROBE_TEST_RELEASE_HANDLER, SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD,
    SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM, SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT,
    SignalFinishAction, install_default_signal_handler, record_signal,
    record_sqlx_probe_test_redelivery, signal_bit, signal_finish_action,
};

#[cfg(unix)]
#[test]
fn sqlx_probe_signal_finish_fails_closed_when_restoration_fails() {
    let signals = RecordedSignals {
        first: Some(libc::SIGINT),
        mask: signal_bit(libc::SIGINT),
    };
    assert_eq!(
        signal_finish_action(signals, true),
        SignalFinishAction::Redeliver(signals)
    );
    assert_eq!(
        signal_finish_action(signals, false),
        SignalFinishAction::Exit(128 + libc::SIGINT)
    );
    assert_eq!(
        signal_finish_action(RecordedSignals::default(), false),
        SignalFinishAction::Continue
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
                    "doctor::tests::probe_signal_session::sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration",
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

    let session = SignalSession::start().unwrap();
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

    finish_signal_session(session).unwrap();
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
                    "doctor::tests::probe_signal_session::sqlx_probe_signal_session_does_not_swallow_later_default_termination",
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
    install_default_signal_handler(libc::SIGTERM).unwrap();

    let session = SignalSession::start().unwrap();
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the active scoped session has installed a handler for
        // each supported signal in this isolated helper process.
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }
    finish_signal_session(session).unwrap();
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
                "doctor::tests::probe_signal_session::sqlx_probe_signal_session_drop_restores_previous_handlers",
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
        let _session = SignalSession::start().unwrap();
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
                    "doctor::tests::probe_signal_session::sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation",
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
        let session = SignalSession::start().unwrap();
        ready_tx.send(session.generation()).unwrap();
        finish_rx.recv().unwrap();
        finished_tx
            .send(finish_signal_session(session).is_ok())
            .unwrap();
    });
    let first_generation = ready_rx.recv().unwrap();

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(true, Ordering::SeqCst);
    let handler = std::thread::spawn(|| record_signal(libc::SIGTERM));
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED.load(Ordering::SeqCst) {
        assert!(Instant::now() < pause_deadline, "handler did not pause");
        std::thread::yield_now();
    }
    finish_tx.send(()).unwrap();

    let (next_tx, next_rx) = mpsc::channel();
    let next = std::thread::spawn(move || {
        let session = SignalSession::start().unwrap();
        let generation = session.generation();
        let redelivered = SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst);
        let finished = finish_signal_session(session).is_ok();
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
                    "doctor::tests::probe_signal_session::sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation",
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

    let first = SignalSession::start().unwrap();
    let first_generation = first.generation();
    let delayed = std::thread::spawn(|| record_signal(libc::SIGTERM));
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < pause_deadline,
            "handler did not pause before claiming a generation"
        );
        std::thread::yield_now();
    }

    finish_signal_session(first).unwrap();
    let second = SignalSession::start().unwrap();
    let second_generation = second.generation();
    assert!(second_generation > first_generation);

    SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
    delayed.join().unwrap();
    assert!(
        second.cancelled(),
        "delayed callback did not join the active generation"
    );
    SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    finish_signal_session(second).unwrap();
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        1
    );

    let third = SignalSession::start().unwrap();
    assert!(third.generation() > second_generation);
    finish_signal_session(third).unwrap();
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
                    "doctor::tests::probe_signal_session::sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal",
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

    let session = SignalSession::start().unwrap();
    let (handler_done_tx, handler_done_rx) = mpsc::channel();
    let handler = std::thread::spawn(move || {
        record_signal(libc::SIGTERM);
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

    let result = finish_signal_session(session);
    coordinator.join().unwrap();
    handler.join().unwrap();
    panic!("recorded signal was not claimed by fail-closed retirement: {result:?}");
}
#[cfg(unix)]
#[test]
fn inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal() {
    const HELPER: &str = "JIG_SQLX_PROBE_INACTIVE_HANDLER_HELPER";
    if std::env::var_os(HELPER).is_some() {
        ACTIVE_SIGNAL_GENERATION.store(0, Ordering::SeqCst);
        SIGNAL_GENERATION.store(0, Ordering::SeqCst);
        record_signal(libc::SIGTERM);
        panic!("an inactive SQLx probe handler swallowed SIGTERM");
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::probe_signal_session::inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal",
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
            let _guard = SIGNAL_SESSION.lock().unwrap();
            panic!("poison the signal-session mutex");
        });
        assert!(poisoner.join().is_err());
        let error = SignalSession::start()
            .err()
            .expect("poisoned mutex must reject a new signal session");
        assert!(error.to_string().contains("mutex is poisoned"));
        return;
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::probe_signal_session::poisoned_sqlx_probe_session_lock_blocks_future_sessions",
            "--nocapture",
        ])
        .env(HELPER, "1")
        .status()
        .unwrap();
    assert!(status.success(), "poison helper exited with {status}");
}
