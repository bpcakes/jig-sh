use std::io::{self, Read};
use std::process::{ChildStderr, ChildStdout};

/// An owned child output stream before nonblocking preparation.
///
/// Dropping the stream closes its reader; process ownership remains with the caller.
pub enum ChildPipe {
    Stdout(ChildStdout),
    Stderr(ChildStderr),
}

/// A child output stream whose reads cannot block waiting for more bytes.
///
/// This adapter retains no captured bytes. Callers own buffer allocation,
/// zeroization, output limits, and process cleanup.
pub struct NonblockingPipe {
    reader: ChildPipe,
}

impl ChildPipe {
    /// Prepare the owned reader, closing it if preparation fails.
    ///
    /// Targets without nonblocking pipe support return `Unsupported` with the
    /// caller's diagnostic. A successful result is required before reading.
    pub fn prepare(self, unsupported_message: &'static str) -> io::Result<NonblockingPipe> {
        self.set_nonblocking(unsupported_message)?;
        Ok(NonblockingPipe { reader: self })
    }

    #[cfg(unix)]
    fn set_nonblocking(&self, _unsupported_message: &'static str) -> io::Result<()> {
        use std::os::fd::AsFd;

        let descriptor = match self {
            Self::Stdout(reader) => reader.as_fd(),
            Self::Stderr(reader) => reader.as_fd(),
        };
        crate::unix::set_nonblocking(descriptor)
    }

    #[cfg(not(unix))]
    fn set_nonblocking(&self, unsupported_message: &'static str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            unsupported_message,
        ))
    }
}

impl NonblockingPipe {
    /// Read immediately available bytes; return `WouldBlock` while the stream
    /// remains open without data, and zero on EOF for a nonempty buffer.
    pub fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.reader {
            ChildPipe::Stdout(reader) => reader.read(buffer),
            ChildPipe::Stderr(reader) => reader.read(buffer),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::Write;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn prepared_output_streams_distinguish_would_block_bytes_and_eof() {
        for stdout in [true, false] {
            let (reader, mut writer) = UnixStream::pair().unwrap();
            let descriptor = OwnedFd::from(reader);
            let stream = if stdout {
                ChildPipe::Stdout(ChildStdout::from(descriptor))
            } else {
                ChildPipe::Stderr(ChildStderr::from(descriptor))
            };
            let mut pipe = stream.prepare("unsupported test pipe").unwrap();
            let mut buffer = [0; 8];
            assert_eq!(
                pipe.read_available(&mut buffer).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            writer.write_all(b"example").unwrap();
            assert_eq!(pipe.read_available(&mut buffer).unwrap(), 7);
            assert_eq!(&buffer[..7], b"example");
            assert_eq!(
                pipe.read_available(&mut buffer).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            drop(writer);
            assert_eq!(pipe.read_available(&mut buffer).unwrap(), 0);
        }
    }
}
