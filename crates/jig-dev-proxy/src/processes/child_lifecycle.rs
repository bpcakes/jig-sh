use std::io;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(any(unix, test))]
use jig_owned_process::unix::ConsecutiveQuiescence;
#[cfg(target_os = "macos")]
use jig_owned_process::unix::{
    MacosProcessGroupSnapshotError,
    macos_process_group_contains_only_pinned_leader as shared_macos_process_group_contains_only_pinned_leader,
};
#[cfg(all(unix, not(target_os = "redox")))]
use jig_owned_process::unix::{
    ProcessGroupId, UnreapedChildObservation, WaitidClassificationError, classify_waitid_status,
    waitid_without_reaping,
};

#[cfg(unix)]
use crate::unix_pid;

use super::cleanup::force_cleanup_requested;

const REAP_TIMEOUT: Duration = Duration::from_secs(2);
const TERMINATE_TIMEOUT: Duration = Duration::from_secs(2);
const KILL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct AppProcessLease;

pub(super) const fn register_app_child(_child: &mut Child) -> AppProcessLease {
    AppProcessLease
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildObservation {
    Running,
    ExitedUnreaped(ExitStatus),
    ExitedReaped(ExitStatus),
}

pub(super) fn try_wait_preserving_process_group(
    child: &mut Child,
) -> io::Result<Option<ExitStatus>> {
    Ok(match observe_child(child)? {
        ChildObservation::Running => None,
        ChildObservation::ExitedUnreaped(status) | ChildObservation::ExitedReaped(status) => {
            Some(status)
        }
    })
}

#[cfg(all(unix, not(target_os = "redox")))]
fn observe_child(child: &mut Child) -> io::Result<ChildObservation> {
    let pid = checked_pid_io(child.id())?;
    let process_group = ProcessGroupId::new(pid).map_err(|_| {
        io::Error::other(format!("child PID {} exceeds platform range", child.id()))
    })?;
    loop {
        match waitid_without_reaping(process_group) {
            Ok(status) => {
                return classify_waitid_status(process_group, status)
                    .map(|observation| match observation {
                        UnreapedChildObservation::Running => ChildObservation::Running,
                        UnreapedChildObservation::Exited(status) => {
                            ChildObservation::ExitedUnreaped(status)
                        }
                    })
                    .map_err(dev_proxy_waitid_classification_error);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
                // Child caches statuses it reaps itself. If some other SIGCHLD
                // consumer reaped the leader, propagate ECHILD rather than ever
                // treating the now-recyclable numeric PID as an owned group.
                return child
                    .try_wait()?
                    .map(ChildObservation::ExitedReaped)
                    .ok_or(error);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(all(unix, not(target_os = "redox")))]
fn dev_proxy_waitid_classification_error(error: WaitidClassificationError) -> io::Error {
    match error {
        WaitidClassificationError::UnexpectedPid {
            expected: expected_pid,
            observed: observed_pid,
        } => io::Error::other(format!(
            "waitid observed unexpected child PID {observed_pid} instead of {expected_pid}"
        )),
        WaitidClassificationError::UnexpectedCode(code) => io::Error::new(
            io::ErrorKind::InvalidData,
            format!("waitid returned unexpected child status code {code}"),
        ),
    }
}

#[cfg(any(not(unix), target_os = "redox"))]
fn observe_child(child: &mut Child) -> io::Result<ChildObservation> {
    Ok(match child.try_wait()? {
        Some(status) => ChildObservation::ExitedReaped(status),
        None => ChildObservation::Running,
    })
}

pub(super) fn terminate_and_reap(child: &mut Child) -> Result<()> {
    terminate_child(child)?;
    wait_after_terminate(child)
}

pub(super) fn terminate_and_reap_logged(child: &mut Child, context: &str) -> bool {
    report_cleanup_result(terminate_and_reap(child), context)
}

fn report_cleanup_result(result: Result<()>, context: &str) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("jig proxy {context}; child cleanup also failed: {error:#}");
            false
        }
    }
}

#[cfg(unix)]
fn child_cleanup_deadline(pid: u32, phase: &str, timeout: Duration) -> Result<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("child process {pid} {phase} deadline overflowed"))
}

