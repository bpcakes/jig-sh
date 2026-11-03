use std::process::{Child, Command, ExitStatus, Output};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use wait_timeout::ChildExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::unix::ProcessGroupId;

mod group_cleanup;
pub mod interaction;
mod output;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use group_cleanup::observe_owned_process;
use group_cleanup::{signal_owned_process_tree_once, terminate_owned_process_tree};
use output::{OutputDrain, OwnedProcessOutputDrains};

const OWNED_PROCESS_TREE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const OWNED_PROCESS_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
const OWNED_PROCESS_OUTPUT_LIMIT: usize = 16 * 1024;
// Output progress warrants a faster retry than idle process polling. A 1 ms
// floor keeps deadline and capture-limit enforcement responsive without
// sustaining thousands of wakeups per second for continuously chatty children.
const ACTIVE_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const TRUNCATED_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(2);
const MAX_OUTPUT_READS_PER_POLL: usize = 64;
const MAX_OUTPUT_READS_PER_POLL_AFTER_TRUNCATION: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessDeadline {
    At(Instant),
    Unbounded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessDeadlineRemaining {
    Time(Duration),
    Elapsed,
    Unbounded,
}

impl ProcessDeadline {
    fn after(timeout: Duration) -> Self {
        Instant::now()
            .checked_add(timeout)
            .map_or(Self::Unbounded, Self::At)
    }

    fn remaining(self) -> ProcessDeadlineRemaining {
        match self {
            Self::At(deadline) => deadline.checked_duration_since(Instant::now()).map_or(
                ProcessDeadlineRemaining::Elapsed,
                ProcessDeadlineRemaining::Time,
            ),
            Self::Unbounded => ProcessDeadlineRemaining::Unbounded,
        }
    }

    fn as_optional_instant(self) -> Option<Instant> {
        match self {
            Self::At(deadline) => Some(deadline),
            Self::Unbounded => None,
        }
    }
}

pub fn run_checked_output(
    command: &mut Command,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<Output> {
    let output = command.output()?;
    require_success(&output, failure_message)?;
    Ok(output)
}

pub fn run_checked_output_with_context(
    command: &mut Command,
    start_context: impl FnOnce() -> String,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<Output> {
    let output = command.output().with_context(start_context)?;
    require_success(&output, failure_message)?;
    Ok(output)
}

pub fn run_checked_stdout_trimmed(
    command: &mut Command,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<String> {
    let output = run_checked_output(command, failure_message)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn require_success(
    output: &Output,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        bail!("{}", failure_message(output))
    }
}

pub fn format_exit_status(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => "termination by signal".to_string(),
    }
}

pub struct OwnedProcessTreeOutput {
    pub status: ExitStatus,
    pub stdout: Option<BoundedProcessOutput>,
    pub stderr: Option<BoundedProcessOutput>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedProcessOutputStream {
    Stdout,
    Stderr,
}

impl std::fmt::Display for OwnedProcessOutputStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessOutputOverflowPolicy {
    /// Retain at most the configured limit while continuing to drain the pipe.
    Truncate,
    /// Fail when either final capture exceeds its limit, terminating the owned
    /// process tree promptly when overflow is observed before it exits.
    Error,
}

/// How much proof the runner requires that an owned process tree is gone.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProcessTreeCleanup {
    /// Re-signal the group until repeated membership checks prove it is empty,
    /// and fail when that proof cannot finish within the cleanup budget.
    #[default]
    Confirmed,
    /// Signal the pinned group once and reap the direct child without proving
    /// that descendants are gone. Use only for short read-only probes, such as
    /// Git source-identity commands and doctor version probes, whose leftovers
    /// cannot change what a check reports.
    BestEffort,
}

/// Receives process activity while an owned process tree is supervised.
/// Callbacks run on the supervision thread and should return quickly.
pub trait OwnedProcessObserver {
    fn cancelled(&mut self) -> bool {
        false
    }

    fn output(&mut self, _stream: OwnedProcessOutputStream, _bytes: &[u8]) {}

    fn poll(&mut self, _elapsed: Duration) {}
}

pub struct BoundedProcessOutput {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub complete: bool,
}

impl BoundedProcessOutput {
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

#[derive(Debug)]
pub enum OwnedProcessTreeError {
    Start(std::io::Error),
    TimedOut,
    CancelledBeforeStart,
    Cancelled,
    OutputLimitExceeded(OwnedProcessOutputStream),
    Await,
    /// Carries the reason cleanup could not confirm the tree was gone.
    Cleanup(std::io::Error),
}

impl OwnedProcessTreeError {
    pub const fn is_cancellation(&self) -> bool {
        match self {
            Self::CancelledBeforeStart | Self::Cancelled => true,
            Self::Start(_)
            | Self::TimedOut
            | Self::OutputLimitExceeded(_)
            | Self::Await
            | Self::Cleanup(_) => false,
        }
    }
}

impl std::fmt::Display for OwnedProcessTreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "the process tree could not start: {error}"),
            Self::TimedOut => formatter.write_str("the process tree timed out"),
            Self::CancelledBeforeStart => {
                formatter.write_str("the process tree was cancelled before it started")
            }
            Self::Cancelled => formatter.write_str("the process tree was cancelled"),
            Self::OutputLimitExceeded(stream) => {
                write!(
                    formatter,
                    "the process tree exceeded its {stream} output limit"
                )
            }
            Self::Await => formatter.write_str("the process tree could not be awaited"),
            Self::Cleanup(_) => {
                formatter.write_str("the process tree could not be cleaned up safely")
            }
        }
    }
}

