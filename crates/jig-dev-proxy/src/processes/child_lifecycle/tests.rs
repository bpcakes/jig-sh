#[cfg(unix)]
use super::super::termination_test_guard;
use super::*;
use std::cell::Cell;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

#[cfg(unix)]
struct ProcessGroupChildGuard {
    child: Child,
    armed: bool,
}

#[cfg(unix)]
impl ProcessGroupChildGuard {
    fn new(child: Child) -> Self {
        Self { child, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl std::ops::Deref for ProcessGroupChildGuard {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

#[cfg(unix)]
impl std::ops::DerefMut for ProcessGroupChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

#[cfg(unix)]
impl Drop for ProcessGroupChildGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        // Preserve the direct child's wait status while every group signal
        // is attempted. If graceful cleanup fails, the production forced
        // path still owns the pinned identity.
        if terminate_child(&mut self.child).is_err() {
            let _ = force_kill_child(&mut self.child);
        }
        let _ = wait_after_terminate(&mut self.child);
        best_effort_direct_child_cleanup(&mut self.child);
    }
}

#[cfg(target_os = "macos")]
struct DirectChildGuard {
    child: Child,
    armed: bool,
}

#[cfg(target_os = "macos")]
impl DirectChildGuard {
    fn new(child: Child) -> Self {
        Self { child, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(target_os = "macos")]
impl std::ops::Deref for DirectChildGuard {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

#[cfg(target_os = "macos")]
impl std::ops::DerefMut for DirectChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

#[cfg(target_os = "macos")]
impl Drop for DirectChildGuard {
    fn drop(&mut self) {
        if self.armed {
            best_effort_direct_child_cleanup(&mut self.child);
        }
    }
}

#[cfg(unix)]
fn best_effort_direct_child_cleanup(child: &mut Child) {
    let deadline = Instant::now() + REAP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(None) => break,
            Err(error)
                if error.kind() == io::ErrorKind::Interrupted && Instant::now() < deadline => {}
            Ok(Some(_)) | Err(_) => return,
        }
    }

    loop {
        match child.kill() {
            Err(error)
                if error.kind() == io::ErrorKind::Interrupted && Instant::now() < deadline => {}
            Ok(()) | Err(_) => break,
        }
    }

    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error)
                if error.kind() == io::ErrorKind::Interrupted && Instant::now() < deadline => {}
            Ok(_) | Err(_) => return,
        }
    }
}

#[cfg(unix)]
struct ReleaseMarkerGuard {
    path: std::path::PathBuf,
    armed: bool,
}

#[cfg(unix)]
impl ReleaseMarkerGuard {
    fn new(path: &std::path::Path) -> Self {
        Self {
            path: path.to_owned(),
            armed: true,
        }
    }

    fn release_now(&self) -> io::Result<()> {
        std::fs::write(&self.path, b"release")
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for ReleaseMarkerGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::write(&self.path, b"release");
        }
    }
}

fn assert_path_stays_absent(path: &std::path::Path, duration: Duration) {
    let deadline = Instant::now() + duration;
    loop {
        assert!(
            !path.exists(),
            "unexpected marker appeared: {}",
            path.display()
        );
        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_path(path: &std::path::Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for marker: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "macos")]
fn wait_for_unreaped_exit(child: &mut Child, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match observe_child(child)? {
            ChildObservation::ExitedUnreaped(_) => return Ok(()),
            ChildObservation::Running if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(2));
            }
            ChildObservation::Running => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("child {} did not exit", child.id()),
                ));
            }
            ChildObservation::ExitedReaped(_) => {
                return Err(io::Error::other(format!(
                    "child {} was reaped before test inspection",
                    child.id()
                )));
            }
        }
    }
}

#[test]
fn exhausted_reap_and_termination_are_pid_specific() {
    let reap = wait_for_reap(4242, Duration::ZERO, || Ok(None)).unwrap_err();
    assert!(reap.to_string().contains("4242"));
    let deadline = Instant::now();
    let termination = confirm_exited_process_group_not_live_with(
        &mut (),
        4343,
        deadline,
        1,
        |(), _, _| panic!("an expired deadline must prevent signaling"),
        |(), _, _| panic!("an expired deadline must prevent membership proofs"),
        || deadline,
        |_| panic!("an expired deadline must prevent sleeping"),
    )
    .unwrap_err();
    assert!(termination.to_string().contains("4343"));
}