#[cfg(any(unix, test))]
fn remaining_phase_budget(deadline: Instant, now: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
}

pub(super) fn wait_after_terminate(child: &mut Child) -> Result<()> {
    let pid = child.id();
    wait_for_reap(pid, REAP_TIMEOUT, || child.try_wait())
}

fn wait_for_reap(
    pid: u32,
    timeout: Duration,
    mut try_wait: impl FnMut() -> io::Result<Option<ExitStatus>>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                bail!("child process {pid} was still running after {timeout:?} reap deadline")
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to reap child process {pid}"));
            }
        }
    }
}

#[cfg(unix)]
pub(super) fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    let direct_child_exited = match observe_child(child)
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
    {
        ChildObservation::Running => false,
        ChildObservation::ExitedUnreaped(_) => true,
        // Once reaped, the numeric PID/PGID may already have been recycled.
        // Refuse every process-group signal rather than risk another tree.
        ChildObservation::ExitedReaped(_) => return Ok(()),
    };
    if force_cleanup_requested() {
        return force_kill_child(child);
    }
    let term_deadline = child_cleanup_deadline(pid, "SIGTERM grace", TERMINATE_TIMEOUT)?;
    if direct_child_exited {
        let signal_result = signal_exited_process_group(child, pid, libc::SIGTERM);
        return finish_exited_process_group_after_term_signal(
            child,
            pid,
            term_deadline,
            signal_result,
            finish_exited_process_group_after_term,
            force_kill_child,
        );
    } else if let Err(error) = terminate_pid(pid) {
        return force_after_graceful_cleanup_error(
            child,
            &error,
            &format!("failed to send SIGTERM to child process/group {pid}"),
            force_kill_child,
        );
    }
    while remaining_phase_budget(term_deadline, Instant::now()).is_some() {
        if force_cleanup_requested() {
            return force_kill_child(child);
        }
        if !direct_child_exited {
            let observation = match observe_child(child) {
                Ok(observation) => observation,
                Err(error) => {
                    return force_after_graceful_cleanup_error(
                        child,
                        &error,
                        &format!("failed to inspect child process {pid} during termination"),
                        force_kill_child,
                    );
                }
            };
            match observation {
                ChildObservation::Running => {}
                ChildObservation::ExitedUnreaped(_) => {
                    return finish_exited_process_group_after_term(child, pid, term_deadline);
                }
                ChildObservation::ExitedReaped(_) => return Ok(()),
            }
        }
        let target_alive = match running_child_target_alive(child, pid) {
            Ok(target_alive) => target_alive,
            Err(error) => {
                return force_after_graceful_cleanup_error(
                    child,
                    &error,
                    &format!("failed to confirm child process/group {pid} liveness after SIGTERM"),
                    force_kill_child,
                );
            }
        };
        if !target_alive {
            return Ok(());
        }
        let Some(remaining) = remaining_phase_budget(term_deadline, Instant::now()) else {
            break;
        };
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    force_kill_child(child)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn finish_exited_process_group_after_term_signal<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    signal_result: Result<bool>,
    finish_grace: impl FnOnce(&mut T, u32, Instant) -> Result<()>,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    match signal_result {
        // `false` means the group signal reported ESRCH (or macOS EPERM after
        // a sole-leader snapshot). Neither signal result alone replaces the
        // normal TERM-grace transition into forced signal and confirmation.
        Ok(_) => finish_grace(state, pid, deadline),
        Err(error) => force_after_graceful_cleanup_error(
            state,
            &error,
            &format!("failed to send SIGTERM to exited child process group {pid}"),
            force_cleanup,
        ),
    }
}

#[cfg(unix)]
fn finish_exited_process_group_after_term(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    // The unreaped leader pins the process-group identity. Give live
    // descendants their normal TERM grace period, but exclude that zombie
    // leader from liveness. An empty TERM-phase scan is only a transition to
    // SIGKILL: a TERM-ignoring member could fork and exit across snapshots, so
    // only post-SIGKILL empty scans can prove that the group is quiescent.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        wait_scannable_exited_group_term_grace(
            child,
            pid,
            deadline,
            exited_process_group_has_live_members,
            Instant::now,
            thread::sleep,
            force_cleanup_requested,
            force_kill_child,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        wait_unscannable_exited_group_term_grace(
            child,
            deadline,
            Instant::now,
            thread::sleep,
            force_cleanup_requested,
            force_kill_child,
        )
    }
}

