use std::time::Duration;

use jig_owned_process::{OwnedProcessObserver, OwnedProcessOutputStream};

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ExecutionEvent<'a> {
    PhaseStarted {
        label: &'a str,
        current: usize,
        total: usize,
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

    fn cancelled(&self) -> bool {
        false
    }
}

pub(crate) struct NoopExecutionObserver;

impl ExecutionObserver for NoopExecutionObserver {}

pub(crate) struct ProcessExecutionObserver<'a> {
    observer: &'a mut dyn ExecutionObserver,
    label: &'a str,
    next_heartbeat: Duration,
}

impl<'a> ProcessExecutionObserver<'a> {
    pub(crate) fn new(observer: &'a mut dyn ExecutionObserver, label: &'a str) -> Self {
        Self {
            observer,
            label,
            next_heartbeat: HEARTBEAT_INTERVAL,
        }
    }
}

impl OwnedProcessObserver for ProcessExecutionObserver<'_> {
    fn cancelled(&mut self) -> bool {
        self.observer.cancelled()
    }

    fn output(&mut self, stream: OwnedProcessOutputStream, bytes: &[u8]) {
        let stream = match stream {
            OwnedProcessOutputStream::Stdout => ExecutionStream::Stdout,
            OwnedProcessOutputStream::Stderr => ExecutionStream::Stderr,
        };
        self.observer
            .event(ExecutionEvent::Output { stream, bytes });
    }

    fn poll(&mut self, elapsed: Duration) {
        if elapsed < self.next_heartbeat {
            return;
        }
        self.observer.event(ExecutionEvent::Heartbeat {
            label: self.label,
            elapsed,
        });
        while self.next_heartbeat <= elapsed {
            self.next_heartbeat = self
                .next_heartbeat
                .checked_add(HEARTBEAT_INTERVAL)
                .unwrap_or(Duration::MAX);
        }
    }
}
