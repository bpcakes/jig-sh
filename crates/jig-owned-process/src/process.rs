use std::io::Read;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Output};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use wait_timeout::ChildExt;

pub mod interaction;

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
    Cancelled,
    Await,
    Cleanup,
}

impl std::fmt::Display for OwnedProcessTreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "the process tree could not start: {error}"),
            Self::TimedOut => formatter.write_str("the process tree timed out"),
            Self::Cancelled => formatter.write_str("the process tree was cancelled"),
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
    if cancelled() {
        return Err(OwnedProcessTreeError::Cancelled);
    }
    let mut process = spawn_owned_process(command).map_err(OwnedProcessTreeError::Start)?;
    let Ok(mut drains) = OwnedProcessOutputDrains::start(&mut process.child, limits) else {
        return match process.terminate_and_reap() {
            Ok(_) => Err(OwnedProcessTreeError::Await),
            Err(_) => Err(OwnedProcessTreeError::Cleanup),
        };
    };
    let wait_result = wait_for_owned_process(&mut process, timeout, &mut cancelled, &mut drains);
    let status = finish_owned_process_wait(&mut process, wait_result);
    let (stdout, stderr) = drains.finish(OWNED_PROCESS_OUTPUT_DRAIN_TIMEOUT);
    status.map(|status| OwnedProcessTreeOutput {
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

    #[cfg(windows)]
    fn prepare(&self) -> std::io::Result<()> {
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
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

    #[cfg(windows)]
    fn read_available(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, HANDLE};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        let handle = match self {
            Self::Stdout(reader) => reader.as_raw_handle(),
            Self::Stderr(reader) => reader.as_raw_handle(),
        } as HANDLE;
        let mut available = 0_u32;
        // SAFETY: `handle` is a live anonymous-pipe read handle and the only
        // output pointer names a writable u32. No bytes are copied by this
        // availability-only call.
        let peeked = unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_NO_DATA as i32
            ) {
                return Ok(0);
            }
            return Err(error);
        }
        if available == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        }
        let read_limit = buffer.len().min(available as usize);
        match self {
            Self::Stdout(reader) => reader.read(&mut buffer[..read_limit]),
            Self::Stderr(reader) => reader.read(&mut buffer[..read_limit]),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn read_available(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "nonblocking process-pipe reads are unavailable on this platform",
        ))
    }
}

struct OutputDrain {
    reader: Option<ProcessPipe>,
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
    complete: bool,
}

impl OutputDrain {
    fn start(reader: ProcessPipe, limit: usize) -> std::io::Result<Self> {
        reader.prepare()?;
        Ok(Self {
            reader: Some(reader),
            bytes: Vec::new(),
            limit,
            truncated: false,
            complete: false,
        })
    }

    fn poll(&mut self) -> bool {
        // A shell can issue thousands of tiny writes. Bound every poll to 64
        // read attempts while the retained output is capped separately. Once
        // capture truncates, tighten the same attempt budget to 16 so repeated
        // short reads and interrupts remain bounded alike.

        let Some(reader) = self.reader.as_mut() else {
            return false;
        };
        let mut chunk = [0_u8; 4096];
        let mut made_progress = false;
        for read_index in 0..MAX_OUTPUT_READS_PER_POLL {
            if self.truncated && read_index >= MAX_OUTPUT_READS_PER_POLL_AFTER_TRUNCATION {
                return made_progress;
            }
            match reader.read_available(&mut chunk) {
                Ok(0) => {
                    self.complete = true;
                    self.reader = None;
                    return made_progress;
                }
                Ok(read) => {
                    made_progress = true;
                    let remaining = self.limit.saturating_sub(self.bytes.len());
                    let retained = remaining.min(read);
                    self.bytes.extend_from_slice(&chunk[..retained]);
                    self.truncated |= retained < read;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return made_progress;
                }
                Err(_) => {
                    // Closing the reader makes an I/O failure terminal and
                    // records the capture as incomplete without retaining a
                    // blocked worker or retry loop.
                    self.reader = None;
                    return made_progress;
                }
            }
        }
        made_progress
    }

    const fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    fn finish(self) -> BoundedProcessOutput {
        BoundedProcessOutput {
            bytes: self.bytes,
            truncated: self.truncated,
            complete: self.complete,
        }
    }
}

struct OwnedProcessOutputDrains {
    stdout: Option<OutputDrain>,
    stderr: Option<OutputDrain>,
}

impl OwnedProcessOutputDrains {
    fn start(child: &mut Child, limits: ProcessOutputLimits) -> std::io::Result<Self> {
        let stdout = child
            .stdout
            .take()
            .map(|reader| OutputDrain::start(ProcessPipe::Stdout(reader), limits.stdout))
            .transpose()?;
        let stderr = child
            .stderr
            .take()
            .map(|reader| OutputDrain::start(ProcessPipe::Stderr(reader), limits.stderr))
            .transpose()?;
        Ok(Self { stdout, stderr })
    }