#[cfg(any(test, all(unix, any(target_os = "linux", target_os = "macos"))))]
#[allow(clippy::too_many_arguments)]
fn wait_scannable_exited_group_term_grace<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    mut live_members: impl FnMut(&mut T, u32, Instant) -> Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    mut forced: impl FnMut() -> bool,
    mut force_cleanup: impl FnMut(&mut T) -> Result<()>,
) -> Result<()> {
    loop {
        let Some(_) = remaining_phase_budget(deadline, now()) else {
            return force_cleanup(state);
        };
        if forced() {
            return force_cleanup(state);
        }
        let live_members = live_members(state, pid, deadline);
        let keep_waiting = continue_exited_group_term_grace(
            state,
            live_members,
            &format!(
                "failed to confirm live membership of exited child process group {pid} after SIGTERM"
            ),
            |state| force_cleanup(state),
        )?;
        if !keep_waiting {
            return Ok(());
        }
        let Some(remaining) = remaining_phase_budget(deadline, now()) else {
            return force_cleanup(state);
        };
        sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(any(test, all(unix, not(any(target_os = "linux", target_os = "macos")))))]
fn wait_unscannable_exited_group_term_grace<T>(
    state: &mut T,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    mut forced: impl FnMut() -> bool,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    while let Some(remaining) = remaining_phase_budget(deadline, now()) {
        if forced() {
            return force_cleanup(state);
        }
        sleep(remaining.min(Duration::from_millis(10)));
    }
    force_cleanup(state)
}

#[cfg(any(unix, test))]
fn continue_exited_group_term_grace<T>(
    state: &mut T,
    live_members: Result<bool>,
    context: &str,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<bool> {
    match live_members {
        Ok(true) => Ok(true),
        Ok(false) => {
            force_cleanup(state)?;
            Ok(false)
        }
        Err(error) => {
            force_after_graceful_cleanup_error(state, &error, context, force_cleanup)?;
            Ok(false)
        }
    }
}

#[cfg(any(unix, test))]
fn force_after_graceful_cleanup_error<T>(
    state: &mut T,
    graceful_error: &dyn std::fmt::Display,
    context: &str,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    force_cleanup(state).with_context(|| {
        format!(
            "{context}; forced cleanup also failed after graceful cleanup error: {graceful_error}"
        )
    })
}

#[cfg(all(unix, target_os = "linux"))]
fn exited_process_group_has_live_members(
    _child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<bool> {
    linux_process_group_has_live_members(pid, deadline)
}

#[cfg(all(unix, target_os = "macos"))]
fn exited_process_group_has_live_members(
    child: &mut Child,
    pid: u32,
    _deadline: Instant,
) -> Result<bool> {
    macos_process_group_is_quiescent(child, pid)
        .map(|quiescent| !quiescent)
        .map_err(Into::into)
}

#[cfg(unix)]
fn force_kill_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    let deadline = child_cleanup_deadline(pid, "SIGKILL confirmation", KILL_CONFIRM_TIMEOUT)?;
    let observation = match observe_child(child) {
        Ok(observation) => observation,
        Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
            return Err(error).with_context(|| {
                format!(
                    "lost ownership of child process {pid} before forced cleanup; refusing to signal a recyclable process identity"
                )
            });
        }
        Err(error) => {
            // terminate_child established direct-child ownership before any
            // graceful work. A non-ECHILD waitid failure does not consume that
            // wait status, so force the pinned identity instead of allowing a
            // persistent observation error to suppress SIGKILL entirely.
            return force_kill_pinned_child(child, pid, deadline).with_context(|| {
                format!(
                    "failed to reobserve pinned child process {pid} before forced cleanup: {error}"
                )
            });
        }
    };
    match observation {
        ChildObservation::ExitedUnreaped(_) => {
            confirm_exited_process_group_not_live(child, pid, deadline)
        }
        ChildObservation::ExitedReaped(_) => Ok(()),
        ChildObservation::Running => force_kill_pinned_child(child, pid, deadline),
    }
}

#[cfg(unix)]
fn force_kill_pinned_child(child: &mut Child, pid: u32, deadline: Instant) -> Result<()> {
    let signal_result = kill_pid(pid);
    let child_result = child
        .kill()
        .with_context(|| format!("failed final Child::kill for process {pid}"));
    if let Err(signal_error) = signal_result
        && let Err(child_error) = child_result
    {
        return Err(
            signal_error.context(format!("fallback child kill also failed: {child_error:#}"))
        );
    }
    wait_for_killed_child_exit_and_confirm(child, pid, deadline)
}

#[cfg(unix)]
fn wait_for_killed_child_exit_and_confirm(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    wait_for_killed_child_exit_and_confirm_with(
        child,
        pid,
        deadline,
        observe_child,
        Instant::now,
        thread::sleep,
        confirm_exited_process_group_not_live,
    )
}

#[cfg(any(unix, test))]
#[allow(clippy::too_many_arguments)]
fn wait_for_killed_child_exit_and_confirm_with<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    mut observe: impl FnMut(&mut T) -> io::Result<ChildObservation>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    mut confirm: impl FnMut(&mut T, u32, Instant) -> Result<()>,
) -> Result<()> {
    loop {
        match observe(state)
            .with_context(|| format!("failed to observe child process {pid} after SIGKILL"))?
        {
            ChildObservation::Running => {
                let Some(remaining) = remaining_phase_budget(deadline, now()) else {
                    bail!(
                        "child process {pid} remained running through its SIGKILL confirmation deadline"
                    )
                };
                sleep(remaining.min(Duration::from_millis(10)));
            }
            ChildObservation::ExitedUnreaped(_) => {
                return confirm(state, pid, deadline);
            }
            ChildObservation::ExitedReaped(_) => {
                bail!(
                    "child process {pid} was reaped before its process group cleanup could be confirmed"
                )
            }
        }
    }
}

