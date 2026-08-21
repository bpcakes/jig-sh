use std::collections::VecDeque;
#[cfg(windows)]
use std::fs;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_env::lock_env;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{
    TestProcessIdentity, assert_test_process_stopped, publish_test_process_identity,
    read_test_process_identity, terminate_and_confirm_test_process,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const OWNED_PROCESS_DESCENDANT_MARKER_ENV: &str = "JIG_OWNED_PROCESS_DESCENDANT_MARKER";

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn shell_quote_test_path(path: &Path) -> String {
    let path = path
        .to_str()
        .expect("test helper paths must be representable in shell fixtures");
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_descendant_helper() {
    let Some(marker) = std::env::var_os(OWNED_PROCESS_DESCENDANT_MARKER_ENV) else {
        return;
    };
    let identity = TestProcessIdentity::capture_current().expect("capture test helper identity");
    publish_test_process_identity(Path::new(&marker), &identity);
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn owned_process_max_timeout_helper() {
    if std::env::var_os("JIG_OWNED_PROCESS_MAX_TIMEOUT_HELPER").is_some() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn owned_process_output_capture_is_bounded_and_lossy_safe() {
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "i=0; while [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done; printf '\\377diagnostic\\n'",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output =
        run_owned_process_tree_with_output(&mut command, Duration::from_secs(2), || false).unwrap();
    let stdout = output.stdout.unwrap();
    let stderr = output.stderr.unwrap();

    assert!(output.status.success());
    assert!(stdout.complete);
    assert!(!stdout.truncated);
    assert!(stdout.to_string_lossy().contains("diagnostic"));
    assert!(stderr.complete);
    assert!(stderr.truncated);
    assert_eq!(stderr.bytes.len(), OWNED_PROCESS_OUTPUT_LIMIT);
}

#[cfg(unix)]
#[test]
fn fatal_output_overflow_terminates_the_owned_tree_immediately() {
    struct NoopObserver;
    impl OwnedProcessObserver for NoopObserver {}

    let temp = tempdir().unwrap();
    let marker = temp.path().join("overflow-descendant-survived");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "(sleep 1; printf survived > \"$1\") & head -c 8192 /dev/zero; wait",
            "sh",
        ])
        .arg(&marker)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let started = Instant::now();
    let error = match run_owned_process_tree_with_output_policy_and_observer(
        &mut command,
        Duration::from_secs(5),
        ProcessOutputLimits {
            stdout: 1024,
            stderr: 1024,
        },
        ProcessOutputOverflowPolicy::Error,
        &mut NoopObserver,
    ) {
        Ok(_) => panic!("fatal output overflow unexpectedly succeeded"),
        Err(error) => error,
    };

    assert!(
        matches!(
            error,
            OwnedProcessTreeError::OutputLimitExceeded(OwnedProcessOutputStream::Stdout)
        ),
        "unexpected overflow result: {error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "fatal overflow waited for the configured timeout"
    );
    std::thread::sleep(Duration::from_millis(1_250));
    assert!(
        !marker.exists(),
        "fatal overflow left an owned descendant running"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn owned_process_timeout_overflow_remains_unbounded() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "process::tests::owned_process_max_timeout_helper",
            "--nocapture",
        ])
        .env("JIG_OWNED_PROCESS_MAX_TIMEOUT_HELPER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = run_owned_process_tree_with_output(&mut command, Duration::MAX, || false)
        .expect("an overflowing timeout remains an unbounded wait");

    assert!(output.status.success());
    assert!(output.stdout.unwrap().complete);
    assert!(output.stderr.unwrap().complete);
}

#[cfg(windows)]
fn wait_for_test_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        path.exists(),
        "test marker {} was not published",
        path.display()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_output_escape_helper() {
    let _env = lock_env();
    let Some(mode) = std::env::var_os("JIG_OWNED_OUTPUT_ESCAPE_HELPER") else {
        return;
    };
    let marker = PathBuf::from(
        std::env::var_os("JIG_OWNED_OUTPUT_ESCAPE_MARKER").expect("escape marker path"),
    );
    if mode == "escaped" {
        assert_ne!(
            unsafe { libc::setsid() },
            -1,
            "escape the owned process group"
        );
        let identity =
            TestProcessIdentity::capture_current().expect("capture escaped helper identity");
        publish_test_process_identity(&marker, &identity);
        std::thread::sleep(Duration::from_secs(30));
        return;
    }

    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::tests::owned_process_output_escape_helper",
            "--nocapture",
        ])
        .env("JIG_OWNED_OUTPUT_ESCAPE_HELPER", "escaped")
        .env("JIG_OWNED_OUTPUT_ESCAPE_MARKER", &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    std::mem::forget(child);
    let _ = read_test_process_identity(&marker);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_output_capture_does_not_wait_for_an_escaped_pipe_owner() {
    let temp = tempdir().unwrap();
    let started = Instant::now();
    for iteration in 0..4 {
        let marker = temp
            .path()
            .join(format!("escaped-output-owner-{iteration}"));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::tests::owned_process_output_escape_helper",
                "--nocapture",
            ])
            .env("JIG_OWNED_OUTPUT_ESCAPE_HELPER", "spawn")
            .env("JIG_OWNED_OUTPUT_ESCAPE_MARKER", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output =
            run_owned_process_tree_with_output(&mut command, Duration::from_secs(5), || false)
                .unwrap();
        let escaped = read_test_process_identity(&marker);
        terminate_and_confirm_test_process(&escaped);

        assert!(output.status.success());
        assert!(
            !output.stdout.unwrap().complete || !output.stderr.unwrap().complete,
            "an escaped pipe owner must make capture explicitly incomplete"
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "repeated silent pipe escapes must remain bounded without accumulating readers"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_group_confirmation_resignals_before_retrying_an_inconclusive_proof() {
    #[derive(Default)]
    struct InjectedCleanup {
        signals: usize,
        proofs: VecDeque<bool>,
    }

    let now = Instant::now();
    let sleeps = std::cell::Cell::new(0_usize);
    let mut cleanup = InjectedCleanup {
        proofs: VecDeque::from([false, true]),
        ..InjectedCleanup::default()
    };
    confirm_process_group_quiescent_with(
        &mut cleanup,
        73,
        now + Duration::from_secs(1),
        1,
        "injected confirmation",
        |cleanup, _, _| {
            cleanup.signals += 1;
            Ok(if cleanup.signals == 1 {
                // Models ESRCH or eligible macOS EPERM: neither is proof
                // that a late group member cannot become visible.
                ProcessGroupSignalResult::Inconclusive
            } else {
                ProcessGroupSignalResult::Delivered
            })
        },
        |cleanup, _, _| {
            Ok(cleanup
                .proofs
                .pop_front()
                .expect("injected proof sequence was exhausted"))
        },
        || now,
        |_| sleeps.set(sleeps.get() + 1),
    )
    .unwrap();

    assert_eq!(cleanup.signals, 2);
    assert!(cleanup.proofs.is_empty());
    assert_eq!(sleeps.get(), 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_group_confirmation_signals_between_two_empty_linux_proofs() {
    #[derive(Default)]
    struct InjectedCleanup {
        signals: usize,
        proofs: VecDeque<bool>,
    }

    let now = Instant::now();
    let mut cleanup = InjectedCleanup {
        // A live scan resets the proof count; two following empty scans
        // are each preceded by a group SIGKILL attempt.
        proofs: VecDeque::from([false, true, true]),
        ..InjectedCleanup::default()
    };
    confirm_process_group_quiescent_with(
        &mut cleanup,
        73,
        now + Duration::from_secs(1),
        2,
        "injected Linux confirmation",
        |cleanup, _, _| {
            cleanup.signals += 1;
            Ok(ProcessGroupSignalResult::Delivered)
        },
        |cleanup, _, _| {
            Ok(cleanup
                .proofs
                .pop_front()
                .expect("injected proof sequence was exhausted"))
        },
        || now,
        |_| {},
    )
    .unwrap();

    assert_eq!(cleanup.signals, 3);
    assert!(cleanup.proofs.is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_group_confirmation_never_proves_or_resignals_after_a_terminal_error() {
    #[derive(Default)]
    struct InjectedCleanup {
        signals: usize,
        proofs: usize,
    }

    let now = Instant::now();
    let mut cleanup = InjectedCleanup::default();
    let error = confirm_process_group_quiescent_with(
        &mut cleanup,
        73,
        now + Duration::from_secs(1),
        1,
        "injected terminal error",
        |cleanup, _, _| {
            cleanup.signals += 1;
            Err(std::io::Error::from_raw_os_error(libc::ECHILD))
        },
        |cleanup, _, _| {
            cleanup.proofs += 1;
            Ok(true)
        },
        || now,
        |_| {},
    )
    .unwrap_err();

    assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
    assert_eq!(cleanup.signals, 1);
    assert_eq!(cleanup.proofs, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_group_signal_observes_the_pinned_leader_before_any_signal_attempt() {
    #[derive(Default)]
    struct InjectedSignal {
        observations: usize,
        signals: usize,
    }

    let mut injected = InjectedSignal::default();
    let error = observe_owned_process_before_group_signal_with(
        &mut injected,
        |injected| {
            injected.observations += 1;
            Err(std::io::Error::from_raw_os_error(libc::ECHILD))
        },
        |injected, _| {
            injected.signals += 1;
            Ok(ProcessGroupSignalResult::Delivered)
        },
    )
    .unwrap_err();

    assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
    assert_eq!(injected.observations, 1);
    assert_eq!(injected.signals, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_group_confirmation_checks_the_absolute_deadline_after_every_signal() {
    #[derive(Default)]
    struct InjectedCleanup {
        signals: usize,
        proofs: usize,
    }

    let start = Instant::now();
    let deadline = start + Duration::from_millis(20);
    let observed_now = std::cell::Cell::new(start);
    let sleeps = std::cell::Cell::new(0_usize);
    let mut cleanup = InjectedCleanup::default();
    let error = confirm_process_group_quiescent_with(
        &mut cleanup,
        73,
        deadline,
        1,
        "injected deadline",
        |cleanup, _, _| {
            cleanup.signals += 1;
            observed_now.set(deadline);
            Ok(ProcessGroupSignalResult::Delivered)
        },
        |cleanup, _, _| {
            cleanup.proofs += 1;
            Ok(true)
        },
        || observed_now.get(),
        |_| sleeps.set(sleeps.get() + 1),
    )
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(cleanup.signals, 1);
    assert_eq!(cleanup.proofs, 0);
    assert_eq!(sleeps.get(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_owned_process_group_eperm_is_inconclusive_only_for_an_exited_pinned_leader() {
    let eligible = resolve_macos_process_group_signal_eperm(
        std::io::Error::from_raw_os_error(libc::EPERM),
        Ok(OwnedProcessObservation::Exited),
    )
    .unwrap();
    assert_eq!(eligible, ProcessGroupSignalResult::Inconclusive);

    let running_error = resolve_macos_process_group_signal_eperm(
        std::io::Error::from_raw_os_error(libc::EPERM),
        Ok(OwnedProcessObservation::Running),
    )
    .unwrap_err();
    assert_eq!(running_error.raw_os_error(), Some(libc::EPERM));

    let observation_error = resolve_macos_process_group_signal_eperm(
        std::io::Error::from_raw_os_error(libc::EPERM),
        Err(std::io::Error::from_raw_os_error(libc::ECHILD)),
    )
    .unwrap_err();
    assert_eq!(
        observation_error.kind(),
        std::io::Error::from_raw_os_error(libc::ECHILD).kind()
    );
    assert!(observation_error.to_string().contains("after EPERM"));
}

#[test]
fn linux_owned_process_scan_distinguishes_live_zombie_and_unverifiable_members() {
    let stats = std::collections::HashMap::from([
        (73, "73 (leader) Z 1 73 0 0 0".to_string()),
        (74, "74 (worker) S 73 73 0 0 0".to_string()),
        (75, "75 (other) S 1 75 0 0 0".to_string()),
    ]);
    assert!(
        linux_process_group_has_live_members_with(
            73,
            [73, 74, 75],
            |pid| Ok(stats[&pid].clone()),
            |_| unreachable!(),
            || true,
        )
        .unwrap()
    );
    assert!(
        !linux_process_group_has_live_members_with(
            73,
            [73, 75],
            |pid| Ok(stats[&pid].clone()),
            |_| unreachable!(),
            || true,
        )
        .unwrap()
    );
    assert!(
        !linux_process_group_has_live_members_with(
            73,
            [76],
            |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            |_| Ok(None),
            || true,
        )
        .unwrap()
    );
    assert!(
        linux_process_group_has_live_members_with(
            73,
            [76],
            |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            |_| Ok(Some(73)),
            || true,
        )
        .is_err()
    );
    assert!(
        linux_process_group_has_live_members_with(
            73,
            [73],
            |pid| Ok(stats[&pid].clone()),
            |_| unreachable!(),
            || false,
        )
        .is_err()
    );
}

#[test]
fn linux_owned_process_scan_accepts_non_utf8_command_names() {
    let mut zombie_stat = b"73 (codex-".to_vec();
    zombie_stat.push(0xff);
    zombie_stat.extend_from_slice(b") Z 1 73 0 0 0");

    assert!(
        !linux_process_group_has_live_members_bytes_with(
            73,
            [73],
            |_| Ok(zombie_stat.clone()),
            |_| unreachable!(),
            || true,
        )
        .unwrap()
    );
}

#[test]
fn linux_process_stat_parser_rejects_a_mismatched_or_missing_pid_prefix() {
    assert!(parse_linux_process_stat(0, b"0 (worker) Z 1 0 0 0 0").is_err());
    assert!(parse_linux_process_stat(73, b"74 (worker) Z 1 73 0 0 0").is_err());
    assert!(parse_linux_process_stat(73, b") Z 1 73 0 0 0").is_err());
}

#[test]
fn linux_owned_process_enumeration_fails_closed_when_advancing_exhausts_budget() {
    let within_budget = std::cell::Cell::new(true);
    let entries = std::iter::from_fn(|| {
        within_budget.set(false);
        None::<std::io::Result<i32>>
    });

    let error =
        collect_linux_process_ids_with(73, entries, Some, || within_budget.get()).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("while scanning Linux processes"));
}

#[test]
fn linux_owned_process_scan_fails_closed_when_fallback_lookup_exhausts_budget() {
    let within_budget = std::cell::Cell::new(true);

    let error = linux_process_group_has_live_members_with(
        73,
        [74],
        |_| Err(std::io::Error::other("injected stat failure")),
        |_| {
            within_budget.set(false);
            Ok(None)
        },
        || within_budget.get(),
    )
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("while scanning Linux processes"));
}

#[test]
fn linux_owned_process_scan_checks_budget_before_live_and_empty_results() {
    let live_checks = std::cell::Cell::new(0usize);
    let live_error = linux_process_group_has_live_members_with(
        73,
        [74],
        |_| Ok("74 (worker) S 1 73 0 0 0".to_string()),
        |_| unreachable!(),
        || {
            let check = live_checks.get() + 1;
            live_checks.set(check);
            check <= 3
        },
    )
    .unwrap_err();
    assert_eq!(live_checks.get(), 4);
    assert_eq!(live_error.kind(), std::io::ErrorKind::TimedOut);

    let empty_checks = std::cell::Cell::new(0usize);
    let empty_error = linux_process_group_has_live_members_with(
        73,
        std::iter::empty(),
        |_| unreachable!(),
        |_| unreachable!(),
        || {
            let check = empty_checks.get() + 1;
            empty_checks.set(check);
            check == 1
        },
    )
    .unwrap_err();
    assert_eq!(empty_checks.get(), 2);
    assert_eq!(empty_error.kind(), std::io::ErrorKind::TimedOut);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_cleanup_uses_one_absolute_deadline() {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "while :; do :; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();
    let first = process.cleanup_deadline();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(process.cleanup_deadline(), first);
    process.cleanup_deadline = None;
    process.terminate_and_reap().unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_tree_failure_uses_one_direct_fallback_and_retains_the_primary_error() {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "while :; do :; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();

    let first = process
        .terminate_and_reap_with(|_, _| {
            Err(std::io::Error::other("injected owned process-tree failure"))
        })
        .unwrap_err()
        .to_string();
    assert!(
        first.contains("injected owned process-tree failure"),
        "{first}"
    );
    assert!(process.cleanup_finalized);
    assert!(!process.cleanup_complete);
    assert!(process.reaped_status.is_some());
    assert!(process.process_group.is_none());
    assert_eq!(process.terminate_and_reap().unwrap_err().to_string(), first);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_owned_process_cleanup_stress_never_releases_a_live_group_member() {
    for iteration in 0..100 {
        let mut command = Command::new("/bin/bash");
        command
            .args(["-c", "(/bin/sleep 30) & printf '%s' \"$!\"; exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let output =
            run_owned_process_tree_with_output(&mut command, Duration::from_secs(2), || false)
                .unwrap_or_else(|error| panic!("iteration {iteration}: {error}"));
        assert!(output.status.success(), "iteration {iteration}");
        let descendant = output
            .stdout
            .unwrap()
            .to_string_lossy()
            .parse::<libc::pid_t>()
            .unwrap();
        // SAFETY: a zero signal only probes the PID written by this owned
        // helper. Cleanup must not return while that group member exists.
        let result = unsafe { libc::kill(descendant, 0) };
        assert_eq!(result, -1, "iteration {iteration}, PID {descendant}");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH),
            "iteration {iteration}, PID {descendant}"
        );
    }
}

#[cfg(unix)]
#[test]
fn owned_process_cleanup_is_bounded_and_requires_pinned_identity() {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "while :; do :; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();
    let process_group = process.process_group.take().unwrap();

    let started = std::time::Instant::now();
    assert!(matches!(
        finish_owned_process_wait(
            &mut process,
            Err(std::io::Error::other("injected observation failure")),
        ),
        Err(OwnedProcessTreeError::Cleanup)
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(process.reaped_status.is_none());
    assert!(!process.cleanup_complete);
    assert!(process.cleanup_finalized);
    let retained_error = process.cleanup_error.as_ref().unwrap().message.clone();
    assert!(retained_error.contains("direct-child fallback also failed"));
    assert!(retained_error.contains("direct-child reap also failed"));

    // Restoring an identity after finalization must not cause a second
    // cleanup attempt or overwrite the first failure.
    process.process_group = Some(process_group);
    assert_eq!(
        process.terminate_and_reap().unwrap_err().to_string(),
        retained_error
    );
    // SAFETY: the test deliberately hid this still-pinned group from the
    // supervisor and retains its exact original identifier for teardown.
    assert_eq!(
        unsafe { libc::kill(-process_group.id.as_raw(), libc::SIGKILL) },
        0
    );
    process.child.wait().unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_wait_failure_reaps_its_exact_descendant() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("wait-failure-descendant");
    let script = format!(
        "{marker_env}={marker} {test_exe} --exact process::tests::owned_process_descendant_helper --nocapture & while [ ! -f {marker} ]; do :; done; while :; do :; done",
        marker_env = OWNED_PROCESS_DESCENDANT_MARKER_ENV,
        marker = shell_quote_test_path(&marker),
        test_exe = shell_quote_test_path(&std::env::current_exe().unwrap()),
    );
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();
    let descendant = read_test_process_identity(&marker);

    assert!(matches!(
        finish_owned_process_wait(
            &mut process,
            Err(std::io::Error::other("injected wait failure")),
        ),
        Err(OwnedProcessTreeError::Await)
    ));
    assert_test_process_stopped(&descendant);
}

#[cfg(unix)]
#[test]
fn owned_process_wait_errors_only_release_identity_after_echild() {
    for errno in [libc::EINVAL, libc::ENOSYS] {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_owned_process(&mut command).unwrap();
        let error = std::io::Error::from_raw_os_error(errno);

        update_owned_process_identity_after_wait_error(&mut process, &error);

        assert!(process.process_group.is_some(), "errno {errno}");
        assert!(
            matches!(
                finish_owned_process_wait(&mut process, Err(error)),
                Err(OwnedProcessTreeError::Await)
            ),
            "errno {errno}"
        );
        assert!(process.cleanup_complete, "errno {errno}");
        assert!(process.process_group.is_none(), "errno {errno}");
    }

    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "while :; do :; done"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();
    let process_group = process.process_group.unwrap();
    let error = std::io::Error::from_raw_os_error(libc::ECHILD);
    update_owned_process_identity_after_wait_error(&mut process, &error);

    assert!(process.process_group.is_none());
    assert!(matches!(
        finish_owned_process_wait(&mut process, Err(error)),
        Err(OwnedProcessTreeError::Cleanup)
    ));
    assert!(!process.cleanup_complete);
    assert!(process.cleanup_finalized);
    let retained_error = process.cleanup_error.as_ref().unwrap().message.clone();

    process.process_group = Some(process_group);
    assert_eq!(
        process.terminate_and_reap().unwrap_err().to_string(),
        retained_error
    );
    // SAFETY: this is the exact original process group whose identity the
    // test removed artificially; terminate it solely for test teardown.
    assert_eq!(
        unsafe { libc::kill(-process_group.id.as_raw(), libc::SIGKILL) },
        0
    );
    process.child.wait().unwrap();
}

#[test]
fn windows_job_active_process_poll_is_bounded_and_propagates_query_errors() {
    let mut active = VecDeque::from([2, 1, 0]);
    wait_for_no_active_processes(Duration::from_secs(1), || {
        Ok(active.pop_front().unwrap_or(0))
    })
    .unwrap();
    assert!(active.is_empty());

    let error = wait_for_no_active_processes(Duration::from_secs(1), || {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected query failure",
        ))
    })
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);

    let started = std::time::Instant::now();
    let error = wait_for_no_active_processes(Duration::from_millis(20), || Ok(1)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[cfg(windows)]
#[test]
fn windows_owned_process_starts_suspended_in_a_new_process_group() {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    let flags = windows_owned_process_creation_flags();
    assert_eq!(flags, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
    assert_ne!(flags & CREATE_SUSPENDED, 0);
    assert_ne!(flags & CREATE_NEW_PROCESS_GROUP, 0);
}

#[cfg(windows)]
#[test]
fn owned_process_windows_job_reaps_descendants() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("windows-descendant");
    let ready = temp.path().join("windows-ready");
    let script = temp.path().join("owned-process-tree.cmd");
    fs::write(
            &script,
            format!(
                "@echo off\r\nif \"%~1\"==\"child\" goto child\r\nstart \"\" /b cmd.exe /d /c call \"%~f0\" child\r\n>\"{}\" echo ready\r\nping.exe -n 30 127.0.0.1 >nul\r\nexit /b\r\n:child\r\nping.exe -n 3 127.0.0.1 >nul\r\n>\"{}\" echo leaked\r\nexit /b\r\n",
                ready.display(),
                marker.display(),
            ),
        )
        .unwrap();
    let mut command = Command::new("cmd.exe");
    command
        .args(["/d", "/c"])
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();
    wait_for_test_file(&ready);
    assert!(matches!(
        finish_owned_process_wait(
            &mut process,
            Err(std::io::Error::other("injected wait failure")),
        ),
        Err(OwnedProcessTreeError::Await)
    ));

    std::thread::sleep(Duration::from_millis(2_500));
    assert!(
        !marker.exists(),
        "Windows Job Object left a descendant running"
    );
}
