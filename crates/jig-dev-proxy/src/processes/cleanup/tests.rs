
use super::*;
use std::cell::RefCell;
use std::process::Command;

use super::super::termination_test_guard;
#[cfg(unix)]
static PRIOR_HANDLER_CALLED: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
const PRIOR_HANDLER_HELPER_ENV: &str = "JIG_TEST_PRIOR_SIGNAL_HANDLER";
#[cfg(unix)]
const NO_RESOURCES_HELPER_ENV: &str = "JIG_TEST_NO_RESOURCES_FORCE_EXIT";
#[cfg(unix)]
const INACTIVE_HANDLER_HELPER_ENV: &str = "JIG_TEST_INACTIVE_SIGNAL_HANDLER";
const PRE_ZERO_HANDLER_HELPER_ENV: &str = "JIG_TEST_PRE_ZERO_SIGNAL_HANDLER";
const RETIRED_HANDLER_HELPER_ENV: &str = "JIG_TEST_RETIRED_SIGNAL_HANDLER";
const STALE_CAPTURED_HANDLER_HELPER_ENV: &str = "JIG_TEST_STALE_CAPTURED_SIGNAL_HANDLER";
const STALE_LATER_SIGNAL_HELPER_ENV: &str = "JIG_TEST_STALE_LATER_SIGNAL_HANDLER";
#[cfg(unix)]
const RESTORING_HANDLER_HELPER_ENV: &str = "JIG_TEST_RESTORING_SIGNAL_HANDLER";

fn reset_for_test() {
    assert_eq!(
        TERMINATION_HANDLERS_IN_FLIGHT.load(Ordering::SeqCst),
        0,
        "test-only one-shot reset requires quiesced handlers"
    );
    OWNED_RESOURCE_STATE.store(RESOURCES_UNARMED, Ordering::SeqCst);
    TERMINATION_SESSION_POISONED.store(false, Ordering::SeqCst);
    TERMINATION_SESSION_CONSUMED.store(false, Ordering::SeqCst);
    NEXT_SESSION_GENERATION.store(1, Ordering::SeqCst);
    reset_session_state();
}

fn activate_test_session(phase: u8) -> usize {
    let generation = claim_next_session_generation().unwrap();
    SESSION_CLAIMED.store(true, Ordering::SeqCst);
    SESSION_PHASE.store(phase, Ordering::SeqCst);
    ACTIVE_SESSION_GENERATION.store(generation, Ordering::SeqCst);
    generation
}

#[test]
fn sibling_cleanup_paths_share_the_first_initialized_deadline() {
    let first_child = new_route_cleanup_deadline();
    let sibling_drop = first_child.clone();
    let first_deadline = shared_route_cleanup_deadline(&first_child);

    assert_eq!(
        *sibling_drop.get_or_init(|| panic!("sibling Drop initialized a second deadline")),
        first_deadline
    );
    assert_eq!(shared_route_cleanup_deadline(&sibling_drop), first_deadline);
    assert_eq!(first_child.get().copied(), Some(first_deadline));
}

