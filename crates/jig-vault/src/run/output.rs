use std::io;
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use jig_owned_process::{ChildPipe, NonblockingPipe};
use zeroize::Zeroizing;

use crate::SecretBytes;

use super::{
    ACTIVE_OUTPUT_POLL_INTERVAL, MAX_CAPTURED_STREAM_BYTES, MAX_STREAM_BYTES_PER_POLL,
    MAX_STREAM_READS_PER_POLL, checked_deadline,
};

struct CappedOutputDrain {
    label: &'static str,
    reader: Option<NonblockingPipe>,
    output: SecretBytes,
    complete: bool,
}

impl CappedOutputDrain {
    fn start(label: &'static str, reader: ChildPipe) -> AnyResult<Self> {
        let reader = reader
            .prepare("nonblocking brokered process-pipe reads are unavailable on this platform")
            .with_context(|| format!("failed to prepare brokered command {label} capture"))?;
        Ok(Self {
            label,
            reader: Some(reader),
            // Allocate the full cap up front so secret-bearing bytes never
            // pass through discarded intermediate Vec allocations.
            output: SecretBytes::with_capacity(MAX_CAPTURED_STREAM_BYTES),
            complete: false,
        })
    }

    fn poll(&mut self) -> AnyResult<bool> {
        let Some(reader) = self.reader.as_mut() else {
            return Ok(false);
        };
        let mut buffer = Zeroizing::new([0_u8; 8192]);
        let mut made_progress = false;
        let mut bytes_read = 0_usize;
        for _ in 0..MAX_STREAM_READS_PER_POLL {
            if bytes_read >= MAX_STREAM_BYTES_PER_POLL {
                return Ok(true);
            }
            let remaining_poll_bytes = MAX_STREAM_BYTES_PER_POLL - bytes_read;
            let read_limit = buffer.len().min(remaining_poll_bytes);
            debug_assert!(read_limit > 0);
            match reader.read_available(&mut buffer[..read_limit]) {
                Ok(0) => {
                    self.complete = true;
                    self.reader = None;
                    return Ok(made_progress);
                }
                Ok(read) => {
                    made_progress = true;
                    let Some(new_len) = self.output.len().checked_add(read) else {
                        self.reader = None;
                        bail!("brokered command {} capture length overflowed", self.label);
                    };
                    if new_len > MAX_CAPTURED_STREAM_BYTES {
                        let remaining = MAX_CAPTURED_STREAM_BYTES - self.output.len();
                        self.output.extend_from_slice(&buffer[..remaining])?;
                        self.reader = None;
                        bail!(
                            "brokered command {} exceeded the {} byte capture limit",
                            self.label,
                            MAX_CAPTURED_STREAM_BYTES
                        );
                    }
                    self.output.extend_from_slice(&buffer[..read])?;
                    bytes_read += read;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(made_progress);
                }
                Err(error) => {
                    self.reader = None;
                    return Err(error).with_context(|| {
                        format!("failed to read brokered command {}", self.label)
                    });
                }
            }
        }
        Ok(made_progress)
    }

    const fn is_terminal(&self) -> bool {
        self.reader.is_none()
    }

    fn into_output(self) -> AnyResult<SecretBytes> {
        if !self.complete {
            bail!(
                "brokered command {} capture remained open after process cleanup",
                self.label
            );
        }
        Ok(self.output)
    }
}

pub(super) struct CappedOutputDrains {
    stdout: CappedOutputDrain,
    stderr: CappedOutputDrain,
}

impl CappedOutputDrains {
    pub(super) fn start(child: &mut Child) -> AnyResult<Self> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("brokered command stdout pipe was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("brokered command stderr pipe was not captured"))?;
        Ok(Self {
            stdout: CappedOutputDrain::start("stdout", ChildPipe::Stdout(stdout))?,
            stderr: CappedOutputDrain::start("stderr", ChildPipe::Stderr(stderr))?,
        })
    }

    pub(super) fn poll(&mut self) -> AnyResult<bool> {
        let stdout_progress = self.stdout.poll()?;
        let stderr_progress = self.stderr.poll()?;
        Ok(stdout_progress || stderr_progress)
    }

    pub(super) const fn is_terminal(&self) -> bool {
        self.stdout.is_terminal() && self.stderr.is_terminal()
    }

    pub(super) fn finish(mut self, timeout: Duration) -> AnyResult<(SecretBytes, SecretBytes)> {
        let deadline = checked_deadline("brokered output drain", timeout)?;
        while !self.is_terminal() {
            let made_progress = self.poll()?;
            if self.is_terminal() {
                break;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                bail!("brokered command output drain exceeded its {timeout:?} deadline");
            };
            if made_progress {
                thread::sleep(remaining.min(ACTIVE_OUTPUT_POLL_INTERVAL));
            } else {
                thread::sleep(remaining.min(Duration::from_millis(2)));
            }
        }
        Ok((self.stdout.into_output()?, self.stderr.into_output()?))
    }
}