    fn poll(&mut self) -> bool {
        let stdout_progress = self.stdout.as_mut().is_some_and(OutputDrain::poll);
        let stderr_progress = self.stderr.as_mut().is_some_and(OutputDrain::poll);
        stdout_progress || stderr_progress
    }

    fn is_terminal(&self) -> bool {
        self.stdout.as_ref().is_none_or(OutputDrain::is_terminal)
            && self.stderr.as_ref().is_none_or(OutputDrain::is_terminal)
    }

    fn active_poll_interval(&self) -> Duration {
        if self.stdout.as_ref().is_some_and(|drain| drain.truncated)
            || self.stderr.as_ref().is_some_and(|drain| drain.truncated)
        {
            TRUNCATED_OUTPUT_POLL_INTERVAL
        } else {
            ACTIVE_OUTPUT_POLL_INTERVAL
        }
    }

    fn finish(
        mut self,
        timeout: Duration,
    ) -> (Option<BoundedProcessOutput>, Option<BoundedProcessOutput>) {
        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        while !self.is_terminal() && Instant::now() < deadline {
            let made_progress = self.poll();
            if !self.is_terminal() {
                if made_progress {
                    std::thread::sleep(self.active_poll_interval());
                } else {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
        // Dropping any still-open reader here closes the local pipe promptly.
        // No worker owns another copy, so an escaped silent writer cannot keep
        // a detached capture thread alive.
        let stdout = self.stdout.map(OutputDrain::finish);
        let stderr = self.stderr.map(OutputDrain::finish);
        (stdout, stderr)
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PinnedProcessGroup {
    id: libc::pid_t,
}

struct OwnedProcess {
    child: Child,
    #[cfg(unix)]
    process_group: Option<PinnedProcessGroup>,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
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
        let mut tree_cleanup_error = terminate_owned_process_tree(self, deadline).err();
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
    #[cfg(windows)]
    ExitedReaped(ExitStatus),
    TimedOut,
    Cancelled,
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
    cancelled: &mut impl FnMut() -> bool,
    drains: &mut OwnedProcessOutputDrains,
) -> std::io::Result<OwnedProcessWait> {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        let made_output_progress = drains.poll();
        if cancelled() {
            return Ok(OwnedProcessWait::Cancelled);
        }
        if observe_owned_process(process)? == OwnedProcessObservation::Exited {
            return Ok(OwnedProcessWait::ExitedUnreaped);
        }

        match deadline {
            Some(deadline) => {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    return Ok(OwnedProcessWait::TimedOut);
                };
                if made_output_progress {
                    std::thread::sleep(remaining.min(drains.active_poll_interval()));
                } else {
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                }
            }
            None if made_output_progress => std::thread::sleep(drains.active_poll_interval()),
            None => std::thread::sleep(Duration::from_millis(10)),
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

#[cfg(windows)]
fn terminate_owned_process_fallback(process: &mut OwnedProcess) -> std::io::Result<()> {
    // `Child` retains the exact process HANDLE even if Job Object termination
    // or confirmation failed, so this fallback cannot target a recycled PID.
    process.child.kill()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminate_owned_process_fallback(_process: &mut OwnedProcess) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(windows)]
fn wait_for_owned_process(
    process: &mut OwnedProcess,
    timeout: Duration,
    cancelled: &mut impl FnMut() -> bool,
    drains: &mut OwnedProcessOutputDrains,
) -> std::io::Result<OwnedProcessWait> {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        drains.poll();
        if cancelled() {
            return Ok(OwnedProcessWait::Cancelled);
        }
        let remaining = match deadline {
            Some(deadline) => {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    return Ok(OwnedProcessWait::TimedOut);
                };
                remaining
            }
            None => Duration::from_millis(10),
        };
        if let Some(status) = process
            .child
            .wait_timeout(remaining.min(Duration::from_millis(10)))?
        {
            return Ok(OwnedProcessWait::ExitedReaped(status));
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn wait_for_owned_process(
    _process: &mut OwnedProcess,
    _timeout: Duration,
    _cancelled: &mut impl FnMut() -> bool,
    drains: &mut OwnedProcessOutputDrains,
) -> std::io::Result<OwnedProcessWait> {
    drains.poll();
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

fn finish_owned_process_wait(
    process: &mut OwnedProcess,
    wait_result: std::io::Result<OwnedProcessWait>,
) -> std::result::Result<ExitStatus, OwnedProcessTreeError> {
    #[cfg(windows)]
    let wait_result = match wait_result {
        Ok(OwnedProcessWait::ExitedReaped(status)) => {
            // A Windows Job Object remains a stable tree identity after its
            // leader exits. Cache the consumed status, terminate the owned
            // job, and only then mark cleanup complete.
            process.reaped_status = Some(status);
            return process
                .terminate_and_reap()
                .map_err(|_| OwnedProcessTreeError::Cleanup);
        }
        other => other,
    };
    let outcome = match wait_result {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        Ok(OwnedProcessWait::ExitedUnreaped) => None,
        #[cfg(windows)]
        Ok(OwnedProcessWait::ExitedReaped(status)) => Some(Ok(status)),
        Ok(OwnedProcessWait::TimedOut) => Some(Err(OwnedProcessTreeError::TimedOut)),
        Ok(OwnedProcessWait::Cancelled) => Some(Err(OwnedProcessTreeError::Cancelled)),
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
    let Ok(process_group) = libc::pid_t::try_from(child.id()) else {
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

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
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
    let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: `information` is writable storage, the identifier names our
    // direct child, and WNOWAIT retains its status so the PID continues to pin
    // this exact process-group generation through cleanup.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_group.id as _,
            information.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == 0 {
        // SAFETY: successful `waitid` initialized the siginfo value and its
        // SIGCHLD union member.
        let information = unsafe { information.assume_init() };
        let observed_pid = unsafe { information.si_pid() };
        return classify_owned_process_waitid_observation(
            process_group.id,
            observed_pid,
            information.si_code,
        );
    }

    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::Interrupted {
        return Ok(OwnedProcessObservation::Running);
    }
    update_owned_process_identity_after_wait_error(process, &error);
    Err(error)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn classify_owned_process_waitid_observation(
    expected_pid: libc::pid_t,
    observed_pid: libc::pid_t,
    code: libc::c_int,
) -> std::io::Result<OwnedProcessObservation> {
    if observed_pid == 0 {
        return Ok(OwnedProcessObservation::Running);
    }
    if observed_pid != expected_pid {
        return Err(std::io::Error::other(format!(
            "waitid observed unexpected owned child PID {observed_pid} instead of {expected_pid}"
        )));
    }
    match code {
        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED => {
            Ok(OwnedProcessObservation::Exited)
        }
        libc::CLD_STOPPED | libc::CLD_TRAPPED | libc::CLD_CONTINUED => {
            Ok(OwnedProcessObservation::Running)
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("waitid returned an unrecognized owned child state code {code}"),
        )),
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
    if process_group.id <= 0 {
        return Err(std::io::Error::other(
            "owned process-group identity is not positive",
        ));
    }
    confirm_process_group_quiescent(process, process_group.id, deadline)
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
    if process_group.id != expected_process_group {
        return Err(std::io::Error::other(format!(
            "owned process-group identity changed from pinned group {expected_process_group} to {}",
            process_group.id
        )));
    }
    if process_group.id <= 0 {
        return Err(std::io::Error::other(
            "owned process-group identity is not positive",
        ));
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
            if unsafe { libc::kill(-process_group.id, libc::SIGKILL) } == 0 {
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
    if required_consecutive_proofs == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "owned process-group confirmation requires at least one proof",
        ));
    }

    let mut consecutive_proofs = 0_u8;
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
        if quiescent {
            consecutive_proofs += 1;
            if consecutive_proofs == required_consecutive_proofs {
                return Ok(());
            }
        } else {
            consecutive_proofs = 0;
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
    let mut members = [0 as libc::pid_t; 2];
    let buffer_size = i32::try_from(std::mem::size_of_val(&members)).map_err(|_| {
        std::io::Error::other("macOS process-group snapshot buffer was not representable")
    })?;
    // SAFETY: `members` is writable storage for two pid_t values and the byte
    // count describes that full buffer. A full buffer means at least two live
    // group entries, so collection is intentionally capped at two.
    let count =
        unsafe { libc::proc_listpgrppids(process_group, members.as_mut_ptr().cast(), buffer_size) };
    if count <= 0 {
        return Err(std::io::Error::other(format!(
            "failed to atomically list owned process group {process_group}: {}",
            std::io::Error::last_os_error()
        )));
    }
    classify_macos_process_group_snapshot(process_group, count, members)
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_process_group_snapshot(
    process_group: i32,
    count: i32,
    members: [i32; 2],
) -> std::io::Result<bool> {
    if process_group <= 0 {
        return Err(std::io::Error::other(
            "macOS process-group snapshot used a non-positive pinned leader",
        ));
    }
    let count = usize::try_from(count).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "macOS process-group snapshot returned a negative member count",
        )
    })?;
    if count == 0 || count > members.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("macOS process-group snapshot returned an untrusted member count of {count}"),
        ));
    }
    let observed = &members[..count];
    if observed.iter().any(|pid| *pid <= 0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "macOS process-group snapshot returned a non-positive member identifier",
        ));
    }
    if count == members.len() {
        return Ok(false);
    }
    if observed[0] != process_group {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "macOS process-group snapshot did not contain the exact pinned leader {process_group}"
            ),
        ));
    }
    Ok(true)
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

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminate_owned_process_tree(
    _process: &mut OwnedProcess,
    _deadline: Instant,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "owned process-tree supervision is unavailable on this platform",
    ))
}