#[cfg(not(unix))]
pub(super) fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    if child
        .try_wait()
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
        .is_none()
    {
        if force_cleanup_requested() {
            kill_pid(pid)?;
        } else {
            terminate_pid(pid)?;
        }
    }
    Ok(())
}

#[cfg(any(unix, test))]
#[allow(clippy::too_many_arguments)]
fn confirm_exited_process_group_not_live_with<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    required_empty_proofs: u8,
    mut resignal: impl FnMut(&mut T, u32, Instant) -> Result<()>,
    mut live_members: impl FnMut(&mut T, u32, Instant) -> Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> Result<()> {
    let mut quiescence = ConsecutiveQuiescence::new(required_empty_proofs).map_err(|_| {
        anyhow!("child process group {pid} confirmation requires at least one empty proof")
    })?;
    loop {
        let Some(_) = remaining_phase_budget(deadline, now()) else {
            bail!(
                "child process group {pid} retained live members through its SIGKILL confirmation deadline"
            );
        };
        resignal(state, pid, deadline)?;
        let Some(_) = remaining_phase_budget(deadline, now()) else {
            bail!(
                "child process group {pid} retained live members through its SIGKILL confirmation deadline"
            );
        };
        let quiescent = quiescence.observe(!live_members(state, pid, deadline)?);
        let Some(remaining) = remaining_phase_budget(deadline, now()) else {
            bail!(
                "child process group {pid} retained live members through its SIGKILL confirmation deadline"
            )
        };
        if quiescent {
            return Ok(());
        }
        sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(all(unix, target_os = "macos"))]
fn running_child_target_alive(child: &mut Child, pid: u32) -> io::Result<bool> {
    match process_group_or_pid_alive(pid) {
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            macos_process_group_is_quiescent(child, pid).map(|quiescent| !quiescent)
        }
        result => result,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn running_child_target_alive(_child: &mut Child, pid: u32) -> io::Result<bool> {
    process_group_or_pid_alive(pid)
}
#[cfg(unix)]
pub(super) fn terminate_pid(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group_or_pid(pid, libc::SIGTERM)
        .with_context(|| format!("failed to send SIGTERM to child process/group {pid}"))
}
#[cfg(unix)]
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group_or_pid(pid, libc::SIGKILL)
        .with_context(|| format!("failed to send SIGKILL to child process/group {pid}"))
}
#[cfg(unix)]
fn checked_pid(pid: u32) -> Result<i32> {
    unix_pid(pid).ok_or_else(|| anyhow!("child PID {pid} exceeds platform process-id range"))
}
#[cfg(unix)]
fn checked_pid_io(pid: u32) -> io::Result<i32> {
    unix_pid(pid).ok_or_else(|| io::Error::other(format!("child PID {pid} exceeds platform range")))
}
#[cfg(unix)]
fn signal_group_or_pid(pid: i32, signal: i32) -> io::Result<()> {
    match send_signal(-pid, signal) {
        Ok(()) => Ok(()),
        Err(group_error) => match send_signal(pid, signal) {
            Ok(()) => Ok(()),
            Err(pid_error) if pid_error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            Err(pid_error) => Err(io::Error::new(
                pid_error.kind(),
                format!("group signal failed: {group_error}; direct signal failed: {pid_error}"),
            )),
        },
    }
}
#[cfg(unix)]
fn signal_exited_process_group(child: &mut Child, pid: u32, signal: i32) -> Result<bool> {
    #[cfg(not(target_os = "macos"))]
    let _ = child;
    let process_group = checked_pid(pid)?;
    match send_signal(-process_group, signal) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        #[cfg(target_os = "macos")]
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            // Darwin also returns EPERM when any live group member is not
            // signalable. Accept it only after a fresh exact-PID WNOWAIT
            // observation and one atomic snapshot prove the exited leader is
            // the sole remaining member of this pinned group generation.
            classify_macos_group_signal_eperm(macos_process_group_is_quiescent(child, pid))
                .map_err(Into::into)
        }
        Err(error) => Err(error).with_context(|| {
            format!("failed to send signal {signal} to child process group {process_group}")
        }),
    }
}