impl std::error::Error for OwnedProcessTreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error) | Self::Cleanup(error) => Some(error),
            _ => None,
        }
    }
}

pub fn run_owned_process_tree_with_output(
    command: &mut Command,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_and_cleanup(
        command,
        timeout,
        ProcessTreeCleanup::Confirmed,
        cancelled,
    )
}

pub fn run_owned_process_tree_with_output_and_cleanup(
    command: &mut Command,
    timeout: Duration,
    cleanup: ProcessTreeCleanup,
    mut cancelled: impl FnMut() -> bool,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_policy_cleanup_and_observer(
        command,
        timeout,
        ProcessOutputLimits::default(),
        ProcessOutputOverflowPolicy::Truncate,
        cleanup,
        &mut CancellationObserver(&mut cancelled),
    )
}

struct CancellationObserver<'a, F>(&'a mut F);

impl<F: FnMut() -> bool> OwnedProcessObserver for CancellationObserver<'_, F> {
    fn cancelled(&mut self) -> bool {
        (self.0)()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessOutputLimits {
    pub stdout: usize,
    pub stderr: usize,
}

impl Default for ProcessOutputLimits {
    fn default() -> Self {
        Self {
            stdout: OWNED_PROCESS_OUTPUT_LIMIT,
            stderr: OWNED_PROCESS_OUTPUT_LIMIT,
        }
    }
}

pub fn run_owned_process_tree_with_output_limits(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    mut cancelled: impl FnMut() -> bool,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_limits_and_observer(
        command,
        timeout,
        limits,
        &mut CancellationObserver(&mut cancelled),
    )
}

pub fn run_owned_process_tree_with_output_limits_and_observer(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    observer: &mut dyn OwnedProcessObserver,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_policy_and_observer(
        command,
        timeout,
        limits,
        ProcessOutputOverflowPolicy::Truncate,
        observer,
    )
}

pub fn run_owned_process_tree_with_output_policy_and_observer(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    overflow_policy: ProcessOutputOverflowPolicy,
    observer: &mut dyn OwnedProcessObserver,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_policy_cleanup_and_observer(
        command,
        timeout,
        limits,
        overflow_policy,
        ProcessTreeCleanup::Confirmed,
        observer,
    )
}

pub fn run_owned_process_tree_with_output_policy_cleanup_and_observer(
    command: &mut Command,
    timeout: Duration,
    limits: ProcessOutputLimits,
    overflow_policy: ProcessOutputOverflowPolicy,
    cleanup: ProcessTreeCleanup,
    observer: &mut dyn OwnedProcessObserver,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    if observer.cancelled() {
        return Err(OwnedProcessTreeError::CancelledBeforeStart);
    }
    let mut process = spawn_owned_process(command).map_err(OwnedProcessTreeError::Start)?;
    process.cleanup = cleanup;
    let Ok(mut drains) = OwnedProcessOutputDrains::start(&mut process.child, limits) else {
        return match process.terminate_and_reap() {
            Ok(_) => Err(OwnedProcessTreeError::Await),
            Err(error) => Err(OwnedProcessTreeError::Cleanup(error)),
        };
    };
    let wait_result = wait_for_owned_process(
        &mut process,
        timeout,
        overflow_policy,
        observer,
        &mut drains,
    );
    let status = finish_owned_process_wait(&mut process, wait_result);
    let (stdout, stderr) = drains.finish(OWNED_PROCESS_OUTPUT_DRAIN_TIMEOUT, observer);
    finalize_owned_process_output(status, stdout, stderr, overflow_policy)
}

fn finalize_owned_process_output(
    status: std::result::Result<ExitStatus, OwnedProcessTreeError>,
    stdout: Option<BoundedProcessOutput>,
    stderr: Option<BoundedProcessOutput>,
    overflow_policy: ProcessOutputOverflowPolicy,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    let status = status?;
    let overflow = match overflow_policy {
        ProcessOutputOverflowPolicy::Truncate => None,
        ProcessOutputOverflowPolicy::Error => [
            (OwnedProcessOutputStream::Stdout, &stdout),
            (OwnedProcessOutputStream::Stderr, &stderr),
        ]
        .into_iter()
        .find_map(|(stream, output)| {
            output
                .as_ref()
                .is_some_and(|output| output.truncated)
                .then_some(stream)
        }),
    };
    if let Some(stream) = overflow {
        return Err(OwnedProcessTreeError::OutputLimitExceeded(stream));
    }
    Ok(OwnedProcessTreeOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PinnedProcessGroup {
    id: ProcessGroupId,
}

struct OwnedProcess {
    child: Child,
    #[cfg(unix)]
    process_group: Option<PinnedProcessGroup>,
    reaped_status: Option<ExitStatus>,
    cleanup: ProcessTreeCleanup,
    cleanup_complete: bool,
    cleanup_finalized: bool,
    cleanup_error: Option<StoredProcessCleanupError>,
    cleanup_deadline: Option<Instant>,
}

#[derive(Clone, Debug)]
struct StoredProcessCleanupError {
    kind: std::io::ErrorKind,
    message: String,
}

impl StoredProcessCleanupError {
    fn capture(error: &std::io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    fn to_io_error(&self) -> std::io::Error {
        std::io::Error::new(self.kind, self.message.clone())
    }
}

impl OwnedProcess {
    fn cleanup_deadline(&mut self) -> Instant {
        *self.cleanup_deadline.get_or_insert_with(|| {
            Instant::now()
                .checked_add(OWNED_PROCESS_TREE_CLEANUP_TIMEOUT)
                .unwrap_or_else(Instant::now)
        })
    }

    fn terminate_and_reap(&mut self) -> std::io::Result<ExitStatus> {
        match self.cleanup {
            ProcessTreeCleanup::Confirmed => {
                self.terminate_and_reap_with(terminate_owned_process_tree)
            }
            ProcessTreeCleanup::BestEffort => {
                self.terminate_and_reap_with(signal_owned_process_tree_once)
            }
        }
    }

    fn terminate_and_reap_with(
        &mut self,
        terminate_tree: impl FnOnce(&mut Self, Instant) -> std::io::Result<()>,
    ) -> std::io::Result<ExitStatus> {
        if self.cleanup_finalized {
            return if self.cleanup_complete {
                self.reaped_status.ok_or_else(|| {
                    std::io::Error::other("owned-process cleanup completed without a leader status")
                })
            } else {
                Err(self
                    .cleanup_error
                    .as_ref()
                    .map(StoredProcessCleanupError::to_io_error)
                    .unwrap_or_else(|| {
                        std::io::Error::other(
                            "owned-process cleanup failed without a retained error",
                        )
                    }))
            };
        }

        let deadline = self.cleanup_deadline();
        let mut tree_cleanup_error = terminate_tree(self, deadline).err();
        let mut direct_fallback_error = None;
        if tree_cleanup_error.is_some() && self.reaped_status.is_none() {
            direct_fallback_error = terminate_owned_process_fallback(self).err();
        }

        let mut reap_error = None;
        if self.reaped_status.is_none() {
            // Every permitted signal is attempted while the direct child's
            // unconsumed wait status still pins its Unix PID/PGID generation.
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            match self.child.wait_timeout(remaining) {
                Ok(Some(status)) => {
                    self.reaped_status = Some(status);
                    #[cfg(unix)]
                    {
                        self.process_group = None;
                    }
                }
                Ok(None) => {
                    reap_error = Some(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "owned-process cleanup timed out while reaping the direct child",
                    ));
                }
                Err(error) => {
                    #[cfg(unix)]
                    update_owned_process_identity_after_wait_error(self, &error);
                    reap_error = Some(error);
                }
            }
        }

        if let Some(error) = tree_cleanup_error.take() {
            let error = append_process_cleanup_error(
                error,
                "direct-child fallback also failed",
                direct_fallback_error,
            );
            let error =
                append_process_cleanup_error(error, "direct-child reap also failed", reap_error);
            return self.finalize_cleanup(Err(error));
        }
        if let Some(error) = reap_error {
            return self.finalize_cleanup(Err(error));
        }
        let status = self.reaped_status.ok_or_else(|| {
            std::io::Error::other("owned-process cleanup completed without a leader status")
        });
        self.finalize_cleanup(status)
    }

    fn finalize_cleanup(
        &mut self,
        result: std::io::Result<ExitStatus>,
    ) -> std::io::Result<ExitStatus> {
        self.cleanup_finalized = true;
        match result {
            Ok(status) => {
                self.cleanup_complete = true;
                Ok(status)
            }
            Err(error) => {
                self.cleanup_error = Some(StoredProcessCleanupError::capture(&error));
                Err(error)
            }
        }
    }
}

fn append_process_cleanup_error(
    primary: std::io::Error,
    label: &str,
    secondary: Option<std::io::Error>,
) -> std::io::Error {
    match secondary {
        Some(secondary) => {
            std::io::Error::new(primary.kind(), format!("{primary}; {label}: {secondary}"))
        }
        None => primary,
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.terminate_and_reap();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnedProcessWait {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    ExitedUnreaped,
    TimedOut,
    Cancelled,
    OutputLimitExceeded(OwnedProcessOutputStream),
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnedProcessObservation {
    Running,
    Exited,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_for_owned_process(
    process: &mut OwnedProcess,
    timeout: Duration,
    overflow_policy: ProcessOutputOverflowPolicy,
    observer: &mut dyn OwnedProcessObserver,
    drains: &mut OwnedProcessOutputDrains,
) -> std::io::Result<OwnedProcessWait> {
    let started = Instant::now();
    let deadline = ProcessDeadline::after(timeout);
    loop {
        let output_poll = drains.poll(observer);
        if overflow_policy == ProcessOutputOverflowPolicy::Error
            && let Some(stream) = output_poll.overflow
        {
            return Ok(OwnedProcessWait::OutputLimitExceeded(stream));
        }
        observer.poll(started.elapsed());
        if observer.cancelled() {
            return Ok(OwnedProcessWait::Cancelled);
        }
        if observe_owned_process(process)? == OwnedProcessObservation::Exited {
            return Ok(OwnedProcessWait::ExitedUnreaped);
        }

        match deadline.remaining() {
            ProcessDeadlineRemaining::Time(remaining) => {
                if output_poll.made_progress {
                    std::thread::sleep(remaining.min(drains.active_poll_interval()));
                } else {
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                }
            }
            ProcessDeadlineRemaining::Elapsed => return Ok(OwnedProcessWait::TimedOut),
            ProcessDeadlineRemaining::Unbounded if output_poll.made_progress => {
                std::thread::sleep(drains.active_poll_interval());
            }
            ProcessDeadlineRemaining::Unbounded => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[cfg(unix)]
fn update_owned_process_identity_after_wait_error(
    process: &mut OwnedProcess,
    error: &std::io::Error,
) {
    // ECHILD proves that this process no longer owns an unconsumed wait status;
    // another SIGCHLD consumer may have reaped the leader and released its
    // PID/PGID. EINVAL, ENOSYS, and other observation errors do not consume the
    // status, so the direct child continues to pin the group identity.
    if error.raw_os_error() == Some(libc::ECHILD) {
        process.process_group = None;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminate_owned_process_fallback(process: &mut OwnedProcess) -> std::io::Result<()> {
    if process.process_group.is_none() {
        return Err(std::io::Error::other(
            "owned child identity is no longer pinned; refusing direct fallback",
        ));
    }
    match process.child.kill() {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {
            if observe_owned_process(process)? == OwnedProcessObservation::Exited {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn terminate_owned_process_fallback(_process: &mut OwnedProcess) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn wait_for_owned_process(
    _process: &mut OwnedProcess,
    _timeout: Duration,
    _overflow_policy: ProcessOutputOverflowPolicy,
    observer: &mut dyn OwnedProcessObserver,
    drains: &mut OwnedProcessOutputDrains,
) -> std::io::Result<OwnedProcessWait> {
    drains.poll(observer);
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

fn finish_owned_process_wait(
    process: &mut OwnedProcess,
    wait_result: std::io::Result<OwnedProcessWait>,
) -> std::result::Result<ExitStatus, OwnedProcessTreeError> {
    let outcome = match wait_result {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        Ok(OwnedProcessWait::ExitedUnreaped) => None,
        Ok(OwnedProcessWait::TimedOut) => Some(Err(OwnedProcessTreeError::TimedOut)),
        Ok(OwnedProcessWait::Cancelled) => Some(Err(OwnedProcessTreeError::Cancelled)),
        Ok(OwnedProcessWait::OutputLimitExceeded(stream)) => {
            Some(Err(OwnedProcessTreeError::OutputLimitExceeded(stream)))
        }
        Err(_) => Some(Err(OwnedProcessTreeError::Await)),
    };
    // A owned process leader can exit while a background descendant keeps running.
    // End the owned tree on every outcome before reading captured output.
    let status = process
        .terminate_and_reap()
        .map_err(OwnedProcessTreeError::Cleanup)?;
    outcome.unwrap_or(Ok(status))
}

fn terminate_spawn_failure_child(child: &mut Child, deadline: Instant) {
    let _ = child.kill();
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .unwrap_or_default();
    let _ = child.wait_timeout(remaining);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_owned_process(command: &mut Command) -> std::io::Result<OwnedProcess> {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
    #[cfg(target_os = "linux")]
    {
        // SAFETY: getpid only reads this process's identity.
        let parent = unsafe { libc::getpid() };
        // SAFETY: the closure runs in the forked child before exec and calls
        // only async-signal-safe functions without allocating.
        unsafe {
            command.pre_exec(move || crate::unix::set_parent_death_signal(parent));
        }
    }
    let mut child = command.spawn()?;
    let Ok(process_group) = ProcessGroupId::try_from(child.id()) else {
        let deadline = Instant::now()
            .checked_add(OWNED_PROCESS_TREE_CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(std::io::Error::other(
            "owned process identifier is not representable",
        ));
    };
    Ok(OwnedProcess {
        child,
        process_group: Some(PinnedProcessGroup { id: process_group }),
        reaped_status: None,
        cleanup: ProcessTreeCleanup::Confirmed,
        cleanup_complete: false,
        cleanup_finalized: false,
        cleanup_error: None,
        cleanup_deadline: None,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn spawn_owned_process(_command: &mut Command) -> std::io::Result<OwnedProcess> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests;