#[test]
fn first_signal_is_sticky_and_any_later_signal_forces() {
    let _guard = termination_test_guard();
    reset_for_test();
    activate_test_session(SESSION_RUNNING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
    let first = if cfg!(unix) { libc::SIGINT } else { 2 };
    let different = if cfg!(unix) { libc::SIGTERM } else { 3 };

    record_termination(first);
    record_termination(different);
    assert_eq!(
        termination_requested(),
        Some(TerminationReason::from_signal(first))
    );
    assert!(force_cleanup_requested());
    reset_for_test();
}

#[test]
fn signal_recorded_before_primary_outcome_remains_graceful() {
    let _guard = termination_test_guard();
    reset_for_test();
    activate_test_session(SESSION_RUNNING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);

    record_termination(if cfg!(unix) { libc::SIGINT } else { 2 });
    select_primary_outcome();

    assert_eq!(termination_requested(), None);
    assert!(!force_cleanup_requested());
    reset_for_test();
}

#[test]
fn first_signal_in_interruption_phase_remains_graceful_until_second_signal() {
    let _guard = termination_test_guard();
    reset_for_test();
    activate_test_session(SESSION_INTERRUPTING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
    let first = if cfg!(unix) { libc::SIGINT } else { 2 };

    record_termination(first);

    assert_eq!(
        termination_requested(),
        Some(TerminationReason::from_signal(first))
    );
    assert!(!force_cleanup_requested());

    record_termination(if cfg!(unix) { libc::SIGTERM } else { 2 });

    assert_eq!(TERMINATION_SIGNAL.load(Ordering::SeqCst), first);
    assert!(force_cleanup_requested());
    reset_for_test();
}

#[test]
fn first_finalizing_signal_remains_graceful_and_second_signal_forces() {
    let _guard = termination_test_guard();
    reset_for_test();
    activate_test_session(SESSION_FINALIZING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);

    record_termination(if cfg!(unix) { libc::SIGTERM } else { 2 });

    assert_eq!(termination_requested(), None);
    assert!(!force_cleanup_requested());

    record_termination(if cfg!(unix) { libc::SIGINT } else { 2 });

    assert!(force_cleanup_requested());
    reset_for_test();
}

#[test]
fn second_signal_forces_after_primary_outcome_selection() {
    let _guard = termination_test_guard();
    reset_for_test();
    activate_test_session(SESSION_RUNNING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
    let first = if cfg!(unix) { libc::SIGINT } else { 2 };

    record_termination(first);
    select_primary_outcome();
    record_termination(if cfg!(unix) { libc::SIGTERM } else { 2 });

    assert_eq!(TERMINATION_SIGNAL.load(Ordering::SeqCst), first);
    assert!(force_cleanup_requested());
    reset_for_test();
}

#[test]
fn inactive_handler_does_not_force_armed_resources() {
    let _guard = termination_test_guard();
    reset_for_test();
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);

    record_termination(if cfg!(unix) { libc::SIGTERM } else { 2 });

    assert!(!force_cleanup_requested());
    assert_eq!(OWNED_RESOURCE_STATE.load(Ordering::SeqCst), RESOURCES_ARMED);
    reset_for_test();
}

#[test]
fn committed_exit_prevents_a_late_resource_arm() {
    let _guard = termination_test_guard();
    reset_for_test();
    SESSION_CLAIMED.store(true, Ordering::SeqCst);
    OWNED_RESOURCE_STATE.store(EXIT_WITHOUT_RESOURCES_CLAIMED, Ordering::SeqCst);

    assert!(arm_owned_resources().is_err());
    reset_session_state();
    assert_eq!(
        OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
        EXIT_WITHOUT_RESOURCES_CLAIMED
    );
    reset_for_test();
}

#[test]
fn transactional_install_rolls_back_partial_success_and_can_retry() {
    let installed = RefCell::new(Vec::new());
    let restored = RefCell::new(Vec::new());
    let signals = [(1, "one"), (2, "two"), (3, "three")];

    let first = install_transactionally(
        &signals,
        |signal| {
            if signal == 2 {
                return Err("injected".into());
            }
            installed.borrow_mut().push(signal);
            Ok(signal * 10)
        },
        |signal, _| {
            restored.borrow_mut().push(signal);
            Ok(())
        },
    );
    let error = first.unwrap_err();
    assert!(error.to_string().contains("failed to register two"));
    assert!(!error.handlers_may_remain);
    assert_eq!(&*restored.borrow(), &[1]);

    let retry = install_transactionally(&signals, |signal| Ok(signal * 10), |_, _| Ok(())).unwrap();
    assert_eq!(retry.len(), 3);
}

#[cfg(unix)]
#[test]
fn failed_restore_installs_default_disposition_and_continues_restoring() {
    let restored = RefCell::new(Vec::new());
    let defaulted = RefCell::new(Vec::new());

    let restoration = restore_with_default_fallback(
        &[(libc::SIGINT, 10), (libc::SIGHUP, 20), (libc::SIGTERM, 30)],
        |signal, _| {
            restored.borrow_mut().push(signal);
            if signal == libc::SIGHUP {
                Err("injected restore failure".to_string())
            } else {
                Ok(())
            }
        },
        |signal| {
            defaulted.borrow_mut().push(signal);
            Ok(())
        },
    );

    assert_eq!(
        &*restored.borrow(),
        &[libc::SIGTERM, libc::SIGHUP, libc::SIGINT]
    );
    assert_eq!(&*defaulted.borrow(), &[libc::SIGHUP]);
    assert!(!restoration.handlers_may_remain);
    assert_eq!(restoration.warnings.len(), 1);
    assert!(restoration.warnings[0].contains("injected restore failure"));
    assert!(restoration.warnings[0].contains("installed the default disposition"));
}

#[cfg(unix)]
#[test]
fn failed_restore_and_default_fallback_are_marked_unsafe() {
    let restoration = restore_with_default_fallback(
        &[(libc::SIGTERM, 30)],
        |_, _| Err("injected restore failure".to_string()),
        |_| Err("injected default failure".to_string()),
    );

    assert!(restoration.handlers_may_remain);
    assert_eq!(restoration.warnings.len(), 1);
    assert!(restoration.warnings[0].contains("default-disposition fallback also failed"));
}

#[test]
fn stale_generation_cannot_mutate_the_active_session() {
    let _guard = termination_test_guard();
    reset_for_test();
    let stale_generation = activate_test_session(SESSION_RUNNING);
    assert_eq!(enter_termination_handler(), stale_generation);
    let active_generation = claim_next_session_generation().unwrap();
    ACTIVE_SESSION_GENERATION.store(active_generation, Ordering::SeqCst);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);

    record_termination_for_generation(if cfg!(unix) { libc::SIGINT } else { 2 }, stale_generation);
    leave_termination_handler();

    assert_eq!(TERMINATION_SIGNAL.load(Ordering::SeqCst), 0);
    assert!(!FORCE_CLEANUP_REQUESTED.load(Ordering::SeqCst));
    assert_eq!(OWNED_RESOURCE_STATE.load(Ordering::SeqCst), RESOURCES_ARMED);
    reset_for_test();
}

#[test]
fn stale_later_signal_generation_cannot_force_armed_resources() {
    let _guard = termination_test_guard();
    reset_for_test();
    let stale_generation = activate_test_session(SESSION_RUNNING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
    let first_signal = if cfg!(unix) { libc::SIGINT } else { 2 };
    TERMINATION_SIGNAL.store(first_signal, Ordering::SeqCst);
    let active_generation = claim_next_session_generation().unwrap();
    ACTIVE_SESSION_GENERATION.store(active_generation, Ordering::SeqCst);

    force_terminate_without_owned_resources(first_signal, Some(stale_generation));

    assert_eq!(TERMINATION_SIGNAL.load(Ordering::SeqCst), first_signal);
    assert!(!FORCE_CLEANUP_REQUESTED.load(Ordering::SeqCst));
    assert_eq!(OWNED_RESOURCE_STATE.load(Ordering::SeqCst), RESOURCES_ARMED);
    reset_for_test();
}

#[cfg(unix)]
#[test]
fn one_shot_session_rejects_reuse_during_and_after_a_paused_handler() {
    use std::sync::mpsc;

    let _guard = termination_test_guard();
    reset_for_test();
    let first = start_termination_cleanup_session().unwrap();
    let first_generation = first.generation;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let handler = thread::spawn(move || {
        let generation = enter_termination_handler();
        entered_tx.send(generation).unwrap();
        release_rx.recv().unwrap();
        leave_termination_handler();
    });
    assert_eq!(entered_rx.recv().unwrap(), first_generation);

    let dropper = thread::spawn(move || drop(first));
    let deadline = Instant::now() + Duration::from_secs(2);
    while ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst) != 0 {
        assert!(Instant::now() < deadline, "first generation did not retire");
        thread::yield_now();
    }
    assert!(
        start_termination_cleanup_session().is_err(),
        "the one-shot session was reused while its handler was in flight"
    );

    release_tx.send(()).unwrap();
    handler.join().unwrap();
    dropper.join().unwrap();
    assert!(!TERMINATION_SESSION_POISONED.load(Ordering::SeqCst));
    let error = start_termination_cleanup_session()
        .err()
        .expect("one-shot session reuse must fail");
    assert!(error.to_string().contains("start a new process"));
    reset_for_test();
}

#[test]
fn handler_quiescence_wait_is_bounded() {
    let _guard = termination_test_guard();
    reset_for_test();
    TERMINATION_HANDLERS_IN_FLIGHT.store(1, Ordering::SeqCst);

    assert!(!wait_for_termination_handlers(Duration::ZERO));

    TERMINATION_HANDLERS_IN_FLIGHT.store(0, Ordering::SeqCst);
    reset_for_test();
}

#[test]
fn retirement_disarms_resources_before_publishing_generation_zero() {
    let _guard = termination_test_guard();
    reset_for_test();
    let generation = activate_test_session(SESSION_FINALIZING);
    OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);

    assert!(retire_session_generation(generation));

    assert_eq!(ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst), 0);
    assert_eq!(
        OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
        RESOURCES_UNARMED
    );

    reset_for_test();
    let generation = activate_test_session(SESSION_FINALIZING);
    OWNED_RESOURCE_STATE.store(EXIT_WITHOUT_RESOURCES_CLAIMED, Ordering::SeqCst);
    assert!(retire_session_generation(generation));
    assert_eq!(
        OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
        EXIT_WITHOUT_RESOURCES_CLAIMED
    );
    reset_for_test();
}

