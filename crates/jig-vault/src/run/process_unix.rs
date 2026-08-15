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
use jig_owned_process::unix::{
    ConsecutiveQuiescence, ProcessGroupId, UnreapedChildObservation, WaitidClassificationError,
    classify_waitid_status, waitid_without_reaping,
};
#[cfg(target_os = "macos")]
use jig_owned_process::unix::{
    MacosProcessGroupSnapshotError,
    macos_process_group_contains_only_pinned_leader as shared_macos_process_group_contains_only_pinned_leader,
};

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
    let Ok(process_group) = ProcessGroupId::new(process_group) else {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(io::Error::other(
            "brokered process identifier was not positive",
        ));
    };
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
    let status = match waitid_without_reaping(process_group.id) {
        Ok(status) => status,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            return Ok(LeaderObservation::Running);
        }
        Err(error) => {
            update_unix_process_group_after_wait_error(&mut process.process_group, &error);
            return Err(error);
        }
    };
    classify_waitid_status(process_group.id, status)
        .map(|observation| match observation {
            UnreapedChildObservation::Running => LeaderObservation::Running,
            UnreapedChildObservation::Exited(_) => LeaderObservation::Exited,
        })
        .map_err(brokered_waitid_classification_error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn brokered_waitid_classification_error(error: WaitidClassificationError) -> io::Error {
    match error {
        WaitidClassificationError::UnexpectedPid {
            observed: observed_pid,
            ..
        } => io::Error::other(format!(
            "waitid observed unexpected brokered child PID {observed_pid}"
        )),
        WaitidClassificationError::UnexpectedCode(code) => io::Error::new(
            io::ErrorKind::InvalidData,
            format!("waitid returned unrecognized brokered child state code {code}"),
        ),
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
        |process| process.process_group.map(|group| group.id.as_raw()),
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
        |process| process.process_group.map(|group| group.id.as_raw()),
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
    let signal_result = match signal_pinned_unix_process_group(process, group.id.as_raw(), deadline)
    {
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
                        confirm_unix_process_group_quiescent(process, group.id.as_raw(), deadline)
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
            if let Err(direct_error) =
                kill_pinned_unix_process_leader(process, group.id.as_raw(), deadline)
            {
                bail!(
                    "process-group SIGKILL failed: {group_error}; direct child SIGKILL also failed: {direct_error}"
                );
            }
            Some(group_error)
        }
    };
    let confirmation = confirm_unix_process_group_quiescent(process, group.id.as_raw(), deadline);
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
    let mut quiescence = ConsecutiveQuiescence::new(required_quiescent_proofs)
        .map_err(|_| anyhow!("brokered process-group confirmation requires at least one proof"))?;

    let timeout_error = || {
        anyhow!(
            "brokered process group {process_group} retained additional or unverified members through its cleanup deadline"
        )
    };
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
        if quiescence.observe(quiescent) {
            return Ok(());
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
                    if group.id.as_raw() == expected_group {
                        Ok(())
                    } else {
                        Err(io::Error::other(format!(
                            "brokered process-group identity changed from {expected_group} to {}",
                            group.id.as_raw()
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
    let process_group = ProcessGroupId::new(process_group)
        .map_err(|_| anyhow!("macOS process-group snapshot used a non-positive pinned leader"))?;
    shared_macos_process_group_contains_only_pinned_leader(process_group).map_err(|error| {
        match error {
            MacosProcessGroupSnapshotError::BufferSize => {
                anyhow!("macOS process-group snapshot buffer size was not representable")
            }
            MacosProcessGroupSnapshotError::List(error) => anyhow!(
                "failed to atomically list brokered process group {} members: {error}",
                process_group.as_raw()
            ),
            MacosProcessGroupSnapshotError::NegativeMemberCount => {
                anyhow!("macOS process-group snapshot returned a negative member count")
            }
            MacosProcessGroupSnapshotError::UntrustedMemberCount(count) => anyhow!(
                "macOS process-group snapshot returned an untrusted member count of {count}"
            ),
            MacosProcessGroupSnapshotError::NonPositiveMember => {
                anyhow!("macOS process-group snapshot returned a non-positive member identifier")
            }
            MacosProcessGroupSnapshotError::MissingPinnedLeader(_) => anyhow!(
                "macOS process-group snapshot did not contain exactly one pinned leader {}",
                process_group.as_raw()
            ),
        }
    })
}
