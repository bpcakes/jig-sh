use std::io;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use jig_owned_process::unix::ProcessGroupId;

use crate::SecretBytes;

use super::output::CappedOutputDrains;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::process_unix::{
    observe_brokered_leader, spawn_brokered_process, terminate_brokered_process_tree,
};
#[cfg(windows)]
use super::process_windows::{
    observe_brokered_leader, spawn_brokered_process, terminate_brokered_process_tree,
};
use super::{
    ACTIVE_OUTPUT_POLL_INTERVAL, BROKERED_OUTPUT_DRAIN_TIMEOUT, BROKERED_PROCESS_CLEANUP_TIMEOUT,
    BROKERED_PROCESS_POLL_INTERVAL, checked_deadline,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LeaderObservation {
    Running,
    Exited,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug)]
pub(super) struct PinnedUnixProcessGroup {
    pub(super) id: ProcessGroupId,
}

pub(super) struct BrokeredProcess {
    pub(super) child: Child,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) process_group: Option<PinnedUnixProcessGroup>,
    #[cfg(windows)]
    pub(super) job: std::os::windows::io::OwnedHandle,
    pub(super) reaped_status: Option<ExitStatus>,
    pub(super) cleanup_deadline: Option<Instant>,
    pub(super) tree_cleanup_error: Option<String>,
    pub(super) cleanup_complete: bool,
}

impl BrokeredProcess {
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        spawn_brokered_process(command)
    }

    pub(super) fn observe_leader(&mut self) -> io::Result<LeaderObservation> {
        observe_brokered_leader(self)
    }

    pub(super) fn terminate_and_reap(&mut self, timeout: Duration) -> AnyResult<ExitStatus> {
        if self.cleanup_complete {
            return self
                .reaped_status
                .ok_or_else(|| anyhow!("brokered process cleanup lost its exit status"));
        }
        let (deadline, first_attempt) =
            fixed_process_cleanup_deadline(&mut self.cleanup_deadline, timeout)?;
        if first_attempt {
            if let Err(error) = terminate_brokered_process_tree(self, deadline) {
                self.tree_cleanup_error = Some(format!("{error:#}"));
            }
        }
        // The owned tree signal has already been attempted while Unix still
        // has a pinned wait status. It is now safe to consume that status;
        // importantly, no later path may signal the numeric group again.
        let reap = self.reap_after_tree_signal(deadline, timeout);
        let status = match reap {
            Ok(status) => status,
            Err(reap_error) => {
                return match &self.tree_cleanup_error {
                    None => Err(reap_error),
                    Some(tree_error) => Err(anyhow!(
                        "failed to terminate brokered process tree: {tree_error}; process reap also failed: {reap_error:#}"
                    )),
                };
            }
        };
        if let Some(error) = &self.tree_cleanup_error {
            bail!("failed to terminate brokered process tree: {error}");
        }
        self.cleanup_complete = true;
        Ok(status)
    }

    pub(super) fn reap_after_tree_signal(
        &mut self,
        deadline: Instant,
        timeout: Duration,
    ) -> AnyResult<ExitStatus> {
        if let Some(status) = self.reaped_status {
            return Ok(status);
        }
        loop {
            let result = self.child.try_wait();
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if matches!(result, Ok(Some(_)))
                || result
                    .as_ref()
                    .is_err_and(|error| error.raw_os_error() == Some(libc::ECHILD))
            {
                // Some consumes the wait status. ECHILD proves somebody else
                // did. Either way the numeric group identity is recyclable.
                self.process_group = None;
            }
            match result {
                Ok(Some(status)) => {
                    self.reaped_status = Some(status);
                    return Ok(status);
                }
                Ok(None) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to reap brokered command process {}",
                            self.child.id()
                        )
                    });
                }
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                bail!("brokered process cleanup exceeded its {timeout:?} deadline");
            };
            thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
        }
    }
}

