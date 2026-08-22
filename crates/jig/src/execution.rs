use std::num::NonZeroUsize;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use anyhow::anyhow;
use jig_owned_process::{
    BoundedProcessOutput, OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError,
    ProcessOutputLimits, ProcessOutputOverflowPolicy,
    run_owned_process_tree_with_output_policy_and_observer,
};

use crate::context::CommandTimeout;

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
pub(crate) const EXECUTION_OUTPUT_CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

pub(crate) struct ExecutionCommandOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum ExecutionCommandError {
    CancelledBeforeStart,
    Cancelled,
    Failed(anyhow::Error),
}

impl ExecutionCommandError {
    pub(crate) fn failed(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed(error.into())
    }

    #[cfg(test)]
    pub(crate) fn into_anyhow(self) -> anyhow::Error {
        match self {
            Self::CancelledBeforeStart => anyhow!("Execution was cancelled before it started"),
            Self::Cancelled => anyhow!("Execution was cancelled"),
            Self::Failed(error) => error,
        }
    }
}

pub(crate) fn run_authoritative_execution_command(
    command: &mut Command,
    timeout: CommandTimeout,
    label: &str,
    observer: &mut dyn ExecutionControl,
) -> Result<ExecutionCommandOutput, ExecutionCommandError> {
    let output = run_owned_process_tree_with_output_policy_and_observer(
        command,
        timeout.duration(),
        ProcessOutputLimits {
            stdout: EXECUTION_OUTPUT_CAPTURE_LIMIT,
            stderr: EXECUTION_OUTPUT_CAPTURE_LIMIT,
        },
        ProcessOutputOverflowPolicy::Error,
        &mut ProcessExecutionObserver::new(observer, label),
    )
    .map_err(|error| execution_command_error(error, timeout, label))?;

    Ok(ExecutionCommandOutput {
        status: output.status,
        stdout: complete_execution_capture(output.stdout, label, "stdout")?,
        stderr: complete_execution_capture(output.stderr, label, "stderr")?,
    })
}

fn execution_command_error(
    error: OwnedProcessTreeError,
    timeout: CommandTimeout,
    label: &str,
) -> ExecutionCommandError {
    let error = match error {
        OwnedProcessTreeError::CancelledBeforeStart => {
            return ExecutionCommandError::CancelledBeforeStart;
        }
        OwnedProcessTreeError::Cancelled => return ExecutionCommandError::Cancelled,
        OwnedProcessTreeError::Start(error) => anyhow!("{label} could not start: {error}"),
        OwnedProcessTreeError::TimedOut => {
            anyhow!("{label} timed out after {} seconds", timeout.as_secs())
        }
        OwnedProcessTreeError::OutputLimitExceeded(stream) => anyhow!(
            "{label} exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte {stream} capture limit"
        ),
        OwnedProcessTreeError::Await => anyhow!("{label} could not be awaited"),
        OwnedProcessTreeError::Cleanup => {
            anyhow!("{label} process tree could not be cleaned up safely")
        }
    };
    ExecutionCommandError::Failed(error)
}

fn complete_execution_capture(
    output: Option<BoundedProcessOutput>,
    label: &str,
    stream: &str,
) -> Result<Vec<u8>, ExecutionCommandError> {
    let output = output.ok_or_else(|| {
        ExecutionCommandError::failed(anyhow!("{label} did not capture {stream}"))
    })?;
    if output.truncated {
        return Err(ExecutionCommandError::failed(anyhow!(
            "{label} exceeded the {EXECUTION_OUTPUT_CAPTURE_LIMIT} byte {stream} capture limit"
        )));
    }
    if !output.complete {
        return Err(ExecutionCommandError::failed(anyhow!(
            "{label} did not finish capturing {stream}"
        )));
    }
    Ok(output.bytes)
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
}

pub(crate) trait ExecutionCancellation {
    fn cancelled(&self) -> bool {
        false
    }
}

pub(crate) trait ExecutionControl: ExecutionObserver + ExecutionCancellation {}

impl<T> ExecutionControl for T where T: ExecutionObserver + ExecutionCancellation + ?Sized {}

pub(crate) struct NoopExecutionObserver;

impl ExecutionObserver for NoopExecutionObserver {}
impl ExecutionCancellation for NoopExecutionObserver {}

pub(crate) struct ExecutionPhase<'a> {
    label: &'a str,
    started: Instant,
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
    use super::*;

    struct CancelledControl;

    impl ExecutionObserver for CancelledControl {}

    impl ExecutionCancellation for CancelledControl {
        fn cancelled(&self) -> bool {
            true
        }
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
}