#[test]
fn first_finalizing_signal_after_unarm_exits_before_generation_zero() {
    if std::env::var_os(PRE_ZERO_HANDLER_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        let generation = activate_test_session(SESSION_FINALIZING);
        OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);

        let _ = retire_session_generation_with(generation, || {
            assert_eq!(
                OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
                RESOURCES_UNARMED
            );
            assert_eq!(ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst), generation);
            record_termination_for_generation(
                if cfg!(unix) { libc::SIGTERM } else { 2 },
                generation,
            );
            panic!(
                "a first finalizing signal between resource and generation retirement was swallowed"
            );
        });
        panic!("resource retirement returned after a conventional-exit claim");
    }

    let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "processes::cleanup::tests::first_finalizing_signal_after_unarm_exits_before_generation_zero",
                "--nocapture",
            ])
            .env(PRE_ZERO_HANDLER_HELPER_ENV, "1")
            .status()
            .unwrap();
    assert_eq!(status.code(), Some(if cfg!(unix) { 143 } else { 130 }));
}

#[cfg(unix)]
#[test]
fn signal_during_unix_handler_restoration_observes_retired_session() {
    if std::env::var_os(RESTORING_HANDLER_HELPER_ENV).is_some() {
        use std::cell::Cell;

        let _guard = termination_test_guard();
        reset_for_test();
        let session = start_termination_cleanup_session().unwrap();
        arm_owned_resources().unwrap();
        select_primary_outcome();
        let pending_signal = session.previous_handlers[0].0;
        let restored_one = Cell::new(false);

        let _ = retire_session_and_restore_unix_handlers(
            session.generation,
            &session.previous_handlers,
            |signal, previous| {
                restore_unix_handler(signal, previous)?;
                if !restored_one.replace(true) {
                    assert_ne!(signal, pending_signal);
                    assert_eq!(ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst), 0);
                    assert_eq!(
                        OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
                        RESOURCES_UNARMED
                    );
                    assert_eq!(unsafe { libc::raise(pending_signal) }, 0);
                    panic!("an in-flight Jig handler swallowed a restoration-window signal");
                }
                Ok(())
            },
            install_default_unix_handler,
        );
        panic!("handler restoration returned after a conventional-exit claim");
    }

    let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "processes::cleanup::tests::signal_during_unix_handler_restoration_observes_retired_session",
                "--nocapture",
            ])
            .env(RESTORING_HANDLER_HELPER_ENV, "1")
            .status()
            .unwrap();
    assert_eq!(status.code(), Some(128 + libc::SIGINT));
}

