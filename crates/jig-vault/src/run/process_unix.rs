#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::process::CommandExt;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::thread;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::{Duration, Instant};

use anyhow::{Result as AnyResult, anyhow, bail};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::process::{
    BrokeredProcess, LeaderObservation, PinnedUnixProcessGroup, terminate_spawn_failure_child,
};
#[cfg(target_os = "linux")]
use super::process_linux::linux_process_group_has_live_members;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::{BROKERED_PROCESS_CLEANUP_TIMEOUT, BROKERED_PROCESS_POLL_INTERVAL};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn spawn_brokered_process(command: &mut Command) -> io::Result<BrokeredProcess> {
    unsafe {
        // SAFETY: pre_exec runs after fork and before exec. The closure only
        // calls the async-signal-safe setsid syscall and reads errno on error.
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    let Ok(process_group) = libc::pid_t::try_from(child.id()) else {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(io::Error::other(
            "brokered process identifier is not representable",
        ));
    };
    if process_group <= 0 {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(io::Error::other(
            "brokered process identifier was not positive",
        ));
    }
    Ok(BrokeredProcess {
        child,
        process_group: Some(PinnedUnixProcessGroup { id: process_group }),
        reaped_status: None,
        cleanup_deadline: None,
        tree_cleanup_error: None,
        cleanup_complete: false,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn observe_brokered_leader(
    process: &mut BrokeredProcess,
) -> io::Result<LeaderObservation> {
    let Some(process_group) = process.process_group.as_mut() else {
        return if process.reaped_status.is_some() {
            Ok(LeaderObservation::Exited)
        } else {
            Err(io::Error::other(
                "brokered process-group identity is no longer pinned",
            ))
        };
    };
    let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: information is writable siginfo_t storage, id names our direct
    // child, and WNOWAIT preserves the wait status so that child continues to
    // pin this process-group generation until cleanup.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_group.id as _,
            information.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == 0 {
        // SAFETY: successful waitid initialized information.
        let information = unsafe { information.assume_init() };
        // SAFETY: waitid populated the SIGCHLD union member.
        let observed_pid = unsafe { information.si_pid() };
        if observed_pid == 0 {
            return Ok(LeaderObservation::Running);
        }
        return classify_waitid_leader_observation(
            process_group.id,
            observed_pid,
            information.si_code,
        );
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
        return Ok(LeaderObservation::Running);
    }
    update_unix_process_group_after_wait_error(&mut process.process_group, &error);
    Err(error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn classify_waitid_leader_observation(
    expected_pid: libc::pid_t,
    observed_pid: libc::pid_t,
    code: libc::c_int,
) -> io::Result<LeaderObservation> {
    if observed_pid == 0 {
        return Ok(LeaderObservation::Running);
    }
    if observed_pid != expected_pid {
        return Err(io::Error::other(format!(
            "waitid observed unexpected brokered child PID {observed_pid}"
        )));
    }
    match code {
        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED => Ok(LeaderObservation::Exited),
        libc::CLD_STOPPED | libc::CLD_TRAPPED | libc::CLD_CONTINUED => {
            Ok(LeaderObservation::Running)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("waitid returned unrecognized brokered child state code {code}"),
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn update_unix_process_group_after_wait_error(
    process_group: &mut Option<PinnedUnixProcessGroup>,
    error: &io::Error,
) {
    if error.raw_os_error() == Some(libc::ECHILD) {
        // ECHILD proves the wait status no longer pins this numeric identity.
        // Clear it before any cleanup path can issue a negative-PID signal.
        *process_group = None;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn with_pinned_unix_process_group<T>(
    process_group: Option<&PinnedUnixProcessGroup>,
    action: impl FnOnce(PinnedUnixProcessGroup) -> io::Result<T>,
) -> io::Result<T> {
    let process_group = process_group.ok_or_else(|| {
        io::Error::other(
            "brokered process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    action(*process_group)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn signal_pinned_unix_process_group(
    process: &mut BrokeredProcess,
    expected_group: libc::pid_t,
    deadline: Instant,
) -> io::Result<()> {
    signal_pinned_unix_process_group_with(
        process,
        expected_group,
        deadline,
        |process| process.process_group.map(|group| group.id),
        BrokeredProcess::observe_leader,
        Instant::now,
        |_, process_group| {
            // SAFETY: the retained exact-child wait status was freshly
            // observed and revalidated immediately before this call.
            if unsafe { libc::kill(-process_group, libc::SIGKILL) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        },
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn kill_pinned_unix_process_leader(
    process: &mut BrokeredProcess,
    expected_group: libc::pid_t,
    deadline: Instant,
) -> io::Result<()> {
    signal_pinned_unix_process_group_with(
        process,
        expected_group,
        deadline,
        |process| process.process_group.map(|group| group.id),
        BrokeredProcess::observe_leader,
        Instant::now,
        |process, _| process.child.kill(),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn signal_pinned_unix_process_group_with<T>(
    state: &mut T,
    expected_group: libc::pid_t,
    deadline: Instant,
    mut current_group: impl FnMut(&T) -> Option<libc::pid_t>,
    mut observe_leader: impl FnMut(&mut T) -> io::Result<LeaderObservation>,
    mut now: impl FnMut() -> Instant,
    mut signal_group: impl FnMut(&mut T, libc::pid_t) -> io::Result<()>,
) -> io::Result<()> {
    let validate_group = |current_group: Option<libc::pid_t>| {
        if expected_group <= 0 {
            return Err(io::Error::other(
                "brokered process-group identity was not positive",
            ));
        }
        let current_group = current_group.ok_or_else(|| {
            io::Error::other(
                "brokered process-group identity is no longer pinned; refusing to signal it",
            )
        })?;
        if current_group <= 0 {
            return Err(io::Error::other(
                "brokered process-group identity was not positive",
            ));
        }
        if current_group != expected_group {
            return Err(io::Error::other(format!(
                "brokered process-group identity changed from {expected_group} to {current_group}"
            )));
        }
        Ok(())
    };

    validate_group(current_group(state))?;
    // WNOWAIT keeps the exact child unreaped. ECHILD clears the cached group
    // inside the production observer and must stop before any numeric signal.
    let _ = observe_leader(state)?;
    validate_group(current_group(state))?;
    if deadline
        .checked_duration_since(now())
        .is_none_or(|remaining| remaining.is_zero())
    {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "brokered process group {expected_group} signal observation exceeded its cleanup deadline"
            ),
        ));
    }
    signal_group(state, expected_group)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn terminate_brokered_process_tree(
    process: &mut BrokeredProcess,
    deadline: Instant,
) -> AnyResult<()> {
    let group = with_pinned_unix_process_group(process.process_group.as_ref(), Ok)?;
    let signal_result = match signal_pinned_unix_process_group(process, group.id, deadline) {
        Ok(()) => Ok(()),
        Err(error) => {
            if error.raw_os_error() == Some(libc::ESRCH) {
                // ESRCH is only an inconclusive signal result. The retained leader
                // still pins this generation, and confirmation below re-signals
                // before every independent membership proof.
                Ok(())
            } else {
                #[cfg(target_vendor = "apple")]
                let error = if error.raw_os_error() == Some(libc::EPERM) {
                    // Darwin reports EPERM for a group containing only its zombie
                    // leader, but also when any live member is not signalable. The
                    // exact-PID WNOWAIT observation pins the group; the atomic
                    // membership proof below must still establish that the exited
                    // leader is its sole remaining member. Preserve EPERM whenever
                    // either observation or confirmation fails.
                    let observation = process.observe_leader();
                    match resolve_macos_group_signal_eperm(error, observation, || {
                        confirm_unix_process_group_quiescent(process, group.id, deadline)
                    })? {
                        None => return Ok(()),
                        Some(error) => error,
                    }
                } else {
                    error
                };
                Err(error)
            }
        }
    };
    let signal_error = match signal_result {
        Ok(()) => None,
        Err(group_error) => {
            // A direct kill is safe only while the same unconsumed wait status
            // still pins the PID. It cannot establish descendant cleanup, so
            // retain the group error as context unless the platform-specific
            // confirmation subsequently proves the whole group quiescent.
            if let Err(direct_error) = kill_pinned_unix_process_leader(process, group.id, deadline)
            {
                bail!(
                    "process-group SIGKILL failed: {group_error}; direct child SIGKILL also failed: {direct_error}"
                );
            }
            Some(group_error)
        }
    };
    let confirmation = confirm_unix_process_group_quiescent(process, group.id, deadline);
    finish_unix_process_group_termination(signal_error, confirmation)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn finish_unix_process_group_termination(
    signal_error: Option<io::Error>,
    confirmation: AnyResult<()>,
) -> AnyResult<()> {
    match (signal_error, confirmation) {
        (_, Ok(())) => Ok(()),
        (None, result) => result,
        (Some(signal_error), Err(confirmation_error)) => Err(anyhow!(
            "process-group SIGKILL failed: {signal_error}; group confirmation also failed: {confirmation_error:#}"
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn confirm_unix_process_group_quiescent_with<T>(
    state: &mut T,
    process_group: libc::pid_t,
    deadline: Instant,
    required_quiescent_proofs: u8,
    mut signal_group: impl FnMut(&mut T, libc::pid_t, Instant) -> io::Result<()>,
    mut prove_quiescent: impl FnMut(&mut T, libc::pid_t, Instant) -> AnyResult<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> AnyResult<()> {
    if required_quiescent_proofs == 0 {
        bail!("brokered process-group confirmation requires at least one proof");
    }

    let timeout_error = || {
        anyhow!(
            "brokered process group {process_group} retained additional or unverified members through its cleanup deadline"
        )
    };
    let mut consecutive_quiescent_proofs = 0_u8;
    let mut retained_signal_error = None;
    loop {
        if deadline
            .checked_duration_since(now())
            .is_none_or(|remaining| remaining.is_zero())
        {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        }

        // A member can fork or join this still-pinned group after an earlier
        // group signal. Re-signal immediately before every proof; on Linux this
        // also places a fresh SIGKILL between the two required empty scans.
        if let Err(error) = signal_group(state, process_group, deadline) {
            // ESRCH and Darwin EPERM are inconclusive rather than absence. All
            // other errors are retained too: an independent proof may still
            // establish quiescence, while a failed proof must preserve the
            // earliest signal failure as its primary context.
            retained_signal_error.get_or_insert(error);
        }
        if deadline
            .checked_duration_since(now())
            .is_none_or(|remaining| remaining.is_zero())
        {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        }

        let quiescent = match prove_quiescent(state, process_group, deadline) {
            Ok(quiescent) => quiescent,
            Err(error) => {
                return finish_unix_process_group_termination(retained_signal_error, Err(error));
            }
        };
        if deadline
            .checked_duration_since(now())
            .is_none_or(|remaining| remaining.is_zero())
        {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        }
        if quiescent {
            consecutive_quiescent_proofs += 1;
            if consecutive_quiescent_proofs == required_quiescent_proofs {
                return Ok(());
            }
        } else {
            consecutive_quiescent_proofs = 0;
        }

        let Some(remaining) = deadline.checked_duration_since(now()) else {
            return finish_unix_process_group_termination(
                retained_signal_error,
                Err(timeout_error()),
            );
        };
        sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    }
}

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
pub(super) fn resolve_macos_group_signal_eperm(
    signal_error: io::Error,
    observation: io::Result<LeaderObservation>,
    confirm_quiescence: impl FnOnce() -> AnyResult<()>,
) -> AnyResult<Option<io::Error>> {
    match observation {
        Ok(LeaderObservation::Running) => Ok(Some(signal_error)),
        Ok(LeaderObservation::Exited) => {
            finish_unix_process_group_termination(Some(signal_error), confirm_quiescence())?;
            Ok(None)
        }
        Err(observation_error) => finish_unix_process_group_termination(
            Some(signal_error),
            Err(anyhow!(observation_error).context("failed to observe brokered process leader")),
        )
        .map(|()| None),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn confirm_unix_process_group_quiescent(
    process: &mut BrokeredProcess,
    process_group: libc::pid_t,
    deadline: Instant,
) -> AnyResult<()> {
    #[cfg(target_os = "linux")]
    {
        confirm_unix_process_group_quiescent_with(
            process,
            process_group,
            deadline,
            2,
            signal_pinned_unix_process_group,
            |process, expected_group, deadline| {
                with_pinned_unix_process_group(process.process_group.as_ref(), |group| {
                    if group.id == expected_group {
                        Ok(())
                    } else {
                        Err(io::Error::other(format!(
                            "brokered process-group identity changed from {expected_group} to {}",
                            group.id
                        )))
                    }
                })?;
                linux_process_group_has_live_members(expected_group, deadline)
                    .map(|live_members| !live_members)
            },
            Instant::now,
            thread::sleep,
        )
    }
    #[cfg(target_vendor = "apple")]
    {
        confirm_unix_process_group_quiescent_with(
            process,
            process_group,
            deadline,
            1,
            signal_pinned_unix_process_group,
            |process, expected_group, deadline| {
                let leader_exited = process.observe_leader()? == LeaderObservation::Exited;
                if deadline.checked_duration_since(Instant::now()).is_none() {
                    bail!(
                        "brokered process group {expected_group} leader observation exceeded its cleanup deadline"
                    );
                }
                if leader_exited {
                    macos_process_group_contains_only_pinned_leader(expected_group)
                } else {
                    Ok(false)
                }
            },
            Instant::now,
            thread::sleep,
        )
    }
}

#[cfg(target_os = "macos")]
pub(super) fn macos_process_group_contains_only_pinned_leader(
    process_group: libc::pid_t,
) -> AnyResult<bool> {
    let mut members = [0 as libc::pid_t; 2];
    let buffer_size = i32::try_from(std::mem::size_of_val(&members))
        .map_err(|_| anyhow!("macOS process-group snapshot buffer size was not representable"))?;
    // SAFETY: members is writable storage for exactly two pid_t values and the
    // byte count describes that complete live buffer. libproc returns a PID
    // count; a count of two may mean two or more members because the kernel
    // deliberately caps collection at the supplied capacity.
    let count =
        unsafe { libc::proc_listpgrppids(process_group, members.as_mut_ptr().cast(), buffer_size) };
    if count <= 0 {
        let error = io::Error::last_os_error();
        bail!("failed to atomically list brokered process group {process_group} members: {error}");
    }
    classify_macos_process_group_snapshot(process_group, count, members)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn classify_macos_process_group_snapshot(
    process_group: i32,
    count: i32,
    members: [i32; 2],
) -> AnyResult<bool> {
    if process_group <= 0 {
        bail!("macOS process-group snapshot used a non-positive pinned leader");
    }
    let count = usize::try_from(count)
        .map_err(|_| anyhow!("macOS process-group snapshot returned a negative member count"))?;
    if count == 0 || count > members.len() {
        bail!("macOS process-group snapshot returned an untrusted member count of {count}");
    }
    let observed = &members[..count];
    if observed.iter().any(|pid| *pid <= 0) {
        bail!("macOS process-group snapshot returned a non-positive member identifier");
    }
    if count == members.len() {
        // The kernel scans live allproc entries before zombies and caps its
        // output at this buffer. Two positive PIDs therefore means "at least
        // two members"; the pinned zombie need not be among the returned pair.
        return Ok(false);
    }
    let leader_count = observed.iter().filter(|pid| **pid == process_group).count();
    if leader_count != 1 {
        bail!(
            "macOS process-group snapshot did not contain exactly one pinned leader {process_group}"
        );
    }
    Ok(count == 1)
}
