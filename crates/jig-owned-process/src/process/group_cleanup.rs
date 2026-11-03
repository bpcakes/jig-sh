//! Signals an owned process group and, for confirmed cleanup, proves it is empty.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;
use std::time::Instant;

#[cfg(target_os = "linux")]
use crate::unix::linux_process_group_has_live_members;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::unix::{
    ConsecutiveQuiescence, ProcessGroupId, UnreapedChildObservation, WaitidClassificationError,
    classify_waitid_status, waitid_without_reaping,
};
#[cfg(target_os = "macos")]
use crate::unix::{
    MacosProcessGroupSnapshotError,
    macos_process_group_contains_only_pinned_leader as shared_macos_process_group_contains_only_pinned_leader,
};

use super::OwnedProcess;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::{
    OwnedProcessObservation, PinnedProcessGroup, update_owned_process_identity_after_wait_error,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const OWNED_PROCESS_TREE_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn observe_owned_process(
    process: &mut OwnedProcess,
) -> std::io::Result<OwnedProcessObservation> {
    let process_group = process
        .process_group
        .ok_or_else(|| std::io::Error::other("owned process-group identity is no longer pinned"))?;
    let status = match waitid_without_reaping(process_group.id) {
        Ok(status) => status,
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            return Ok(OwnedProcessObservation::Running);
        }
        Err(error) => {
            update_owned_process_identity_after_wait_error(process, &error);
            return Err(error);
        }
    };
    classify_waitid_status(process_group.id, status)
        .map(|observation| match observation {
            UnreapedChildObservation::Running => OwnedProcessObservation::Running,
            UnreapedChildObservation::Exited(_) => OwnedProcessObservation::Exited,
        })
        .map_err(owned_process_waitid_classification_error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_process_waitid_classification_error(error: WaitidClassificationError) -> std::io::Error {
    match error {
        WaitidClassificationError::UnexpectedPid {
            expected: expected_pid,
            observed: observed_pid,
        } => std::io::Error::other(format!(
            "waitid observed unexpected owned child PID {observed_pid} instead of {expected_pid}"
        )),
        WaitidClassificationError::UnexpectedCode(code) => std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("waitid returned an unrecognized owned child state code {code}"),
        ),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn terminate_owned_process_tree(
    process: &mut OwnedProcess,
    deadline: Instant,
) -> std::io::Result<()> {
    ensure_owned_process_cleanup_budget(deadline, "before process-group termination")?;
    let process_group = process.process_group.ok_or_else(|| {
        std::io::Error::other(
            "owned process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    confirm_process_group_quiescent(process, process_group.id.as_raw(), deadline)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn signal_owned_process_tree_once(
    process: &mut OwnedProcess,
    deadline: Instant,
) -> std::io::Result<()> {
    ensure_owned_process_cleanup_budget(deadline, "before process-group termination")?;
    let process_group = process.process_group.ok_or_else(|| {
        std::io::Error::other(
            "owned process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    // The unreaped direct child still pins the group, so this one signal
    // reaches every current member. Membership is not proven afterwards.
    signal_pinned_process_group(process, process_group.id.as_raw(), deadline).map(|_| ())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessGroupSignalResult {
    Delivered,
    Inconclusive,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pinned_process_group_for_retry(
    process: &OwnedProcess,
    expected_process_group: libc::pid_t,
) -> std::io::Result<PinnedProcessGroup> {
    let process_group = process.process_group.ok_or_else(|| {
        std::io::Error::other(
            "owned process-group identity is no longer pinned; refusing to signal it",
        )
    })?;
    if process_group.id.as_raw() != expected_process_group {
        return Err(std::io::Error::other(format!(
            "owned process-group identity changed from pinned group {expected_process_group} to {}",
            process_group.id.as_raw()
        )));
    }
    Ok(process_group)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn signal_owned_process_group_with<T>(
    state: &mut T,
    mut observe: impl FnMut(&mut T) -> std::io::Result<OwnedProcessObservation>,
    signal: impl FnOnce(&mut T) -> std::io::Result<()>,
) -> std::io::Result<ProcessGroupSignalResult> {
    observe(state)?;
    match signal(state) {
        Ok(()) => Ok(ProcessGroupSignalResult::Delivered),
        // ESRCH is not proof of absence; platform confirmation must still run.
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
            Ok(ProcessGroupSignalResult::Inconclusive)
        }
        #[cfg(target_os = "macos")]
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            // Darwin can return EPERM while the child is exiting, before
            // waitid exposes its terminal status. Recheck that the child is
            // still ours and let the bounded confirmation loop retry.
            resolve_macos_process_group_signal_eperm(error, observe(state))
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn signal_pinned_process_group(
    process: &mut OwnedProcess,
    expected_process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<ProcessGroupSignalResult> {
    ensure_owned_process_cleanup_budget(deadline, "before process-group SIGKILL")?;
    pinned_process_group_for_retry(process, expected_process_group)?;
    signal_owned_process_group_with(process, observe_owned_process, |process| {
        // The exact WNOWAIT observation above must precede every numeric
        // group signal. If another waiter consumed the status, ECHILD has
        // already cleared the cached identity and this closure is never
        // entered.
        ensure_owned_process_cleanup_budget(deadline, "after pre-signal leader observation")?;
        let process_group = pinned_process_group_for_retry(process, expected_process_group)?;
        // SAFETY: the positive group identifier was revalidated after a
        // fresh non-consuming observation of our direct child. Its
        // unconsumed wait status pins this exact process-group generation.
        if unsafe { libc::kill(-process_group.id.as_raw(), libc::SIGKILL) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    })
}

#[cfg(target_os = "macos")]
pub(super) fn resolve_macos_process_group_signal_eperm(
    signal_error: std::io::Error,
    leader_observation: std::io::Result<OwnedProcessObservation>,
) -> std::io::Result<ProcessGroupSignalResult> {
    match leader_observation {
        Ok(OwnedProcessObservation::Exited | OwnedProcessObservation::Running) => {
            // EPERM is not absence, and a running observation can precede
            // waitid's terminal status during exit. Keep the pinned identity
            // and retry within the original deadline. Confirmation still
            // requires a fresh terminal observation and sole-leader snapshot.
            Ok(ProcessGroupSignalResult::Inconclusive)
        }
        Err(observation_error) => Err(std::io::Error::new(
            observation_error.kind(),
            format!(
                "process-group SIGKILL failed: {signal_error}; failed to verify the pinned leader after EPERM: {observation_error}"
            ),
        )),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn confirm_process_group_quiescent_with<T>(
    state: &mut T,
    process_group: libc::pid_t,
    deadline: Instant,
    required_consecutive_proofs: u8,
    timeout_phase: &str,
    mut signal: impl FnMut(&mut T, libc::pid_t, Instant) -> std::io::Result<ProcessGroupSignalResult>,
    mut prove_quiescent: impl FnMut(&mut T, libc::pid_t, Instant) -> std::io::Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> std::io::Result<()> {
    let mut quiescence = ConsecutiveQuiescence::new(required_consecutive_proofs).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "owned process-group confirmation requires at least one proof",
        )
    })?;
    loop {
        owned_process_cleanup_remaining_at(deadline, now(), timeout_phase)?;
        // Signal before every proof. A descendant can become visible in this
        // pinned group after an earlier group signal, so polling alone cannot
        // make a prior SIGKILL authoritative for a later membership snapshot.
        let _signal_result = signal(state, process_group, deadline)?;
        owned_process_cleanup_remaining_at(deadline, now(), "after process-group SIGKILL")?;
        let quiescent = prove_quiescent(state, process_group, deadline)?;
        // Never accept a proof that completed outside the original absolute
        // cleanup budget.
        owned_process_cleanup_remaining_at(deadline, now(), "after process-group confirmation")?;
        if quiescence.observe(quiescent) {
            return Ok(());
        }

        let remaining = owned_process_cleanup_remaining_at(deadline, now(), timeout_phase)?;
        sleep(remaining.min(OWNED_PROCESS_TREE_POLL_INTERVAL));
    }
}

#[cfg(target_os = "linux")]
fn confirm_process_group_quiescent(
    process: &mut OwnedProcess,
    process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<()> {
    confirm_process_group_quiescent_with(
        process,
        process_group,
        deadline,
        2,
        "while confirming the Linux process group",
        signal_pinned_process_group,
        |_process, process_group, deadline| {
            linux_process_group_has_live_members(ProcessGroupId::new(process_group)?, deadline)
                .map(|live| !live)
        },
        Instant::now,
        std::thread::sleep,
    )
}

#[cfg(target_os = "macos")]
fn confirm_process_group_quiescent(
    process: &mut OwnedProcess,
    process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<()> {
    confirm_process_group_quiescent_with(
        process,
        process_group,
        deadline,
        1,
        "while confirming the macOS process group",
        signal_pinned_process_group,
        |process, process_group, deadline| {
            pinned_process_group_for_retry(process, process_group)?;
            let leader_exited = observe_owned_process(process)? == OwnedProcessObservation::Exited;
            ensure_owned_process_cleanup_budget(deadline, "after macOS leader observation")?;
            if !leader_exited {
                return Ok(false);
            }
            let sole_pinned_leader =
                macos_process_group_contains_only_pinned_leader(process_group)?;
            ensure_owned_process_cleanup_budget(deadline, "after macOS process-group snapshot")?;
            Ok(sole_pinned_leader)
        },
        Instant::now,
        std::thread::sleep,
    )
}

#[cfg(target_os = "macos")]
fn macos_process_group_contains_only_pinned_leader(
    process_group: libc::pid_t,
) -> std::io::Result<bool> {
    let process_group = ProcessGroupId::new(process_group).map_err(|_| {
        std::io::Error::other("macOS process-group snapshot used a non-positive pinned leader")
    })?;
    shared_macos_process_group_contains_only_pinned_leader(process_group).map_err(|error| {
        match error {
            MacosProcessGroupSnapshotError::BufferSize => {
                std::io::Error::other("macOS process-group snapshot buffer was not representable")
            }
            MacosProcessGroupSnapshotError::List(error) => std::io::Error::other(format!(
                "failed to atomically list owned process group {}: {error}",
                process_group.as_raw()
            )),
            MacosProcessGroupSnapshotError::NegativeMemberCount => std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "macOS process-group snapshot returned a negative member count",
            ),
            MacosProcessGroupSnapshotError::UntrustedMemberCount(count) => std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "macOS process-group snapshot returned an untrusted member count of {count}"
                ),
            ),
            MacosProcessGroupSnapshotError::NonPositiveMember => std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "macOS process-group snapshot returned a non-positive member identifier",
            ),
            MacosProcessGroupSnapshotError::MissingPinnedLeader(_) => std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "macOS process-group snapshot did not contain the exact pinned leader {}",
                    process_group.as_raw()
                ),
            ),
        }
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ensure_owned_process_cleanup_budget(deadline: Instant, phase: &str) -> std::io::Result<()> {
    owned_process_cleanup_remaining_at(deadline, Instant::now(), phase).map(|_| ())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_process_cleanup_remaining_at(
    deadline: Instant,
    now: Instant,
    phase: &str,
) -> std::io::Result<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| owned_process_cleanup_timeout(phase))
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn owned_process_cleanup_timeout(phase: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("owned process-tree cleanup timed out {phase}"),
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn signal_owned_process_tree_once(
    process: &mut OwnedProcess,
    deadline: Instant,
) -> std::io::Result<()> {
    terminate_owned_process_tree(process, deadline)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn terminate_owned_process_tree(
    _process: &mut OwnedProcess,
    _deadline: Instant,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}
