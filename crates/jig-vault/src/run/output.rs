use std::io::{self, Read};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStderr, ChildStdout};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use zeroize::Zeroizing;

use crate::SecretBytes;

use super::{
    ACTIVE_OUTPUT_POLL_INTERVAL, MAX_CAPTURED_STREAM_BYTES, MAX_STREAM_BYTES_PER_POLL,
    MAX_STREAM_READS_PER_POLL, checked_deadline,
};

enum ProcessPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

impl ProcessPipe {
    #[cfg(unix)]
    fn prepare(&self) -> io::Result<()> {
        let descriptor = match self {
            Self::Stdout(reader) => reader.as_raw_fd(),
            Self::Stderr(reader) => reader.as_raw_fd(),
        };
        // SAFETY: the descriptor is owned by this live pipe. F_GETFL only
        // inspects its current status flags.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the descriptor remains live and F_SETFL preserves all
        // existing flags while adding nonblocking reads.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(windows)]
    fn prepare(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn prepare(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "nonblocking brokered process-pipe reads are unavailable on this platform",
        ))
    }

    #[cfg(unix)]
    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(reader) => reader.read(buffer),
            Self::Stderr(reader) => reader.read(buffer),
        }
    }

    #[cfg(windows)]
    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, HANDLE};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        let handle = match self {
            Self::Stdout(reader) => reader.as_raw_handle(),
            Self::Stderr(reader) => reader.as_raw_handle(),
        } as HANDLE;
        let mut available = 0_u32;
        // SAFETY: handle is a live anonymous-pipe read handle and the only
        // output pointer names a writable u32. This call copies no bytes.
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
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(code) if code == ERROR_BROKEN_PIPE as i32 || code == ERROR_NO_DATA as i32
            ) {
                return Ok(0);
            }
            return Err(error);
        }
        if available == 0 {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let read_limit = buffer.len().min(available as usize);
        match self {
            Self::Stdout(reader) => reader.read(&mut buffer[..read_limit]),
            Self::Stderr(reader) => reader.read(&mut buffer[..read_limit]),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn read_available(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "nonblocking brokered process-pipe reads are unavailable on this platform",
        ))
    }
}

struct CappedOutputDrain {
    label: &'static str,
    reader: Option<ProcessPipe>,
    output: SecretBytes,
    complete: bool,
}

impl CappedOutputDrain {
    fn start(label: &'static str, reader: ProcessPipe) -> AnyResult<Self> {
        reader
            .prepare()
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
            stdout: CappedOutputDrain::start("stdout", ProcessPipe::Stdout(stdout))?,
            stderr: CappedOutputDrain::start("stderr", ProcessPipe::Stderr(stderr))?,
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