#[cfg(windows)]
fn spawn_owned_process(command: &mut Command) -> std::io::Result<OwnedProcess> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = create_owned_process_job()?;
    command.creation_flags(windows_owned_process_creation_flags());
    let mut child = command.spawn()?;
    // SAFETY: both handles are live handles owned by `job` and `child`.
    let assigned = unsafe {
        AssignProcessToJobObject(
            job.as_raw_handle() as HANDLE,
            child.as_raw_handle() as HANDLE,
        )
    };
    if assigned == 0 || resume_owned_process(child.id()).is_err() {
        let deadline = Instant::now()
            .checked_add(OWNED_PROCESS_TREE_CLEANUP_TIMEOUT)
            .unwrap_or_else(Instant::now);
        let _ = terminate_windows_job(&job, deadline);
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(std::io::Error::other(
            "could not isolate the owned process tree",
        ));
    }
    Ok(OwnedProcess {
        child,
        job,
        reaped_status: None,
        cleanup_complete: false,
        cleanup_finalized: false,
        cleanup_error: None,
        cleanup_deadline: None,
    })
}

#[cfg(windows)]
const fn windows_owned_process_creation_flags() -> u32 {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP
}

#[cfg(windows)]
fn create_owned_process_job() -> std::io::Result<std::os::windows::io::OwnedHandle> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // SAFETY: null attributes and name request a private unnamed Job Object.
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw_job` is a newly created owned handle and is transferred
    // exactly once into `OwnedHandle`.
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job as RawHandle) };
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the information pointer and byte length describe a live value of
    // the class requested, and the Job Object handle remains owned by `job`.
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw const information).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(job)
}

#[cfg(windows)]
fn resume_owned_process(pid: u32) -> std::io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot call has no borrowed pointer arguments.
    let raw_snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw_snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw_snapshot` is a newly created owned handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw_snapshot as RawHandle) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: `entry` is initialized with the required size and remains valid
    // for the duration of the snapshot enumeration.
    let mut has_entry = unsafe { Thread32First(raw_snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the enumerated thread ID belongs to the suspended child;
            // no handle is inherited by the resumed process.
            let raw_thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if raw_thread.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: `raw_thread` is a newly opened owned handle.
            let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread as RawHandle) };
            // SAFETY: `thread` names the initial thread created suspended by
            // `CREATE_SUSPENDED` and remains live for this call.
            let previous_count = unsafe { ResumeThread(raw_thread) };
            drop(thread);
            drop(snapshot);
            if previous_count == u32::MAX {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: same initialized entry and live snapshot as above.
        has_entry = unsafe { Thread32Next(raw_snapshot, &mut entry) } != 0;
    }
    Err(std::io::Error::other(
        "could not find the suspended owned process thread",
    ))
}

