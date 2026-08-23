use std::{process::Child, time::Duration};

use super::{
    ACTIVE_OUTPUT_POLL_INTERVAL, BoundedProcessOutput, MAX_OUTPUT_READS_PER_POLL,
    MAX_OUTPUT_READS_PER_POLL_AFTER_TRUNCATION, OwnedProcessObserver, OwnedProcessOutputStream,
    ProcessOutputLimits, ProcessPipe, TRUNCATED_OUTPUT_POLL_INTERVAL,
};

pub(super) struct OutputDrain {
    reader: Option<ProcessPipe>,
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
    complete: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct OutputPoll {
    pub(super) made_progress: bool,
    pub(super) overflow: Option<OwnedProcessOutputStream>,
}

impl OutputDrain {
    pub(super) fn start(reader: ProcessPipe, limit: usize) -> std::io::Result<Self> {
        reader.prepare()?;
        Ok(Self {
            reader: Some(reader),
            bytes: Vec::new(),
            limit,
            truncated: false,
            complete: false,
        })
    }

    pub(super) fn poll(
        &mut self,
        stream: OwnedProcessOutputStream,
        observer: &mut dyn OwnedProcessObserver,
    ) -> OutputPoll {
        // A shell can issue thousands of tiny writes. Bound every poll to 64
        // read attempts while the retained output is capped separately. Once
        // capture truncates, tighten the same attempt budget to 16 so repeated
        // short reads and interrupts remain bounded alike.
        let Some(reader) = self.reader.as_mut() else {
            return OutputPoll::default();
        };
        let mut chunk = [0_u8; 4096];
        let mut poll = OutputPoll::default();
        for read_index in 0..MAX_OUTPUT_READS_PER_POLL {
            if self.truncated && read_index >= MAX_OUTPUT_READS_PER_POLL_AFTER_TRUNCATION {
                return poll;
            }
            match reader.read_available(&mut chunk) {
                Ok(0) => {
                    self.complete = true;
                    self.reader = None;
                    return poll;
                }
                Ok(read) => {
                    poll.made_progress = true;
                    observer.output(stream, &chunk[..read]);
                    let remaining = self.limit.saturating_sub(self.bytes.len());
                    let retained = remaining.min(read);
                    self.bytes.extend_from_slice(&chunk[..retained]);
                    if retained < read && !self.truncated {
                        poll.overflow = Some(stream);
                    }
                    self.truncated |= retained < read;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return poll;
                }
                Err(_) => {
                    // Closing the reader makes an I/O failure terminal and
                    // records the capture as incomplete without retaining a
                    // blocked worker or retry loop.
                    self.reader = None;
                    return poll;
                }
            }
        }
        poll
    }

    const fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    pub(super) fn finish(self) -> BoundedProcessOutput {
        BoundedProcessOutput {
            bytes: self.bytes,
            truncated: self.truncated,
            complete: self.complete,
        }
    }
}

pub(super) struct OwnedProcessOutputDrains {
    stdout: Option<OutputDrain>,
    stderr: Option<OutputDrain>,
}

impl OwnedProcessOutputDrains {
    pub(super) fn start(child: &mut Child, limits: ProcessOutputLimits) -> std::io::Result<Self> {
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

    pub(super) fn poll(&mut self, observer: &mut dyn OwnedProcessObserver) -> OutputPoll {
        let stdout_poll = self
            .stdout
            .as_mut()
            .map_or_else(OutputPoll::default, |drain| {
                drain.poll(OwnedProcessOutputStream::Stdout, observer)
            });
        let stderr_poll = self
            .stderr
            .as_mut()
            .map_or_else(OutputPoll::default, |drain| {
                drain.poll(OwnedProcessOutputStream::Stderr, observer)
            });
        OutputPoll {
            made_progress: stdout_poll.made_progress || stderr_poll.made_progress,
            overflow: stdout_poll.overflow.or(stderr_poll.overflow),
        }
    }

    fn is_terminal(&self) -> bool {
        self.stdout.as_ref().is_none_or(OutputDrain::is_terminal)
            && self.stderr.as_ref().is_none_or(OutputDrain::is_terminal)
    }

    pub(super) fn active_poll_interval(&self) -> Duration {
        if self.stdout.as_ref().is_some_and(|drain| drain.truncated)
            || self.stderr.as_ref().is_some_and(|drain| drain.truncated)
        {
            TRUNCATED_OUTPUT_POLL_INTERVAL
        } else {
            ACTIVE_OUTPUT_POLL_INTERVAL
        }
    }

    pub(super) fn finish(
        mut self,
        timeout: Duration,
        observer: &mut dyn OwnedProcessObserver,
    ) -> (Option<BoundedProcessOutput>, Option<BoundedProcessOutput>) {
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(std::time::Instant::now);
        while !self.is_terminal() && std::time::Instant::now() < deadline {
            let made_progress = self.poll(observer).made_progress;
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