#[test]
fn signal_after_generation_retirement_exits_before_state_reset() {
    if std::env::var_os(RETIRED_HANDLER_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        let generation = activate_test_session(SESSION_FINALIZING);
        OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
        assert!(retire_session_generation(generation));
        assert_eq!(SESSION_PHASE.load(Ordering::SeqCst), SESSION_FINALIZING);
        assert!(SESSION_CLAIMED.load(Ordering::SeqCst));

        record_termination_for_generation(if cfg!(unix) { libc::SIGTERM } else { 2 }, 0);
        panic!("a signal entering after generation retirement was swallowed");
    }

    let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "processes::cleanup::tests::signal_after_generation_retirement_exits_before_state_reset",
                "--nocapture",
            ])
            .env(RETIRED_HANDLER_HELPER_ENV, "1")
            .status()
            .unwrap();
    assert_eq!(status.code(), Some(if cfg!(unix) { 143 } else { 130 }));
}

#[test]
fn signal_with_captured_generation_exits_after_retirement() {
    if std::env::var_os(STALE_CAPTURED_HANDLER_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        let generation = activate_test_session(SESSION_FINALIZING);
        OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
        let captured_generation = enter_termination_handler();
        assert_eq!(captured_generation, generation);
        assert!(retire_session_generation(generation));
        assert_eq!(ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst), 0);
        assert_eq!(
            OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
            RESOURCES_UNARMED
        );

        record_termination_for_generation(
            if cfg!(unix) { libc::SIGTERM } else { 2 },
            captured_generation,
        );
        panic!("a signal with a pre-retirement generation was swallowed");
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "processes::cleanup::tests::signal_with_captured_generation_exits_after_retirement",
            "--nocapture",
        ])
        .env(STALE_CAPTURED_HANDLER_HELPER_ENV, "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(if cfg!(unix) { 143 } else { 130 }));
}

