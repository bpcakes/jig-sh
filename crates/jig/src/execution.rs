use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use jig_owned_process::{OwnedProcessObserver, OwnedProcessOutputStream};

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

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