#[cfg(unix)]
fn resignal_pinned_exited_process_group(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    resignal_pinned_exited_process_group_with(
        child,
        pid,
        deadline,
        observe_child,
        Instant::now,
        send_signal,
    )
}

#[cfg(unix)]
fn resignal_pinned_exited_process_group_with<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    mut observe: impl FnMut(&mut T) -> io::Result<ChildObservation>,
    mut now: impl FnMut() -> Instant,
    mut send_group_signal: impl FnMut(i32, i32) -> io::Result<()>,
) -> Result<()> {
    let Some(_) = remaining_phase_budget(deadline, now()) else {
        bail!("child process group {pid} SIGKILL confirmation deadline expired before revalidation")
    };
    match observe(state)
        .with_context(|| format!("failed to revalidate child process {pid} before SIGKILL"))?
    {
        ChildObservation::ExitedUnreaped(_) => {}
        ChildObservation::Running => {
            bail!("child process {pid} was still running before exited-group SIGKILL")
        }
        ChildObservation::ExitedReaped(_) => {
            bail!("child process {pid} was reaped before its process group could be re-signaled")
        }
    }
    let Some(_) = remaining_phase_budget(deadline, now()) else {
        bail!("child process group {pid} SIGKILL confirmation deadline expired before re-signaling")
    };

    let process_group = i32::try_from(pid)
        .ok()
        .filter(|process_group| *process_group > 0)
        .ok_or_else(|| anyhow!("child PID {pid} exceeds platform process-id range"))?;
    match send_group_signal(-process_group, libc::SIGKILL) {
        Ok(()) => Ok(()),
        // Neither ESRCH nor Darwin's zombie-only EPERM proves that the pinned
        // group is quiescent. The caller must still perform its platform proof.
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
        #[cfg(target_os = "macos")]
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!("failed to re-send SIGKILL to child process group {process_group}")
        }),
    }
}

