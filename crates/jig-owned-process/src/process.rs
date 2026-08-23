use std::io::Read;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Output};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use wait_timeout::ChildExt;

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

pub mod interaction;
mod output;

use output::{OutputDrain, OwnedProcessOutputDrains};

const OWNED_PROCESS_TREE_CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(any(target_os = "linux", target_os = "macos"))]
const OWNED_PROCESS_TREE_POLL_INTERVAL: Duration = Duration::from_millis(10);
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
    Cleanup,
}

impl OwnedProcessTreeError {
    pub const fn is_cancellation(&self) -> bool {
        match self {
            Self::CancelledBeforeStart | Self::Cancelled => true,
            Self::Start(_)
            | Self::TimedOut
            | Self::OutputLimitExceeded(_)
            | Self::Await
            | Self::Cleanup => false,
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
            Self::Cleanup => formatter.write_str("the process tree could not be cleaned up safely"),
        }
    }
}

impl std::error::Error for OwnedProcessTreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(error) => Some(error),
            _ => None,
        }
    }
}

pub fn run_owned_process_tree_with_output(
    command: &mut Command,
    timeout: Duration,
    cancelled: impl FnMut() -> bool,
) -> std::result::Result<OwnedProcessTreeOutput, OwnedProcessTreeError> {
    run_owned_process_tree_with_output_limits(
        command,
        timeout,
        ProcessOutputLimits::default(),
        cancelled,
    )
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
    struct CancellationObserver<'a, F>(&'a mut F);
    impl<F: FnMut() -> bool> OwnedProcessObserver for CancellationObserver<'_, F> {
        fn cancelled(&mut self) -> bool {
            (self.0)()
        }
    }

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
    if observer.cancelled() {
        return Err(OwnedProcessTreeError::CancelledBeforeStart);
    }
    let mut process = spawn_owned_process(command).map_err(OwnedProcessTreeError::Start)?;
    let Ok(mut drains) = OwnedProcessOutputDrains::start(&mut process.child, limits) else {
        return match process.terminate_and_reap() {
            Ok(_) => Err(OwnedProcessTreeError::Await),
            Err(_) => Err(OwnedProcessTreeError::Cleanup),
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

enum ProcessPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

impl ProcessPipe {
    #[cfg(unix)]
    fn prepare(&self) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;

        let descriptor = match self {
            Self::Stdout(reader) => reader.as_raw_fd(),
            Self::Stderr(reader) => reader.as_raw_fd(),
        };
        // SAFETY: the live pipe reader owns the descriptor; F_GETFL only inspects its flags.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the live descriptor keeps every flag when F_SETFL adds nonblocking reads.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn prepare(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "nonblocking process-pipe reads are unavailable on this platform",
        ))
    }

    #[cfg(unix)]
    fn read_available(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Stdout(reader) => reader.read(buffer),
            Self::Stderr(reader) => reader.read(buffer),
        }
    }

    #[cfg(not(unix))]
    fn read_available(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "nonblocking process-pipe reads are unavailable on this platform",
        ))
    }
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
        self.terminate_and_reap_with(terminate_owned_process_tree)
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
    let cleanup = process.terminate_and_reap();
    if cleanup.is_err() {
        return Err(OwnedProcessTreeError::Cleanup);
    }
    match outcome {
        Some(outcome) => outcome,
        None => cleanup.map_err(|_| OwnedProcessTreeError::Await),
    }
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe_owned_process(process: &mut OwnedProcess) -> std::io::Result<OwnedProcessObservation> {
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
fn terminate_owned_process_tree(
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessGroupSignalResult {
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
fn observe_owned_process_before_group_signal_with<T>(
    state: &mut T,
    mut observe: impl FnMut(&mut T) -> std::io::Result<OwnedProcessObservation>,
    signal: impl FnOnce(&mut T, OwnedProcessObservation) -> std::io::Result<ProcessGroupSignalResult>,
) -> std::io::Result<ProcessGroupSignalResult> {
    let observation = observe(state)?;
    signal(state, observation)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn signal_pinned_process_group(
    process: &mut OwnedProcess,
    expected_process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<ProcessGroupSignalResult> {
    ensure_owned_process_cleanup_budget(deadline, "before process-group SIGKILL")?;
    pinned_process_group_for_retry(process, expected_process_group)?;
    observe_owned_process_before_group_signal_with(
        process,
        observe_owned_process,
        |process, leader_observation| {
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
                return Ok(ProcessGroupSignalResult::Delivered);
            }

            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                // ESRCH only says that this pinned generation had no signalable
                // member at this instant. A concurrently starting descendant
                // may still become visible, so only the following platform
                // proof may finish cleanup.
                return Ok(ProcessGroupSignalResult::Inconclusive);
            }
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM) {
                return resolve_macos_process_group_signal_eperm(error, Ok(leader_observation));
            }
            #[cfg(not(target_os = "macos"))]
            let _ = leader_observation;
            Err(error)
        },
    )
}

#[cfg(target_os = "macos")]
fn resolve_macos_process_group_signal_eperm(
    signal_error: std::io::Error,
    leader_observation: std::io::Result<OwnedProcessObservation>,
) -> std::io::Result<ProcessGroupSignalResult> {
    match leader_observation {
        Ok(OwnedProcessObservation::Exited) => {
            // Darwin can report EPERM for a group containing only its zombie
            // leader, but EPERM is not absence. The confirmation loop must
            // still take a fresh atomic sole-leader snapshot before success.
            Ok(ProcessGroupSignalResult::Inconclusive)
        }
        Ok(OwnedProcessObservation::Running) => Err(signal_error),
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
fn confirm_process_group_quiescent_with<T>(
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
            linux_process_group_has_live_members(process_group, deadline).map(|live| !live)
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

#[cfg(target_os = "linux")]
fn linux_process_group_has_live_members(
    process_group: libc::pid_t,
    deadline: Instant,
) -> std::io::Result<bool> {
    let mut within_budget = || {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| !remaining.is_zero())
    };
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    let entries = std::fs::read_dir("/proc");
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    let pids = collect_linux_process_ids_with(
        process_group,
        entries?,
        |entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<libc::pid_t>().ok())
        },
        &mut within_budget,
    )?;
    linux_process_group_has_live_members_bytes_with(
        process_group,
        pids,
        // The parenthesized command name in /proc/<pid>/stat may contain
        // arbitrary bytes even though the process-state fields are ASCII.
        |pid| std::fs::read(format!("/proc/{pid}/stat")),
        linux_process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_process_ids_with<T>(
    process_group: i32,
    mut entries: impl Iterator<Item = std::io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<i32>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<Vec<i32>> {
    let mut pids = Vec::new();
    loop {
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let entry = entries.next();
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let Some(entry) = entry else {
            return Ok(pids);
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if let Some(pid) = process_id(entry) {
            pids.push(pid);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn ensure_linux_process_scan_budget(
    _process_group: i32,
    within_budget: &mut impl FnMut() -> bool,
) -> std::io::Result<()> {
    if within_budget() {
        Ok(())
    } else {
        Err(owned_process_cleanup_timeout(
            "while scanning Linux processes",
        ))
    }
}

#[cfg(test)]
fn linux_process_group_has_live_members_with(
    process_group: i32,
    pids: impl IntoIterator<Item = i32>,
    mut read_stat: impl FnMut(i32) -> std::io::Result<String>,
    mut process_group_for_pid: impl FnMut(i32) -> std::io::Result<Option<i32>>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<bool> {
    linux_process_group_has_live_members_bytes_with(
        process_group,
        pids,
        |pid| read_stat(pid).map(String::into_bytes),
        &mut process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn linux_process_group_has_live_members_bytes_with(
    process_group: i32,
    pids: impl IntoIterator<Item = i32>,
    mut read_stat: impl FnMut(i32) -> std::io::Result<Vec<u8>>,
    mut process_group_for_pid: impl FnMut(i32) -> std::io::Result<Option<i32>>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<bool> {
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    for pid in pids {
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let observation = read_stat(pid).and_then(|stat| parse_linux_process_stat(pid, &stat));
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let observation = match observation {
            Ok(observation) => observation,
            Err(stat_error) => {
                ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
                let observed_group = process_group_for_pid(pid);
                ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
                match observed_group {
                    Ok(None) => continue,
                    Ok(Some(other_group)) if other_group != process_group => continue,
                    Ok(Some(_)) => {
                        return Err(std::io::Error::new(
                            stat_error.kind(),
                            format!(
                                "could not inspect process {pid} in owned group {process_group}: {stat_error}"
                            ),
                        ));
                    }
                    Err(group_error) => {
                        return Err(std::io::Error::new(
                            stat_error.kind(),
                            format!(
                                "could not inspect process {pid} or prove it is outside owned group {process_group}: {stat_error}; group lookup failed: {group_error}"
                            ),
                        ));
                    }
                }
            }
        };
        if observation.process_group == process_group && observation.live {
            ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
            return Ok(true);
        }
    }
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxProcessObservation {
    process_group: i32,
    live: bool,
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(
    expected_pid: i32,
    stat: &[u8],
) -> std::io::Result<LinuxProcessObservation> {
    let expected_prefix = format!("{expected_pid} (");
    if expected_pid <= 0 || !stat.starts_with(expected_prefix.as_bytes()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Linux process stat did not begin with the expected process identifier",
        ));
    }
    let command_end = stat
        .windows(2)
        .rposition(|window| window == b") ")
        .filter(|command_end| *command_end >= expected_prefix.len())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing Linux process stat command field",
            )
        })?;
    let fields = std::str::from_utf8(&stat[command_end + 2..]).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Linux process stat fields are not valid UTF-8",
        )
    })?;
    let mut fields = fields.split_whitespace();
    let state = fields.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing process state")
    })?;
    let process_group = fields
        .nth(1)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing process group")
        })?
        .parse::<i32>()
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid process group")
        })?;
    Ok(LinuxProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
fn linux_process_group_for_pid(pid: libc::pid_t) -> std::io::Result<Option<libc::pid_t>> {
    // SAFETY: `pid` is a positive identifier enumerated from /proc and this
    // call only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return Ok(Some(process_group));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
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
fn terminate_owned_process_tree(
    _process: &mut OwnedProcess,
    _deadline: Instant,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests;
