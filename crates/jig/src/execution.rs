use std::fmt;
use std::num::NonZeroUsize;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use jig_owned_process::{
    BoundedProcessOutput, OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError,
    ProcessOutputLimits, ProcessOutputOverflowPolicy,
    run_owned_process_tree_with_output_policy_and_observer,
};

use crate::context::{CommandOutputLimit, CommandTimeout};

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
pub(crate) const EXECUTION_OUTPUT_CAPTURE_LIMIT: usize = 4 * 1024 * 1024;
const MAX_OVERFLOW_DIAGNOSTIC_BYTES: usize = 64 * 1024;

pub(crate) fn internal_execution_output_limit() -> CommandOutputLimit {
    CommandOutputLimit::from_bytes(EXECUTION_OUTPUT_CAPTURE_LIMIT as u64)
        .expect("internal execution output limit is valid")
}

#[derive(Debug)]
pub(crate) struct ExecutionCommandOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum SupervisedExecutionError {
    CancelledBeforeStart,
    Cancelled,
    TimedOut,
    OutputLimitExceeded {
        stream: OwnedProcessOutputStream,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Failed {
        error: anyhow::Error,
        process_started: bool,
    },
}

#[derive(Debug)]
pub(crate) enum ExecutionCommandError {
    CancelledBeforeStart,
    Cancelled,
    Failed {
        error: anyhow::Error,
        process_started: bool,
    },
}

impl ExecutionCommandError {
    pub(crate) fn failed(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed {
            error: error.into(),
            process_started: false,
        }
    }

    pub(crate) fn failed_after_start(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed {
            error: error.into(),
            process_started: true,
        }
    }

    pub(crate) fn into_anyhow(self) -> anyhow::Error {
        match self {
            Self::CancelledBeforeStart => anyhow!("Execution was cancelled before it started"),
            Self::Cancelled => anyhow!("Execution was cancelled"),
            Self::Failed { error, .. } => error,
        }
    }
}

impl fmt::Display for ExecutionCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CancelledBeforeStart => {
                formatter.write_str("Execution was cancelled before it started")
            }
            Self::Cancelled => formatter.write_str("Execution was cancelled"),
            Self::Failed { error, .. } => fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ExecutionCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Failed { error, .. } => error.source(),
            Self::CancelledBeforeStart | Self::Cancelled => None,
        }
    }
}

impl From<anyhow::Error> for ExecutionCommandError {
    fn from(error: anyhow::Error) -> Self {
        Self::failed(error)
    }
}

