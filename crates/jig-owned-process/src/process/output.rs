use std::{process::Child, time::Duration};

use super::{
    ACTIVE_OUTPUT_POLL_INTERVAL, BoundedProcessOutput, MAX_OUTPUT_READS_PER_POLL,
    MAX_OUTPUT_READS_PER_POLL_AFTER_TRUNCATION, ProcessOutputLimits, ProcessPipe,
    TRUNCATED_OUTPUT_POLL_INTERVAL,
};

pub(super) struct OutputDrain {
    reader: Option<ProcessPipe>,
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
    complete: bool,
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

    pub(super) fn poll(&mut self) -> bool {
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

    pub(super) fn poll(&mut self) -> bool {
        let stdout_progress = self.stdout.as_mut().is_some_and(OutputDrain::poll);
        let stderr_progress = self.stderr.as_mut().is_some_and(OutputDrain::poll);
        stdout_progress || stderr_progress
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
    ) -> (Option<BoundedProcessOutput>, Option<BoundedProcessOutput>) {
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(std::time::Instant::now);
        while !self.is_terminal() && std::time::Instant::now() < deadline {
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