#[cfg(unix)]
#[test]
fn forced_group_confirmation_resignals_before_every_membership_proof() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(50);
    let clock = Cell::new(started);
    let proofs = Cell::new(vec![true, false, false]);
    let events = Cell::new(Vec::new());

    confirm_exited_process_group_not_live_with(
        &mut (),
        4343,
        deadline,
        2,
        |(), _, _| {
            let mut recorded = events.take();
            recorded.push("signal");
            events.set(recorded);
            Ok(())
        },
        |(), _, _| {
            let mut remaining = proofs.take();
            let proof = remaining.remove(0);
            proofs.set(remaining);
            let mut recorded = events.take();
            recorded.push("proof");
            events.set(recorded);
            Ok(proof)
        },
        || clock.get(),
        |duration| clock.set(clock.get() + duration),
    )
    .unwrap();

    assert_eq!(
        events.take(),
        vec!["signal", "proof", "signal", "proof", "signal", "proof"]
    );
    assert!(proofs.take().is_empty());
}

#[cfg(unix)]
#[test]
fn forced_group_confirmation_preserves_deadline_and_caps_its_sleep() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(5);
    let clock = Cell::new(started);
    let signals = Cell::new(0usize);
    let proofs = Cell::new(0usize);
    let sleeps = Cell::new(Vec::new());

    let error = confirm_exited_process_group_not_live_with(
        &mut (),
        4343,
        deadline,
        2,
        |(), _, _| {
            signals.set(signals.get() + 1);
            Ok(())
        },
        |(), _, _| {
            proofs.set(proofs.get() + 1);
            Ok(true)
        },
        || clock.get(),
        |duration| {
            let mut recorded = sleeps.take();
            recorded.push(duration);
            sleeps.set(recorded);
            clock.set(clock.get() + duration);
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("4343"));
    assert_eq!(signals.get(), 1, "deadline reset allowed another signal");
    assert_eq!(proofs.get(), 1);
    assert_eq!(sleeps.take(), vec![Duration::from_millis(5)]);
}