#[test]
fn later_signal_after_retirement_exits_with_sticky_first_status() {
    if std::env::var_os(STALE_LATER_SIGNAL_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        let generation = activate_test_session(SESSION_FINALIZING);
        OWNED_RESOURCE_STATE.store(RESOURCES_ARMED, Ordering::SeqCst);
        let first_signal = if cfg!(unix) { libc::SIGINT } else { 2 };
        let later_signal = if cfg!(unix) { libc::SIGTERM } else { 3 };
        TERMINATION_SIGNAL.store(first_signal, Ordering::SeqCst);
        let captured_generation = enter_termination_handler();
        let sticky_signal = TERMINATION_SIGNAL
            .compare_exchange(0, later_signal, Ordering::SeqCst, Ordering::SeqCst)
            .expect_err("the later signal must observe the sticky first signal");
        assert_eq!(sticky_signal, first_signal);
        assert!(retire_session_generation(generation));
        assert_eq!(ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst), 0);
        assert_eq!(
            OWNED_RESOURCE_STATE.load(Ordering::SeqCst),
            RESOURCES_UNARMED
        );

        force_terminate_without_owned_resources(sticky_signal, Some(captured_generation));
        panic!("a later signal after retirement did not preserve the first status");
    }

    let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "processes::cleanup::tests::later_signal_after_retirement_exits_with_sticky_first_status",
                "--nocapture",
            ])
            .env(STALE_LATER_SIGNAL_HELPER_ENV, "1")
            .status()
            .unwrap();
    assert_eq!(status.code(), Some(130));
}

#[test]
fn failed_start_cleanup_does_not_reset_the_one_shot_claim() {
    let _guard = termination_test_guard();
    reset_for_test();
    claim_one_shot_termination_session().unwrap();

    // A failed start may clear ordinary session state after rolling back
    // partial handler installation, but it must never permit registration
    // again in this process.
    reset_session_state();

    let error = claim_one_shot_termination_session().unwrap_err();
    assert!(error.to_string().contains("start a new process"));
    reset_for_test();
}

#[cfg(unix)]
#[test]
fn unquiesced_handler_poisons_later_sessions() {
    let _guard = termination_test_guard();
    reset_for_test();
    let session = start_termination_cleanup_session().unwrap();
    TERMINATION_HANDLERS_IN_FLIGHT.store(1, Ordering::SeqCst);

    drop(session);

    assert!(TERMINATION_SESSION_POISONED.load(Ordering::SeqCst));
    assert!(start_termination_cleanup_session().is_err());
    TERMINATION_HANDLERS_IN_FLIGHT.store(0, Ordering::SeqCst);
    reset_for_test();
}