pub(crate) fn run_authoritative_execution_command(
    command: &mut Command,
    timeout: CommandTimeout,
    output_limit: CommandOutputLimit,
    label: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<ExecutionCommandOutput, ExecutionCommandError> {
    run_authoritative_execution_command_for_duration(
        command,
        timeout.duration(),
        output_limit,
        label,
        observer,
    )
}

pub(crate) fn run_authoritative_execution_command_for_duration(
    command: &mut Command,
    timeout: Duration,
    output_limit: CommandOutputLimit,
    label: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<ExecutionCommandOutput, ExecutionCommandError> {
    run_supervised_execution_command(command, timeout, output_limit, label, observer)
        .map_err(|error| execution_command_error_for_duration(error, timeout, output_limit, label))
}

/// Runs a repository-owned command under the complete non-interactive process
/// policy. Callers cannot accidentally inherit stdin, skip capture, or choose a
/// truncating overflow policy by partially configuring `command` themselves.
pub(crate) fn run_supervised_execution_command(
    command: &mut Command,
    timeout: Duration,
    output_limit: CommandOutputLimit,
    label: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<ExecutionCommandOutput, SupervisedExecutionError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process_observer = CapturingProcessExecutionObserver::new(
        observer,
        label,
        output_limit.bytes().min(MAX_OVERFLOW_DIAGNOSTIC_BYTES),
    );
    let output = run_owned_process_tree_with_output_policy_and_observer(
        command,
        timeout,
        ProcessOutputLimits {
            stdout: output_limit.bytes(),
            stderr: output_limit.bytes(),
        },
        ProcessOutputOverflowPolicy::Error,
        &mut process_observer,
    );
    let captured = process_observer.into_capture();
    let output = output.map_err(|error| supervised_execution_error(error, label, captured))?;

    let (stdout, stderr) = complete_supervised_captures(output.stdout, output.stderr, label)?;
    Ok(ExecutionCommandOutput {
        status: output.status,
        stdout,
        stderr,
    })
}

pub(crate) fn execution_command_error(
    error: SupervisedExecutionError,
    timeout: CommandTimeout,
    output_limit: CommandOutputLimit,
    label: &str,
) -> ExecutionCommandError {
    execution_command_error_for_duration(error, timeout.duration(), output_limit, label)
}

fn execution_command_error_for_duration(
    error: SupervisedExecutionError,
    timeout: Duration,
    output_limit: CommandOutputLimit,
    label: &str,
) -> ExecutionCommandError {
    let error = match error {
        SupervisedExecutionError::CancelledBeforeStart => {
            return ExecutionCommandError::CancelledBeforeStart;
        }
        SupervisedExecutionError::Cancelled => return ExecutionCommandError::Cancelled,
        SupervisedExecutionError::TimedOut => {
            anyhow!("{label} timed out after {}", timeout_description(timeout))
        }
        SupervisedExecutionError::OutputLimitExceeded { stream, .. } => anyhow!(
            "{label} exceeded the {} byte {stream} capture limit",
            output_limit.bytes()
        ),
        SupervisedExecutionError::Failed {
            error,
            process_started,
        } => {
            return ExecutionCommandError::Failed {
                error,
                process_started,
            };
        }
    };
    ExecutionCommandError::failed_after_start(error)
}

fn timeout_description(timeout: Duration) -> String {
    if timeout.subsec_nanos() == 0 {
        format!("{} seconds", timeout.as_secs())
    } else {
        format!("{timeout:?}")
    }
}

fn supervised_execution_error(
    error: OwnedProcessTreeError,
    label: &str,
    captured: SupervisedCapture,
) -> SupervisedExecutionError {
    match error {
        OwnedProcessTreeError::CancelledBeforeStart => {
            SupervisedExecutionError::CancelledBeforeStart
        }
        OwnedProcessTreeError::Cancelled => SupervisedExecutionError::Cancelled,
        OwnedProcessTreeError::TimedOut => SupervisedExecutionError::TimedOut,
        OwnedProcessTreeError::OutputLimitExceeded(stream) => {
            SupervisedExecutionError::OutputLimitExceeded {
                stream,
                stdout: captured.stdout,
                stderr: captured.stderr,
            }
        }
        OwnedProcessTreeError::Start(error) => SupervisedExecutionError::Failed {
            error: anyhow!("{label} could not start: {error}"),
            process_started: false,
        },
        OwnedProcessTreeError::Await => SupervisedExecutionError::Failed {
            error: anyhow!("{label} could not be awaited"),
            process_started: true,
        },
        OwnedProcessTreeError::Cleanup => SupervisedExecutionError::Failed {
            error: anyhow!("{label} process tree could not be cleaned up safely"),
            process_started: true,
        },
    }
}

fn complete_supervised_captures(
    stdout: Option<BoundedProcessOutput>,
    stderr: Option<BoundedProcessOutput>,
    label: &str,
) -> Result<(Vec<u8>, Vec<u8>), SupervisedExecutionError> {
    let stdout = stdout.ok_or_else(|| SupervisedExecutionError::Failed {
        error: anyhow!("{label} did not capture stdout"),
        process_started: true,
    })?;
    let stderr = stderr.ok_or_else(|| SupervisedExecutionError::Failed {
        error: anyhow!("{label} did not capture stderr"),
        process_started: true,
    })?;
    if stdout.truncated || stderr.truncated {
        let stream = if stdout.truncated {
            OwnedProcessOutputStream::Stdout
        } else {
            OwnedProcessOutputStream::Stderr
        };
        let mut stdout = stdout.bytes;
        let mut stderr = stderr.bytes;
        stdout.truncate(MAX_OVERFLOW_DIAGNOSTIC_BYTES);
        stderr.truncate(MAX_OVERFLOW_DIAGNOSTIC_BYTES);
        return Err(SupervisedExecutionError::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        });
    }
    if !stdout.complete || !stderr.complete {
        let stream = if !stdout.complete { "stdout" } else { "stderr" };
        return Err(SupervisedExecutionError::Failed {
            error: anyhow!("{label} did not finish capturing {stream}"),
            process_started: true,
        });
    }
    Ok((stdout.bytes, stderr.bytes))
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn supervised_failures_distinguish_spawn_from_post_spawn_errors() {
        let capture = || SupervisedCapture {
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        let spawn = supervised_execution_error(
            OwnedProcessTreeError::Start(std::io::Error::other("spawn failed")),
            "test command",
            capture(),
        );
        let await_error =
            supervised_execution_error(OwnedProcessTreeError::Await, "test command", capture());

        assert!(matches!(
            spawn,
            SupervisedExecutionError::Failed {
                process_started: false,
                ..
            }
        ));
        assert!(matches!(
            await_error,
            SupervisedExecutionError::Failed {
                process_started: true,
                ..
            }
        ));
    }

    #[test]
    fn overflow_diagnostics_are_bounded_even_when_the_drain_returns_more() {
        let error = complete_supervised_captures(
            Some(BoundedProcessOutput {
                bytes: vec![b'x'; MAX_OVERFLOW_DIAGNOSTIC_BYTES * 2],
                truncated: true,
                complete: true,
            }),
            Some(BoundedProcessOutput {
                bytes: b"sibling diagnostic".to_vec(),
                truncated: false,
                complete: true,
            }),
            "test command",
        )
        .unwrap_err();

        let SupervisedExecutionError::OutputLimitExceeded { stdout, stderr, .. } = error else {
            panic!("expected output-limit error");
        };
        assert_eq!(stdout.len(), MAX_OVERFLOW_DIAGNOSTIC_BYTES);
        assert_eq!(stderr, b"sibling diagnostic");
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhasePosition {
    current: NonZeroUsize,
    total: NonZeroUsize,
}

impl PhasePosition {
    pub(crate) const fn single() -> Self {
        Self {
            current: NonZeroUsize::MIN,
            total: NonZeroUsize::MIN,
        }
    }

    pub(crate) fn new(current: usize, total: usize) -> Option<Self> {
        let current = NonZeroUsize::new(current)?;
        let total = NonZeroUsize::new(total)?;
        (current <= total).then_some(Self { current, total })
    }

    pub(crate) const fn current(self) -> usize {
        self.current.get()
    }

    pub(crate) const fn total(self) -> usize {
        self.total.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ExecutionEvent<'a> {
    PhaseStarted {
        label: &'a str,
        position: PhasePosition,
    },
    Output {
        stream: ExecutionStream,
        bytes: &'a [u8],
    },
    Heartbeat {
        label: &'a str,
        elapsed: Duration,
    },
    PhaseFinished {
        label: &'a str,
        success: bool,
        elapsed: Duration,
    },
}

pub(crate) trait ExecutionObserver {
    fn event(&mut self, _event: ExecutionEvent<'_>) {}

    /// Delivers events buffered since the previous successful flush.
    ///
    /// A later flush must not redeliver events that this call delivered.
    fn flush(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

pub(crate) trait ExecutionCancellation {
    fn cancelled(&self) -> bool {
        false
    }
}

pub(crate) trait ExecutionControl: ExecutionObserver + ExecutionCancellation {}

impl<T> ExecutionControl for T where T: ExecutionObserver + ExecutionCancellation + ?Sized {}

pub(crate) struct AdditionalCancellationControl<'a> {
    control: &'a mut dyn ExecutionControl,
    additional_cancelled: &'a dyn Fn() -> bool,
}

impl<'a> AdditionalCancellationControl<'a> {
    pub(crate) fn new(
        control: &'a mut dyn ExecutionControl,
        additional_cancelled: &'a dyn Fn() -> bool,
    ) -> Self {
        Self {
            control,
            additional_cancelled,
        }
    }
}

impl ExecutionObserver for AdditionalCancellationControl<'_> {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        self.control.event(event);
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.control.flush()
    }
}

impl ExecutionCancellation for AdditionalCancellationControl<'_> {
    fn cancelled(&self) -> bool {
        self.control.cancelled() || (self.additional_cancelled)()
    }
}

pub(crate) struct NoopExecutionObserver;

impl ExecutionObserver for NoopExecutionObserver {}
impl ExecutionCancellation for NoopExecutionObserver {}

pub(crate) struct ExecutionPhase<'a> {
    label: &'a str,
    started: Instant,
}

pub(crate) struct CompletedExecutionPhase {
    label: String,
    elapsed: Duration,
}

impl<'a> ExecutionPhase<'a> {
    pub(crate) fn start<O: ExecutionObserver + ?Sized>(
        observer: &mut O,
        label: &'a str,
        position: PhasePosition,
    ) -> Self {
        let started = Instant::now();
        observer.event(ExecutionEvent::PhaseStarted { label, position });
        Self { label, started }
    }

    pub(crate) fn finish<O: ExecutionObserver + ?Sized>(self, observer: &mut O, success: bool) {
        observer.event(ExecutionEvent::PhaseFinished {
            label: self.label,
            success,
            elapsed: self.started.elapsed(),
        });
    }

    pub(crate) fn complete_owned(self) -> CompletedExecutionPhase {
        CompletedExecutionPhase {
            label: self.label.to_owned(),
            elapsed: self.started.elapsed(),
        }
    }
}

impl CompletedExecutionPhase {
    pub(crate) fn finish<O: ExecutionObserver + ?Sized>(self, observer: &mut O, success: bool) {
        observer.event(ExecutionEvent::PhaseFinished {
            label: &self.label,
            success,
            elapsed: self.elapsed,
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HeartbeatSchedule {
    next: Option<Duration>,
}

impl HeartbeatSchedule {
    pub(crate) const fn new() -> Self {
        Self {
            next: Some(HEARTBEAT_INTERVAL),
        }
    }

    pub(crate) fn due(&mut self, elapsed: Duration) -> bool {
        let Some(next) = self.next else {
            return false;
        };
        if elapsed < next {
            return false;
        }
        let interval_seconds = HEARTBEAT_INTERVAL.as_secs();
        self.next = elapsed
            .as_secs()
            .checked_div(interval_seconds)
            .and_then(|intervals| intervals.checked_add(1))
            .and_then(|intervals| intervals.checked_mul(interval_seconds))
            .map(Duration::from_secs);
        true
    }
}

pub(crate) struct ProcessExecutionObserver<'a> {
    control: &'a mut dyn ExecutionControl,
    label: &'a str,
    heartbeat: HeartbeatSchedule,
}

struct SupervisedCapture {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

struct CapturingProcessExecutionObserver<'a> {
    inner: ProcessExecutionObserver<'a>,
    limit: usize,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl<'a> CapturingProcessExecutionObserver<'a> {
    fn new(control: &'a mut dyn ExecutionControl, label: &'a str, limit: usize) -> Self {
        Self {
            inner: ProcessExecutionObserver::new(control, label),
            limit,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn into_capture(self) -> SupervisedCapture {
        SupervisedCapture {
            stdout: self.stdout,
            stderr: self.stderr,
        }
    }
}

impl OwnedProcessObserver for CapturingProcessExecutionObserver<'_> {
    fn cancelled(&mut self) -> bool {
        self.inner.cancelled()
    }

    fn output(&mut self, stream: OwnedProcessOutputStream, bytes: &[u8]) {
        let destination = match stream {
            OwnedProcessOutputStream::Stdout => &mut self.stdout,
            OwnedProcessOutputStream::Stderr => &mut self.stderr,
        };
        let retained = self
            .limit
            .saturating_sub(destination.len())
            .min(bytes.len());
        destination.extend_from_slice(&bytes[..retained]);
        self.inner.output(stream, bytes);
    }

    fn poll(&mut self, elapsed: Duration) {
        self.inner.poll(elapsed);
    }
}

impl<'a> ProcessExecutionObserver<'a> {
    pub(crate) fn new(control: &'a mut dyn ExecutionControl, label: &'a str) -> Self {
        Self {
            control,
            label,
            heartbeat: HeartbeatSchedule::new(),
        }
    }
}

impl OwnedProcessObserver for ProcessExecutionObserver<'_> {
    fn cancelled(&mut self) -> bool {
        self.control.cancelled()
    }

    fn output(&mut self, stream: OwnedProcessOutputStream, bytes: &[u8]) {
        let stream = match stream {
            OwnedProcessOutputStream::Stdout => ExecutionStream::Stdout,
            OwnedProcessOutputStream::Stderr => ExecutionStream::Stderr,
        };
        self.control.event(ExecutionEvent::Output { stream, bytes });
    }

    fn poll(&mut self, elapsed: Duration) {
        if !self.heartbeat.due(elapsed) {
            return;
        }
        self.control.event(ExecutionEvent::Heartbeat {
            label: self.label,
            elapsed,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct CancelledControl;

    impl ExecutionObserver for CancelledControl {}

    impl ExecutionCancellation for CancelledControl {
        fn cancelled(&self) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct RecordingControl {
        event_count: usize,
        flush_count: usize,
        cancelled: bool,
        fail_flush: bool,
    }

    impl ExecutionObserver for RecordingControl {
        fn event(&mut self, _event: ExecutionEvent<'_>) {
            self.event_count += 1;
        }

        fn flush(&mut self) -> anyhow::Result<()> {
            self.flush_count += 1;
            if self.fail_flush {
                return Err(anyhow!("injected flush failure"));
            }
            Ok(())
        }
    }

    impl ExecutionCancellation for RecordingControl {
        fn cancelled(&self) -> bool {
            self.cancelled
        }
    }

    #[test]
    fn additional_cancellation_control_preserves_the_observer_contract() {
        let additional_cancelled = Cell::new(false);
        let mut control = RecordingControl::default();
        {
            let additional = || additional_cancelled.get();
            let mut wrapper = AdditionalCancellationControl::new(&mut control, &additional);
            wrapper.event(ExecutionEvent::Heartbeat {
                label: "test",
                elapsed: Duration::ZERO,
            });
            wrapper.flush().unwrap();
            assert!(!wrapper.cancelled());
            additional_cancelled.set(true);
            assert!(wrapper.cancelled());
        }
        assert_eq!(control.event_count, 1);
        assert_eq!(control.flush_count, 1);

        control.cancelled = true;
        control.fail_flush = true;
        additional_cancelled.set(false);
        let additional = || additional_cancelled.get();
        let mut wrapper = AdditionalCancellationControl::new(&mut control, &additional);
        assert!(wrapper.cancelled());
        assert_eq!(
            wrapper.flush().unwrap_err().to_string(),
            "injected flush failure"
        );
    }

    #[test]
    fn phase_position_rejects_impossible_progress() {
        assert!(PhasePosition::new(0, 1).is_none());
        assert!(PhasePosition::new(1, 0).is_none());
        assert!(PhasePosition::new(2, 1).is_none());
        assert_eq!(PhasePosition::new(1, 2).unwrap().current(), 1);
    }

    #[test]
    fn heartbeat_schedule_advances_past_delayed_polls() {
        let mut schedule = HeartbeatSchedule::new();
        assert!(!schedule.due(HEARTBEAT_INTERVAL - Duration::from_millis(1)));
        assert!(schedule.due(HEARTBEAT_INTERVAL * 3));
        assert!(!schedule.due(HEARTBEAT_INTERVAL * 3));
        assert!(schedule.due(HEARTBEAT_INTERVAL * 4));
        assert!(schedule.due(Duration::MAX));
        assert!(!schedule.due(Duration::MAX));
    }

    #[test]
    fn process_observation_delegates_to_the_independent_cancellation_source() {
        let mut control = CancelledControl;
        let mut observer = ProcessExecutionObserver::new(&mut control, "test");

        assert!(OwnedProcessObserver::cancelled(&mut observer));
    }

    #[test]
    fn supervised_commands_override_inherited_stdin_and_capture_policy() {
        let mut command = Command::new("bash");
        command
            .args(["-c", "read -r ignored || true; printf supervised"])
            .stdin(Stdio::piped());
        let mut observer = NoopExecutionObserver;

        let output = run_supervised_execution_command(
            &mut command,
            Duration::from_secs(1),
            CommandOutputLimit::from_bytes(1024).unwrap(),
            "stdin policy test",
            &mut observer,
        )
        .unwrap();

        assert!(output.status.success());
        assert_eq!(output.stdout, b"supervised");
    }

    #[test]
    fn supervised_commands_fail_when_output_exceeds_the_limit() {
        let mut command = Command::new("bash");
        command.args(["-c", "printf 12345"]);
        let mut observer = NoopExecutionObserver;

        let error = run_supervised_execution_command(
            &mut command,
            Duration::from_secs(1),
            CommandOutputLimit::from_bytes(4).unwrap(),
            "output policy test",
            &mut observer,
        )
        .unwrap_err();

        let SupervisedExecutionError::OutputLimitExceeded {
            stream,
            stdout,
            stderr,
        } = error
        else {
            panic!("expected output overflow");
        };
        assert_eq!(stream, OwnedProcessOutputStream::Stdout);
        assert_eq!(stdout, b"1234");
        assert!(stderr.is_empty());
    }
}