#[cfg(unix)]
#[test]
fn pinned_group_resignal_refuses_lost_or_reaped_identity_before_signaling() {
    let exited = ExitStatus::from_raw(0);
    let signals = Cell::new(0usize);
    let now = Instant::now();
    let deadline = now + Duration::from_secs(1);

    let reaped = resignal_pinned_exited_process_group_with(
        &mut (),
        4343,
        deadline,
        |()| Ok(ChildObservation::ExitedReaped(exited)),
        || now,
        |_, _| {
            signals.set(signals.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(reaped.to_string().contains("reaped"));
    assert_eq!(signals.get(), 0);

    let lost = resignal_pinned_exited_process_group_with(
        &mut (),
        4343,
        deadline,
        |()| Err(io::Error::from_raw_os_error(libc::ECHILD)),
        || now,
        |_, _| {
            signals.set(signals.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(lost.to_string().contains("revalidate"));
    assert_eq!(signals.get(), 0);
}

#[cfg(unix)]
#[test]
fn pinned_group_resignal_does_not_signal_after_observation_exhausts_deadline() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(5);
    let clock = Cell::new(started);
    let signals = Cell::new(0usize);

    let error = resignal_pinned_exited_process_group_with(
        &mut (),
        4343,
        deadline,
        |()| {
            clock.set(deadline);
            Ok(ChildObservation::ExitedUnreaped(ExitStatus::from_raw(0)))
        },
        || clock.get(),
        |_, _| {
            signals.set(signals.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("before re-signaling"));
    assert_eq!(signals.get(), 0);
}

#[cfg(unix)]
#[test]
fn esrch_group_resignal_still_requires_a_membership_proof() {
    let now = Instant::now();
    let signals = Cell::new(0usize);
    let proofs = Cell::new(0usize);

    confirm_exited_process_group_not_live_with(
        &mut (),
        4343,
        now + Duration::from_secs(1),
        1,
        |state, pid, deadline| {
            resignal_pinned_exited_process_group_with(
                state,
                pid,
                deadline,
                |()| Ok(ChildObservation::ExitedUnreaped(ExitStatus::from_raw(0))),
                || now,
                |target, signal| {
                    assert_eq!(target, -4343);
                    assert_eq!(signal, libc::SIGKILL);
                    signals.set(signals.get() + 1);
                    Err(io::Error::from_raw_os_error(libc::ESRCH))
                },
            )
        },
        |(), _, _| {
            proofs.set(proofs.get() + 1);
            Ok(false)
        },
        || now,
        |_| unreachable!(),
    )
    .unwrap();

    assert_eq!(signals.get(), 1);
    assert_eq!(proofs.get(), 1, "ESRCH was accepted as absence");
}

#[cfg(target_os = "macos")]
#[test]
fn eperm_group_resignal_retries_until_the_atomic_snapshot_is_quiescent() {
    let now = Instant::now();
    let signals = Cell::new(0usize);
    let proofs = Cell::new(vec![true, false]);

    confirm_exited_process_group_not_live_with(
        &mut (),
        4343,
        now + Duration::from_secs(1),
        1,
        |state, pid, deadline| {
            resignal_pinned_exited_process_group_with(
                state,
                pid,
                deadline,
                |()| Ok(ChildObservation::ExitedUnreaped(ExitStatus::from_raw(0))),
                || now,
                |_, _| {
                    signals.set(signals.get() + 1);
                    Err(io::Error::from_raw_os_error(libc::EPERM))
                },
            )
        },
        |(), _, _| {
            let mut remaining = proofs.take();
            let live = remaining.remove(0);
            proofs.set(remaining);
            Ok(live)
        },
        || now,
        |_| {},
    )
    .unwrap();

    assert_eq!(signals.get(), 2, "EPERM stopped confirmation early");
    assert!(proofs.take().is_empty());
}

#[cfg(unix)]
#[test]
fn unexpected_group_resignal_error_prevents_membership_proof() {
    let now = Instant::now();
    let proofs = Cell::new(0usize);

    let error = confirm_exited_process_group_not_live_with(
        &mut (),
        4343,
        now + Duration::from_secs(1),
        1,
        |state, pid, deadline| {
            resignal_pinned_exited_process_group_with(
                state,
                pid,
                deadline,
                |()| Ok(ChildObservation::ExitedUnreaped(ExitStatus::from_raw(0))),
                || now,
                |_, _| Err(io::Error::from_raw_os_error(libc::EIO)),
            )
        },
        |(), _, _| {
            proofs.set(proofs.get() + 1);
            Ok(false)
        },
        || now,
        |_| unreachable!(),
    )
    .unwrap_err();

    assert_eq!(
        error
            .downcast_ref::<io::Error>()
            .and_then(io::Error::raw_os_error),
        Some(libc::EIO)
    );
    assert_eq!(proofs.get(), 0);
}

#[test]
fn cleanup_reporting_preserves_the_callers_primary_error() {
    let primary = anyhow!("primary startup failure");
    assert!(!report_cleanup_result(
        Err(anyhow!("child process 4444 remained alive")),
        "could not clean up after primary failure",
    ));
    assert_eq!(primary.to_string(), "primary startup failure");
}

#[test]
fn unscannable_unix_exited_group_gets_term_grace_before_force() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(30);
    let clock = Cell::new(started);
    let sleeps = Cell::new(0usize);
    let cleanup_calls = Cell::new(0usize);
    wait_unscannable_exited_group_term_grace(
        &mut (),
        deadline,
        || clock.get(),
        |duration| {
            sleeps.set(sleeps.get() + 1);
            clock.set(clock.get() + duration);
        },
        || false,
        |()| {
            cleanup_calls.set(cleanup_calls.get() + 1);
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(sleeps.get(), 3);
    assert_eq!(cleanup_calls.get(), 1);
}

#[test]
fn second_signal_bypasses_unscannable_unix_term_grace() {
    let sleeps = Cell::new(0usize);
    let cleanup_calls = Cell::new(0usize);
    wait_unscannable_exited_group_term_grace(
        &mut (),
        Instant::now() + Duration::from_secs(2),
        Instant::now,
        |_| sleeps.set(sleeps.get() + 1),
        || true,
        |()| {
            cleanup_calls.set(cleanup_calls.get() + 1);
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(sleeps.get(), 0);
    assert_eq!(cleanup_calls.get(), 1);
}

#[test]
fn exited_group_term_grace_preserves_the_callers_nearly_spent_deadline() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(30);
    let clock = Cell::new(started + Duration::from_millis(25));
    let scans = Cell::new(0usize);
    let sleeps = Cell::new(Vec::new());
    let cleanup_calls = Cell::new(0usize);

    wait_scannable_exited_group_term_grace(
        &mut (),
        4242,
        deadline,
        |(), pid, received_deadline| {
            assert_eq!(pid, 4242);
            assert_eq!(received_deadline, deadline);
            scans.set(scans.get() + 1);
            Ok(true)
        },
        || clock.get(),
        |duration| {
            let mut observed = sleeps.take();
            observed.push(duration);
            sleeps.set(observed);
            clock.set(clock.get() + duration);
        },
        || false,
        |()| {
            cleanup_calls.set(cleanup_calls.get() + 1);
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(scans.get(), 1);
    assert_eq!(sleeps.take(), vec![Duration::from_millis(5)]);
    assert_eq!(cleanup_calls.get(), 1);
}

#[cfg(unix)]
#[test]
fn killed_leader_exit_preserves_the_original_confirmation_deadline() {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(30);
    let clock = Cell::new(started);
    let observations = Cell::new(0usize);
    let sleeps = Cell::new(Vec::new());
    let confirmed_deadline = Cell::new(None);

    let error = wait_for_killed_child_exit_and_confirm_with(
        &mut (),
        4343,
        deadline,
        |()| {
            let observation = observations.get();
            observations.set(observation + 1);
            if observation == 0 {
                clock.set(deadline - Duration::from_millis(1));
                Ok(ChildObservation::Running)
            } else {
                Ok(ChildObservation::ExitedUnreaped(ExitStatus::from_raw(0)))
            }
        },
        || clock.get(),
        |duration| {
            let mut observed = sleeps.take();
            observed.push(duration);
            sleeps.set(observed);
            clock.set(clock.get() + duration);
        },
        |(), pid, received_deadline| {
            assert_eq!(pid, 4343);
            confirmed_deadline.set(Some(received_deadline));
            Err(anyhow!("injected confirmation failure"))
        },
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "injected confirmation failure");
    assert_eq!(observations.get(), 2);
    assert_eq!(sleeps.take(), vec![Duration::from_millis(1)]);
    assert_eq!(confirmed_deadline.get(), Some(deadline));
}

#[cfg(unix)]
#[test]
fn child_cleanup_deadline_overflow_fails_closed() {
    let error = child_cleanup_deadline(4444, "injected phase", Duration::MAX).unwrap_err();
    let error = error.to_string();
    assert!(error.contains("4444"));
    assert!(error.contains("injected phase"));
    assert!(error.contains("overflowed"));
}

#[cfg(unix)]
#[test]
fn terminate_child_kills_process_group_after_wrapper_exits() {
    let _guard = termination_test_guard();
    use crate::test_tempdir as tempdir;
    #[cfg(target_os = "linux")]
    use std::fs;
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    let temp = tempdir().unwrap();
    let pid_path = temp.path().join("grandchild.pid");
    let release_path = temp.path().join("grandchild.release");
    let leak_path = temp.path().join("grandchild.leaked");
    let mut command = Command::new("sh");
    command
            .arg("-c")
            .arg(
                "(trap '' HUP; while [ ! -e \"$2\" ]; do sleep 0.01; done; printf leaked > \"$3\") & echo $! > \"$1\"",
            )
            .arg("sh")
            .arg(&pid_path)
            .arg(&release_path)
            .arg(&leak_path);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = ProcessGroupChildGuard::new(command.spawn().unwrap());
    let mut release_guard = ReleaseMarkerGuard::new(&release_path);
    let pid = child.id();
    for _ in 0..50 {
        if pid_path.exists() && matches!(try_wait_preserving_process_group(&mut child), Ok(Some(_)))
        {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let pid_path_exists = pid_path.exists();
    let child_exit = try_wait_preserving_process_group(&mut child);
    let group_alive = process_group_alive(pid);
    let termination = terminate_child(&mut child);
    #[cfg(target_os = "linux")]
    let linux_group_members = termination
        .as_ref()
        .ok()
        .map(|_| linux_process_group_has_live_members(pid, Instant::now() + KILL_CONFIRM_TIMEOUT));
    if termination.is_err() {
        let _ = force_kill_child(&mut child);
    }
    let reap = wait_after_terminate(&mut child);
    let release = release_guard.release_now();
    release.unwrap();
    assert_path_stays_absent(&leak_path, Duration::from_millis(300));

    assert!(pid_path_exists);
    assert!(child_exit.unwrap().is_some());
    assert!(group_alive.unwrap());
    termination.unwrap();
    #[cfg(target_os = "linux")]
    assert!(
        !linux_group_members.unwrap().unwrap(),
        "grandchild remained: {}",
        fs::read_to_string(pid_path).unwrap_or_default()
    );
    reap.unwrap();
    child.disarm();
    release_guard.disarm();
}

#[cfg(unix)]
#[test]
fn lifecycle_fixture_guards_clean_an_exited_wrapper_during_unwind() {
    let _guard = termination_test_guard();
    use crate::test_tempdir as tempdir;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let temp = tempdir().unwrap();
    let release_path = temp.path().join("unwind.release");
    let leak_path = temp.path().join("unwind.leaked");
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut command = Command::new("sh");
        command
                .arg("-c")
                .arg(
                    "(trap '' HUP; while [ ! -e \"$1\" ]; do sleep 0.01; done; sleep 0.2; printf leaked > \"$2\") & exit 0",
                )
                .arg("sh")
                .arg(&release_path)
                .arg(&leak_path);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = ProcessGroupChildGuard::new(command.spawn().unwrap());
        let _release_guard = ReleaseMarkerGuard::new(&release_path);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(try_wait_preserving_process_group(&mut child), Ok(Some(_))) {
                break;
            }
            assert!(Instant::now() < deadline, "test wrapper did not exit");
            thread::sleep(Duration::from_millis(2));
        }
        panic!("exercise panic-safe fixture cleanup");
    }));

    assert!(unwind.is_err());
    assert!(
        release_path.exists(),
        "unwind did not open the release barrier"
    );
    assert_path_stays_absent(&leak_path, Duration::from_millis(500));
}

#[cfg(target_os = "linux")]
#[test]
fn exited_zombie_only_group_is_reaped_without_liveness_timeout() {
    let _guard = termination_test_guard();
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let mut command = Command::new("sh");
    command.args(["-c", "exit 0"]);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = ProcessGroupChildGuard::new(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if try_wait_preserving_process_group(&mut child)
            .unwrap()
            .is_some()
        {
            break;
        }
        assert!(Instant::now() < deadline, "child did not exit");
        thread::sleep(Duration::from_millis(10));
    }

    let process_group_id = child.id();
    let cleanup = terminate_and_reap(&mut child);

    cleanup.unwrap();
    assert!(
        !process_group_alive(process_group_id).unwrap(),
        "zombie-only process group remained observable after cleanup"
    );
    child.disarm();
}

#[cfg(target_os = "linux")]
#[test]
fn running_term_resistant_tree_is_confirmed_dead_before_leader_reap() {
    let _guard = termination_test_guard();
    use crate::test_tempdir as tempdir;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let temp = tempdir().unwrap();
    let release_path = temp.path().join("term-resistant.release");
    let leader_ready = temp.path().join("term-resistant-leader.ready");
    let member_ready = temp.path().join("term-resistant-member.ready");
    let mut command = Command::new("sh");
    command
            .arg("-c")
            .arg(
                "trap '' TERM; sh -c 'trap \"\" TERM; printf ready > \"$2\"; while [ ! -e \"$1\" ]; do sleep 0.01; done' sh \"$1\" \"$3\" & printf ready > \"$2\"; while [ ! -e \"$1\" ]; do sleep 0.01; done",
            )
            .arg("sh")
            .arg(&release_path)
            .arg(&leader_ready)
            .arg(&member_ready);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = ProcessGroupChildGuard::new(command.spawn().unwrap());
    let mut release_guard = ReleaseMarkerGuard::new(&release_path);
    let pid = child.id();
    wait_for_path(&leader_ready, Duration::from_secs(2));
    wait_for_path(&member_ready, Duration::from_secs(2));

    let termination = terminate_child(&mut child);
    let live_members = termination
        .as_ref()
        .ok()
        .map(|_| linux_process_group_has_live_members(pid, Instant::now() + KILL_CONFIRM_TIMEOUT));
    if termination.is_err() {
        let _ = force_kill_child(&mut child);
    }
    let reap = wait_after_terminate(&mut child);
    let release = release_guard.release_now();

    release.unwrap();
    termination.unwrap();
    assert!(
        !live_members.unwrap().unwrap(),
        "SIGKILL cleanup returned while process group {pid} still had live members"
    );
    reap.unwrap();
    child.disarm();
    release_guard.disarm();
}

#[test]
fn macos_group_signal_eperm_keeps_additional_members_pending() {
    assert!(!classify_macos_group_signal_eperm(Ok(true)).unwrap());
    assert!(classify_macos_group_signal_eperm(Ok(false)).unwrap());
    assert!(classify_macos_group_signal_eperm(Err(io::Error::other("snapshot failed"))).is_err());
}

#[cfg(unix)]
#[test]
fn macos_group_quiescence_requires_an_unreaped_exit_and_exact_snapshot() {
    use std::os::unix::process::ExitStatusExt;

    let exited = ExitStatus::from_raw(0);
    assert!(
        classify_macos_process_group_quiescence(ChildObservation::ExitedUnreaped(exited), || Ok(
            true
        ),)
        .unwrap()
    );
    assert!(
        !classify_macos_process_group_quiescence(ChildObservation::ExitedUnreaped(exited), || Ok(
            false
        ),)
        .unwrap()
    );
    assert!(
        !classify_macos_process_group_quiescence(ChildObservation::Running, || {
            panic!("a running leader must not reach the membership snapshot")
        })
        .unwrap()
    );
    assert!(
        classify_macos_process_group_quiescence(ChildObservation::ExitedReaped(exited), || {
            panic!("a reaped leader must not reach the membership snapshot")
        })
        .is_err()
    );
    assert!(
        classify_macos_process_group_quiescence(ChildObservation::ExitedUnreaped(exited), || Err(
            io::Error::other("snapshot failed")
        ),)
        .is_err()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_group_confirmation_resignals_an_exited_leader_with_a_live_member() {
    let _guard = termination_test_guard();
    use crate::test_tempdir as tempdir;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let temp = tempdir().unwrap();
    let ready = temp.path().join("group-member.ready");
    let release = temp.path().join("group-member.release");
    let leak = temp.path().join("group-member.leaked");
    let mut command = Command::new("sh");
    command
            .arg("-c")
            .arg(
                "(printf ready > \"$1\"; while [ ! -e \"$2\" ]; do sleep 0.01; done; printf leaked > \"$3\") & while [ ! -e \"$1\" ]; do sleep 0.01; done; exit 0",
            )
            .arg("sh")
            .arg(&ready)
            .arg(&release)
            .arg(&leak)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = ProcessGroupChildGuard::new(command.spawn().unwrap());
    let mut release_guard = ReleaseMarkerGuard::new(&release);
    let pid = child.id();
    wait_for_path(&ready, Duration::from_secs(2));

    let observation_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if matches!(
            observe_child(&mut child).unwrap(),
            ChildObservation::ExitedUnreaped(_)
        ) {
            break;
        }
        assert!(
            Instant::now() < observation_deadline,
            "test group leader did not exit"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let confirmation = confirm_exited_process_group_not_live(
        &mut child,
        pid,
        Instant::now() + Duration::from_millis(500),
    );
    let cleanup = terminate_and_reap(&mut child);
    let release = release_guard.release_now();
    release.unwrap();
    assert_path_stays_absent(&leak, Duration::from_millis(300));

    confirmation.unwrap();
    cleanup.unwrap();
    child.disarm();
    release_guard.disarm();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_group_signal_eperm_keeps_a_retained_zombie_member_pending() {
    let _guard = termination_test_guard();
    use crate::test_tempdir as tempdir;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let temp = tempdir().unwrap();
    let ready = temp.path().join("zombie-group.ready");
    let release = temp.path().join("zombie-group.release");
    let mut leader_command = Command::new("sh");
    leader_command
        .arg("-c")
        .arg("printf ready > \"$1\"; while [ ! -e \"$2\" ]; do sleep 0.01; done; exit 0")
        .arg("sh")
        .arg(&ready)
        .arg(&release)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        leader_command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut leader = ProcessGroupChildGuard::new(leader_command.spawn().unwrap());
    let mut release_guard = ReleaseMarkerGuard::new(&release);
    let leader_pid = leader.id();
    let process_group = checked_pid_io(leader_pid).unwrap();
    wait_for_path(&ready, Duration::from_secs(2));

    let mut member_command = Command::new("sh");
    member_command
        .args(["-c", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        member_command.pre_exec(move || {
            if libc::setpgid(0, process_group) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut member = DirectChildGuard::new(member_command.spawn().unwrap());
    let member_observation = wait_for_unreaped_exit(&mut member, Duration::from_secs(2));
    member_observation.unwrap();

    release_guard.release_now().unwrap();
    let leader_observation = wait_for_unreaped_exit(&mut leader, Duration::from_secs(2));
    leader_observation.unwrap();

    let snapshot = macos_process_group_contains_only_pinned_leader(leader_pid);
    let raw_signal = send_signal(-process_group, libc::SIGTERM);
    let production_signal = signal_exited_process_group(&mut leader, leader_pid, libc::SIGTERM);
    let confirmation = confirm_exited_process_group_not_live(
        &mut leader,
        leader_pid,
        Instant::now() + Duration::from_millis(40),
    );

    let member_status = member.wait();
    let cleanup = terminate_and_reap(&mut leader);

    assert!(!snapshot.unwrap(), "two retained zombies looked quiescent");
    assert_eq!(
        raw_signal.unwrap_err().raw_os_error(),
        Some(libc::EPERM),
        "Darwin did not expose the zombie-only EPERM state"
    );
    assert!(
        production_signal.unwrap(),
        "non-sole EPERM must stay pending"
    );
    assert!(confirmation.is_err(), "an extra zombie was accepted");
    assert!(member_status.unwrap().success());
    cleanup.unwrap();
    member.disarm();
    leader.disarm();
    release_guard.disarm();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_stopped_leader_remains_running_until_forced_cleanup() {
    let _guard = termination_test_guard();
    use crate::test_tempdir as tempdir;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let temp = tempdir().unwrap();
    let ready = temp.path().join("stopped-leader.ready");
    let leak = temp.path().join("stopped-leader.leaked");
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("printf ready > \"$1\"; kill -STOP $$; printf leaked > \"$2\"")
        .arg("sh")
        .arg(&ready)
        .arg(&leak)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = DirectChildGuard::new(command.spawn().unwrap());
    let pid = checked_pid_io(child.id()).unwrap();
    wait_for_path(&ready, Duration::from_secs(2));

    let stop_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let result = unsafe {
            // SAFETY: information is writable siginfo_t storage, pid is
            // our direct child, and WNOWAIT preserves the stop record for
            // the production observer exercised below.
            libc::waitid(
                libc::P_PID,
                pid as _,
                information.as_mut_ptr(),
                libc::WSTOPPED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        assert_eq!(result, 0, "waitid failed: {}", io::Error::last_os_error());
        let information = unsafe {
            // SAFETY: successful waitid initialized information.
            information.assume_init()
        };
        if unsafe { information.si_pid() } == pid && information.si_code == libc::CLD_STOPPED {
            break;
        }
        assert!(
            Instant::now() < stop_deadline,
            "test child did not publish a stopped state"
        );
        thread::sleep(Duration::from_millis(2));
    }

    let observation = try_wait_preserving_process_group(&mut child);
    let cleanup = force_kill_child(&mut child);
    let reap = wait_after_terminate(&mut child);
    assert_path_stays_absent(&leak, Duration::from_millis(300));

    assert!(observation.unwrap().is_none());
    cleanup.unwrap();
    reap.unwrap();
    child.disarm();
}

#[cfg(unix)]
#[test]
fn liveness_probes_only_classify_esrch_as_absent() {
    assert!(classify_liveness_probe(0, || io::Error::other("unused")).unwrap());
    assert!(!classify_liveness_probe(-1, || io::Error::from_raw_os_error(libc::ESRCH)).unwrap());
    assert_eq!(
        classify_liveness_probe(-1, || io::Error::from_raw_os_error(libc::EPERM))
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EPERM)
    );
    assert_eq!(
        classify_liveness_probe(-1, || io::Error::from_raw_os_error(libc::EIO))
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EIO)
    );
}

#[cfg(unix)]
#[test]
fn exited_group_term_esrch_continues_into_forced_confirmation() {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut calls = (0usize, 0usize);

    finish_exited_process_group_after_term_signal(
        &mut calls,
        4343,
        deadline,
        Ok(false),
        |calls, pid, received_deadline| {
            calls.0 += 1;
            assert_eq!(pid, 4343);
            assert_eq!(received_deadline, deadline);
            let keep_waiting = continue_exited_group_term_grace(
                calls,
                Ok(false),
                "injected empty TERM scan",
                |calls| {
                    calls.1 += 1;
                    Ok(())
                },
            )?;
            assert!(!keep_waiting);
            Ok(())
        },
        |_| panic!("an ESRCH result is not a graceful signal error"),
    )
    .unwrap();

    assert_eq!(calls, (1, 1));
}

#[cfg(unix)]
#[test]
fn empty_exited_group_term_scan_transitions_to_forced_cleanup() {
    let mut force_attempts = 0;

    let keep_waiting = continue_exited_group_term_grace(
        &mut force_attempts,
        Ok(false),
        "unused scan context",
        |attempts| {
            *attempts += 1;
            Ok(())
        },
    )
    .unwrap();

    assert!(!keep_waiting);
    assert_eq!(force_attempts, 1);
}

#[cfg(unix)]
#[test]
fn graceful_liveness_error_attempts_force_before_propagating_confirmation_error() {
    let mut force_attempts = 0;
    let scan_error = anyhow!("injected liveness scan failure");

    let error = continue_exited_group_term_grace(
        &mut force_attempts,
        Err(scan_error),
        "could not prove graceful liveness",
        |attempts| {
            *attempts += 1;
            Err(anyhow!("injected post-SIGKILL confirmation failure"))
        },
    )
    .unwrap_err();

    assert_eq!(force_attempts, 1, "forced cleanup was not attempted");
    let error = format!("{error:#}");
    assert!(error.contains("injected liveness scan failure"));
    assert!(error.contains("injected post-SIGKILL confirmation failure"));
}

#[test]
fn linux_proc_scan_skips_only_proven_unrelated_unreadable_processes() {
    let live = linux_process_group_has_live_members_with(
        77,
        [10, 11],
        |pid| {
            if pid == 10 {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected hidepid denial",
                ))
            } else {
                Ok("11 (owned worker) S 1 77 77 0".to_string())
            }
        },
        |pid| {
            assert_eq!(pid, 10, "readable entries do not need a fallback lookup");
            Ok(Some(9))
        },
        || true,
    )
    .unwrap();

    assert!(live);
}

#[test]
fn linux_proc_scan_fails_closed_for_an_unreadable_owned_process() {
    let error = linux_process_group_has_live_members_with(
        77,
        [10],
        |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected owned denial",
            ))
        },
        |_| Ok(Some(77)),
        || true,
    )
    .unwrap_err();

    assert!(error.to_string().contains("owned process group 77"));
    assert_eq!(
        error
            .root_cause()
            .downcast_ref::<io::Error>()
            .map(io::Error::kind),
        Some(io::ErrorKind::PermissionDenied)
    );
}

#[test]
fn linux_proc_scan_fails_closed_when_membership_cannot_be_classified() {
    let error = linux_process_group_has_live_members_with(
        77,
        [10],
        |_| Err(io::Error::other("injected stat failure")),
        |_| Err(io::Error::other("injected getpgid failure")),
        || true,
    )
    .unwrap_err();

    assert!(error.to_string().contains("or prove"));
    assert!(error.to_string().contains("injected getpgid failure"));
}

#[test]
fn linux_proc_scan_treats_vanished_and_dead_members_as_absent() {
    let live = linux_process_group_has_live_members_with(
        77,
        [10, 11, 12, 13, 14],
        |pid| match pid {
            10 => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "hidden or vanished",
            )),
            11 => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed unrelated stat",
            )),
            12 => Ok("12 (zombie worker) Z 1 77 77 0".to_string()),
            13 => Ok("13 (dead worker) X 1 77 77 0".to_string()),
            14 => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "vanished before fallback lookup",
            )),
            _ => unreachable!(),
        },
        |pid| match pid {
            10 | 14 => Ok(None),
            11 => Ok(Some(9)),
            _ => panic!("unexpected fallback lookup for {pid}"),
        },
        || true,
    )
    .unwrap();

    assert!(!live);
}

#[test]
fn linux_proc_scan_does_not_treat_enoent_as_owned_process_absence() {
    let error = linux_process_group_has_live_members_with(
        77,
        [10],
        |_| {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "hidepid can present an existing stat file as absent",
            ))
        },
        |_| Ok(Some(77)),
        || true,
    )
    .unwrap_err();

    assert!(error.to_string().contains("owned process group 77"));
    assert_eq!(
        error
            .root_cause()
            .downcast_ref::<io::Error>()
            .map(io::Error::kind),
        Some(io::ErrorKind::NotFound)
    );
}

#[test]
fn linux_proc_enumeration_fails_closed_when_budget_expires_while_advancing() {
    let within_budget = Cell::new(true);
    let entries = std::iter::from_fn(|| {
        within_budget.set(false);
        None::<io::Result<u32>>
    });

    let error =
        collect_linux_process_ids_with(77, entries, Some, || within_budget.get()).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cleanup scan exceeded its deadline")
    );
}

#[test]
fn linux_proc_scan_fails_closed_when_stat_read_exhausts_budget() {
    let within_budget = Cell::new(true);
    let membership_checks = Cell::new(0);

    let error = linux_process_group_has_live_members_with(
        77,
        [10],
        |_| {
            within_budget.set(false);
            Ok("10 (owned worker) S 1 77 77 0".to_string())
        },
        |_| {
            membership_checks.set(membership_checks.get() + 1);
            Ok(Some(77))
        },
        || within_budget.get(),
    )
    .unwrap_err();

    assert_eq!(membership_checks.get(), 0);
    assert!(
        error
            .to_string()
            .contains("cleanup scan exceeded its deadline")
    );
}

#[test]
fn linux_proc_scan_fails_closed_when_membership_lookup_exhausts_budget() {
    let within_budget = Cell::new(true);

    let error = linux_process_group_has_live_members_with(
        77,
        [10],
        |_| Err(io::Error::other("injected stat failure")),
        |_| {
            within_budget.set(false);
            Ok(None)
        },
        || within_budget.get(),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cleanup scan exceeded its deadline")
    );
}

#[test]
fn linux_proc_scan_checks_budget_before_live_and_empty_results() {
    let live_checks = Cell::new(0usize);
    let live_error = linux_process_group_has_live_members_with(
        77,
        [10],
        |_| Ok("10 (owned worker) S 1 77 77 0".to_string()),
        |_| unreachable!(),
        || {
            let check = live_checks.get() + 1;
            live_checks.set(check);
            check <= 3
        },
    )
    .unwrap_err();
    assert_eq!(live_checks.get(), 4);
    assert!(
        live_error
            .to_string()
            .contains("cleanup scan exceeded its deadline")
    );

    let empty_checks = Cell::new(0usize);
    let empty_error = linux_process_group_has_live_members_with(
        77,
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
    assert!(
        empty_error
            .to_string()
            .contains("cleanup scan exceeded its deadline")
    );
}

#[test]
fn linux_proc_stat_parser_handles_parentheses_and_dead_states() {
    assert_eq!(
        parse_linux_process_stat("42 (name ) with spaces) S 1 77 77 0".to_string()).unwrap(),
        LinuxProcessObservation {
            process_group: 77,
            live: true,
        }
    );
    for state in ["Z", "X", "x"] {
        assert!(
            !parse_linux_process_stat(format!("42 (worker) {state} 1 77 77 0"))
                .unwrap()
                .live
        );
    }
}