#[cfg(unix)]
#[test]
fn completed_session_restores_prior_dispositions_and_rejects_reuse() {
    let _guard = termination_test_guard();
    reset_for_test();
    let before = current_unix_handler(libc::SIGTERM);
    {
        let _first = start_termination_cleanup_session().unwrap();
        assert_ne!(current_unix_handler(libc::SIGTERM), before);
    }
    assert_eq!(current_unix_handler(libc::SIGTERM), before);
    let error = start_termination_cleanup_session()
        .err()
        .expect("one-shot session reuse must fail");
    assert!(error.to_string().contains("start a new process"));
    assert_eq!(current_unix_handler(libc::SIGTERM), before);
    reset_for_test();
}

#[cfg(unix)]
fn current_unix_handler(signal: i32) -> usize {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::sigaction(signal, std::ptr::null(), &mut action) },
        0
    );
    action.sa_sigaction
}

#[cfg(unix)]
extern "C" fn prior_signal_handler(_: libc::c_int) {
    PRIOR_HANDLER_CALLED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
#[test]
fn scoped_session_restores_a_real_prior_signal_handler() {
    if std::env::var_os(PRIOR_HANDLER_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        PRIOR_HANDLER_CALLED.store(false, Ordering::SeqCst);
        let previous = unsafe {
            libc::signal(
                libc::SIGTERM,
                prior_signal_handler as *const () as libc::sighandler_t,
            )
        };
        assert_ne!(previous, libc::SIG_ERR);
        {
            let _first = start_termination_cleanup_session().unwrap();
        }
        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        assert!(PRIOR_HANDLER_CALLED.load(Ordering::SeqCst));
        return;
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "processes::cleanup::tests::scoped_session_restores_a_real_prior_signal_handler",
            "--nocapture",
        ])
        .env(PRIOR_HANDLER_HELPER_ENV, "1")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "prior-handler helper exited with {status}"
    );
}

#[cfg(unix)]
#[test]
fn repeated_signal_before_owned_resources_exits_promptly() {
    if std::env::var_os(NO_RESOURCES_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        activate_test_session(SESSION_RUNNING);
        record_termination(libc::SIGINT);
        record_termination(libc::SIGINT);
        panic!("a repeated signal without owned resources did not exit");
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "processes::cleanup::tests::repeated_signal_before_owned_resources_exits_promptly",
            "--nocapture",
        ])
        .env(NO_RESOURCES_HELPER_ENV, "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(130));
}

#[cfg(unix)]
#[test]
fn inactive_installed_handler_does_not_swallow_a_later_signal() {
    if std::env::var_os(INACTIVE_HANDLER_HELPER_ENV).is_some() {
        let _guard = termination_test_guard();
        reset_for_test();
        let session = start_termination_cleanup_session().unwrap();
        // Model a handler that remains installed after a failed restore.
        // The subprocess exits below, so intentionally leaking the guard
        // cannot affect another test or inherited disposition.
        std::mem::forget(session);
        reset_session_state();
        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        panic!("an inactive installed Jig handler swallowed SIGTERM");
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "processes::cleanup::tests::inactive_installed_handler_does_not_swallow_a_later_signal",
            "--nocapture",
        ])
        .env(INACTIVE_HANDLER_HELPER_ENV, "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(143));
}

#[cfg(unix)]
#[test]
fn termination_reason_maps_unix_signals_to_shell_statuses() {
    for (signal, label, status) in [
        (libc::SIGHUP, "SIGHUP", 129),
        (libc::SIGINT, "SIGINT", 130),
        (libc::SIGTERM, "SIGTERM", 143),
    ] {
        let reason = TerminationReason::from_signal(signal);
        assert_eq!(reason.signal(), signal);
        assert_eq!(reason.label(), label);
        assert_eq!(reason.exit_status(), status);
    }
}

#[cfg(not(unix))]
#[test]
fn termination_reason_maps_ctrl_c_to_shell_status() {
    let reason = TerminationReason::from_signal(2);
    assert_eq!(reason.signal(), 2);
    assert_eq!(reason.label(), "Ctrl-C");
    assert_eq!(reason.exit_status(), 130);
}