#[cfg(windows)]
fn terminate_windows_job(
    job: &std::os::windows::io::OwnedHandle,
    deadline: Instant,
) -> std::io::Result<()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, TerminateJobObject,
    };

    // SAFETY: `job` is a live Job Object owned by the caller.
    let result = unsafe { TerminateJobObject(job.as_raw_handle() as HANDLE, 1) };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    wait_for_no_active_processes_until(deadline, || {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the Job Object handle is live, the output pointer names a
        // correctly sized accounting value, and no return-length is needed.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectBasicAccountingInformation,
                (&raw mut information).cast::<c_void>(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(information.ActiveProcesses)
        }
    })
}

#[cfg(windows)]
fn terminate_owned_process_tree(
    process: &mut OwnedProcess,
    deadline: Instant,
) -> std::io::Result<()> {
    terminate_windows_job(&process.job, deadline)
}

#[cfg(test)]
fn wait_for_no_active_processes(
    timeout: Duration,
    active_processes: impl FnMut() -> std::io::Result<u32>,
) -> std::io::Result<()> {
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(std::time::Instant::now);
    wait_for_no_active_processes_until(deadline, active_processes)
}

#[cfg(any(windows, test))]
fn wait_for_no_active_processes_until(
    deadline: Instant,
    mut active_processes: impl FnMut() -> std::io::Result<u32>,
) -> std::io::Result<()> {
    loop {
        if active_processes()? == 0 {
            return Ok(());
        }
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "owned process tree cleanup timed out",
            ));
        };
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(test)]
mod tests;