impl Drop for BrokeredProcess {
    fn drop(&mut self) {
        let _ = self.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        // On Windows the retained kill-on-close Job Object is the final RAII
        // backstop if explicit termination or confirmation failed.
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn spawn_brokered_process(_command: &mut Command) -> io::Result<BrokeredProcess> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "owned brokered process trees are unavailable on this platform",
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn observe_brokered_leader(
    _process: &mut BrokeredProcess,
) -> io::Result<LeaderObservation> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "non-reaping brokered process observation is unavailable on this platform",
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn terminate_brokered_process_tree(
    _process: &mut BrokeredProcess,
    _deadline: Instant,
) -> AnyResult<()> {
    bail!("owned brokered process-tree cleanup is unavailable on this platform")
}

pub(super) fn terminate_spawn_failure_child(child: &mut Child, deadline: Option<Instant>) {
    let _ = child.kill();
    let Some(deadline) = deadline else {
        return;
    };
    loop {
        match child.try_wait() {
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Ok(Some(_)) | Err(_) => return,
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    }
}

pub(super) fn wait_for_capped_output(
    mut process: BrokeredProcess,
    command_name: &str,
    timeout: Duration,
) -> AnyResult<(PortableRunStatus, SecretBytes, SecretBytes)> {
    let mut drains = match CappedOutputDrains::start(&mut process.child) {
        Ok(drains) => drains,
        Err(primary) => {
            let cleanup = process.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
            return Err(append_secondary_error(
                primary,
                "process cleanup also failed",
                cleanup.err(),
            ));
        }
    };
    let deadline = match checked_deadline("brokered command run", timeout) {
        Ok(deadline) => deadline,
        Err(primary) => {
            let cleanup = process.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
            let drain = drains.finish(BROKERED_OUTPUT_DRAIN_TIMEOUT);
            let primary =
                append_secondary_error(primary, "process cleanup also failed", cleanup.err());
            return Err(append_secondary_error(
                primary,
                "output drain also failed",
                drain.err(),
            ));
        }
    };
    let primary = loop {
        if deadline.checked_duration_since(Instant::now()).is_none() {
            break Some(brokered_run_timeout_error(command_name, timeout));
        }
        let made_output_progress = match preserve_poll_result_before_timeout(
            drains.poll(),
            deadline.checked_duration_since(Instant::now()),
            || brokered_run_timeout_error(command_name, timeout),
        ) {
            Ok(made_progress) => made_progress,
            Err(error) => break Some(error),
        };
        let observation = process
            .observe_leader()
            .map_err(anyhow::Error::from)
            .context(format!("failed to poll brokered command '{command_name}'"));
        let observation = preserve_leader_poll_result_before_timeout(
            observation,
            deadline.checked_duration_since(Instant::now()),
            || brokered_run_timeout_error(command_name, timeout),
        );
        match observation {
            Ok(LeaderObservation::Running) => {}
            Ok(LeaderObservation::Exited) => break None,
            Err(error) => break Some(error),
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break Some(brokered_run_timeout_error(command_name, timeout));
        };
        if made_output_progress {
            thread::sleep(remaining.min(ACTIVE_OUTPUT_POLL_INTERVAL));
        } else {
            thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
        }
    };

    // End the entire owned tree on every path before the sole Unix wait. A
    // leader may exit successfully while a descendant still owns a pipe.
    let cleanup = process.terminate_and_reap(BROKERED_PROCESS_CLEANUP_TIMEOUT);
    let captured = drains.finish(BROKERED_OUTPUT_DRAIN_TIMEOUT);
    if let Some(primary) = primary {
        let primary = append_secondary_error(primary, "process cleanup also failed", cleanup.err());
        return Err(append_secondary_error(
            primary,
            "output drain also failed",
            captured.err(),
        ));
    }
    let status = match cleanup {
        Ok(status) => status,
        Err(primary) => {
            return Err(append_secondary_error(
                primary,
                "output drain also failed",
                captured.err(),
            ));
        }
    };
    let (stdout, stderr) = captured?;
    Ok((run_status(status), stdout, stderr))
}

pub(super) fn preserve_poll_result_before_timeout(
    result: AnyResult<bool>,
    remaining: Option<Duration>,
    timeout_error: impl FnOnce() -> anyhow::Error,
) -> AnyResult<bool> {
    let made_progress = result?;
    remaining.map(|_| made_progress).ok_or_else(timeout_error)
}

pub(super) fn preserve_leader_poll_result_before_timeout(
    result: AnyResult<LeaderObservation>,
    remaining: Option<Duration>,
    timeout_error: impl FnOnce() -> anyhow::Error,
) -> AnyResult<LeaderObservation> {
    match result {
        Ok(LeaderObservation::Exited) => Ok(LeaderObservation::Exited),
        Err(error) => Err(error),
        Ok(LeaderObservation::Running) => remaining
            .map(|_| LeaderObservation::Running)
            .ok_or_else(timeout_error),
    }
}

pub(super) fn brokered_run_timeout_error(command_name: &str, timeout: Duration) -> anyhow::Error {
    anyhow!("brokered command '{command_name}' exceeded the {timeout:?} run timeout")
}

pub(super) fn append_secondary_error(
    primary: anyhow::Error,
    label: &str,
    secondary: Option<anyhow::Error>,
) -> anyhow::Error {
    match secondary {
        Some(secondary) => anyhow!("{primary:#}; {label}: {secondary:#}"),
        None => primary,
    }
}

pub(super) fn fixed_process_cleanup_deadline(
    deadline: &mut Option<Instant>,
    timeout: Duration,
) -> AnyResult<(Instant, bool)> {
    if let Some(deadline) = *deadline {
        return Ok((deadline, false));
    }
    let new_deadline = checked_deadline("brokered process cleanup", timeout)?;
    *deadline = Some(new_deadline);
    Ok((new_deadline, true))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PortableRunStatus {
    pub(super) exit_status: i32,
    pub(super) exit_signal: Option<i32>,
}

pub(super) fn run_status(status: ExitStatus) -> PortableRunStatus {
    if let Some(code) = status.code() {
        return PortableRunStatus {
            exit_status: code,
            exit_signal: None,
        };
    }
    #[cfg(unix)]
    {
        let signal = status.signal();
        PortableRunStatus {
            exit_status: signal.map(|signal| 128 + signal).unwrap_or(1),
            exit_signal: signal,
        }
    }
    #[cfg(not(unix))]
    {
        PortableRunStatus {
            exit_status: 1,
            exit_signal: None,
        }
    }
}