#[cfg(unix)]
fn send_signal(pid: i32, signal: i32) -> io::Result<()> {
    if unsafe { libc::kill(pid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[cfg(unix)]
fn process_group_or_pid_alive(pid: u32) -> io::Result<bool> {
    let Some(pid) = unix_pid(pid) else {
        return Ok(false);
    };
    match probe_process_liveness(-pid) {
        Ok(true) => Ok(true),
        Ok(false) => probe_process_liveness(pid),
        Err(error) => Err(error),
    }
}
#[cfg(all(unix, test))]
pub(super) fn process_group_alive(pid: u32) -> io::Result<bool> {
    let Some(pid) = unix_pid(pid) else {
        return Ok(false);
    };
    probe_process_liveness(-pid)
}

#[cfg(target_os = "macos")]
fn macos_process_group_is_quiescent(child: &mut Child, pid: u32) -> io::Result<bool> {
    classify_macos_process_group_quiescence(observe_child(child)?, || {
        macos_process_group_contains_only_pinned_leader(pid)
    })
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn classify_macos_process_group_quiescence(
    observation: ChildObservation,
    snapshot: impl FnOnce() -> io::Result<bool>,
) -> io::Result<bool> {
    match observation {
        ChildObservation::Running => Ok(false),
        ChildObservation::ExitedUnreaped(_) => snapshot(),
        ChildObservation::ExitedReaped(_) => Err(io::Error::other(
            "child was reaped before its macOS process group could be confirmed",
        )),
    }
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_group_signal_eperm(quiescence: io::Result<bool>) -> io::Result<bool> {
    // The caller's boolean means that this pinned group still needs bounded
    // cleanup/confirmation, not that Darwin delivered the requested signal.
    // An additional member may be a transient zombie or a live unsignalable
    // process; neither is safe to accept as quiescent at this snapshot.
    quiescence.map(|quiescent| !quiescent)
}

#[cfg(target_os = "macos")]
fn macos_process_group_contains_only_pinned_leader(pid: u32) -> io::Result<bool> {
    let process_group = ProcessGroupId::new(checked_pid_io(pid)?).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "macOS process-group snapshot used a non-positive pinned leader",
        )
    })?;
    shared_macos_process_group_contains_only_pinned_leader(process_group).map_err(|error| {
        match error {
            MacosProcessGroupSnapshotError::BufferSize => {
                io::Error::other("macOS process-group snapshot buffer was too large")
            }
            MacosProcessGroupSnapshotError::List(error) => io::Error::new(
                error.kind(),
                format!(
                    "failed to atomically list macOS process group {} members: {error}",
                    process_group.as_raw()
                ),
            ),
            MacosProcessGroupSnapshotError::NegativeMemberCount => io::Error::new(
                io::ErrorKind::InvalidData,
                "macOS process-group snapshot returned a negative member count",
            ),
            MacosProcessGroupSnapshotError::UntrustedMemberCount(count) => io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "macOS process-group snapshot returned an untrusted member count of {count}"
                ),
            ),
            MacosProcessGroupSnapshotError::NonPositiveMember => io::Error::new(
                io::ErrorKind::InvalidData,
                "macOS process-group snapshot returned a non-positive member identifier",
            ),
            MacosProcessGroupSnapshotError::MissingPinnedLeader(_) => io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "macOS process-group snapshot did not contain pinned leader {}",
                    process_group.as_raw()
                ),
            ),
        }
    })
}

#[cfg(all(unix, target_os = "linux"))]
fn confirm_exited_process_group_not_live(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    confirm_exited_process_group_not_live_with(
        child,
        pid,
        deadline,
        2,
        resignal_pinned_exited_process_group,
        |_child, pid, deadline| linux_process_group_has_live_members(pid, deadline),
        Instant::now,
        thread::sleep,
    )
}

