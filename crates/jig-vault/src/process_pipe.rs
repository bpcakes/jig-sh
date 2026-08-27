use std::io::{self, Read};
#[cfg(unix)]
use std::os::fd::AsFd;
use std::process::{ChildStderr, ChildStdout};

pub(crate) enum ProcessPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

impl ProcessPipe {
    #[cfg(unix)]
    pub(crate) fn prepare(&self, _unsupported_message: &'static str) -> io::Result<()> {
        let descriptor = match self {
            Self::Stdout(reader) => reader.as_fd(),
            Self::Stderr(reader) => reader.as_fd(),
        };
        jig_owned_process::unix::set_nonblocking(descriptor)
    }

    #[cfg(not(unix))]
    pub(crate) fn prepare(&self, unsupported_message: &'static str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            unsupported_message,
        ))
    }

    pub(crate) fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(reader) => reader.read(buffer),
            Self::Stderr(reader) => reader.read(buffer),
        }
    }
}