#[cfg(all(unix, target_os = "macos"))]
fn confirm_exited_process_group_not_live(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    confirm_exited_process_group_not_live_with(
        child,
        pid,
        deadline,
        1,
        resignal_pinned_exited_process_group,
        |child, pid, _deadline| {
            macos_process_group_is_quiescent(child, pid)
                .map(|quiescent| !quiescent)
                .map_err(Into::into)
        },
        Instant::now,
        thread::sleep,
    )
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn confirm_exited_process_group_not_live(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    // Other supported Unix targets do not expose a portable post-SIGKILL
    // membership proof. Revalidate the unreaped leader and re-signal once,
    // then retain the documented best-effort behavior on those platforms.
    resignal_pinned_exited_process_group(child, pid, deadline)
}

#[cfg(target_os = "linux")]
fn linux_process_group_has_live_members(process_group: u32, deadline: Instant) -> Result<bool> {
    let mut within_budget = || remaining_phase_budget(deadline, Instant::now()).is_some();
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    let entries = std::fs::read_dir("/proc");
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    let pids = collect_linux_process_ids_with(
        process_group,
        entries.context("failed to enumerate /proc")?,
        |entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
        },
        &mut within_budget,
    )?;
    linux_process_group_has_live_members_with(
        process_group,
        pids,
        |pid| std::fs::read_to_string(format!("/proc/{pid}/stat")),
        linux_process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_process_ids_with<T>(
    process_group: u32,
    mut entries: impl Iterator<Item = io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<u32>,
    mut within_budget: impl FnMut() -> bool,
) -> Result<Vec<u32>> {
    let mut pids = Vec::new();
    loop {
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let entry = entries.next();
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let Some(entry) = entry else {
            return Ok(pids);
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("failed to enumerate /proc entry"),
        };
        if let Some(pid) = process_id(entry) {
            pids.push(pid);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn ensure_linux_group_scan_budget(
    process_group: u32,
    within_budget: &mut impl FnMut() -> bool,
) -> Result<()> {
    if within_budget() {
        Ok(())
    } else {
        bail!("child process group {process_group} cleanup scan exceeded its deadline")
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_process_group_has_live_members_with(
    process_group: u32,
    pids: impl IntoIterator<Item = u32>,
    mut read_stat: impl FnMut(u32) -> io::Result<String>,
    mut process_group_for_pid: impl FnMut(u32) -> io::Result<Option<u32>>,
    mut within_budget: impl FnMut() -> bool,
) -> Result<bool> {
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    for pid in pids {
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let observation = read_stat(pid).and_then(parse_linux_process_stat);
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let observation = match observation {
            Ok(observation) => observation,
            Err(stat_error) => {
                ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
                let observed_group = process_group_for_pid(pid);
                ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
                match observed_group {
                    Ok(None) => continue,
                    Ok(Some(other_group)) if other_group != process_group => continue,
                    Ok(Some(_)) => {
                        return Err(stat_error).with_context(|| {
                            format!(
                                "could not inspect process {pid}, which belongs to owned process group {process_group}"
                            )
                        });
                    }
                    Err(group_error) => {
                        return Err(stat_error).with_context(|| {
                            format!(
                                "could not inspect process {pid} or prove it is outside owned process group {process_group}: {group_error}"
                            )
                        });
                    }
                }
            }
        };
        if observation.process_group == process_group && observation.live {
            ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
            return Ok(true);
        }
    }
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxProcessObservation {
    process_group: u32,
    live: bool,
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(stat: String) -> io::Result<LinuxProcessObservation> {
    let (_, fields) = stat
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing stat command field"))?;
    let mut fields = fields.split_whitespace();
    let state = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process state"))?;
    let process_group = fields
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process group"))?
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid process group"))?;
    Ok(LinuxProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
fn linux_process_group_for_pid(pid: u32) -> io::Result<Option<u32>> {
    let pid = checked_pid_io(pid)?;
    // SAFETY: `pid` is a positive, representable process identifier. `getpgid`
    // only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return u32::try_from(process_group)
            .map(Some)
            .map_err(|_| io::Error::other("process group is not representable"));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
}
#[cfg(unix)]
fn probe_process_liveness(pid: i32) -> io::Result<bool> {
    let result = unsafe { libc::kill(pid, 0) };
    classify_liveness_probe(result, io::Error::last_os_error)
}
#[cfg(unix)]
fn classify_liveness_probe(result: i32, error: impl FnOnce() -> io::Error) -> io::Result<bool> {
    if result == 0 {
        return Ok(true);
    }
    let error = error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        // EPERM and unexpected probe failures are not evidence of absence.
        Err(error)
    }
}
#[cfg(not(unix))]
pub(super) fn terminate_pid(pid: u32) -> Result<()> {
    bail!("terminating child process {pid} is unsupported on this platform")
}
#[cfg(not(unix))]
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    terminate_pid(pid)
}

#[cfg(test)]
mod tests;
