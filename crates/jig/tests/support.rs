use std::io;

#[cfg(unix)]
use std::{
    fs::File,
    io::Read,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn tempdir() -> io::Result<TempDir> {
    #[cfg(unix)]
    {
        use std::sync::Once;

        static TEST_UMASK: Once = Once::new();
        TEST_UMASK.call_once(|| {
            // SAFETY: this is test-only process setup; the test process exits
            // after the suite, so the private umask does not escape to callers.
            unsafe { libc::umask(0o077) };
        });

        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
    }

    #[cfg(not(unix))]
    {
        tempfile::tempdir()
    }
}

#[cfg(unix)]
pub fn read_pty_available(file: &mut File, output: &mut Vec<u8>) {
    let mut buffer = [0_u8; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
            Err(error) if pty_master_reached_eof(&error) => return,
            Err(error) => panic!("reading PTY output failed: {error}"),
        }
    }
}

#[cfg(unix)]
pub fn wait_for_child_while_draining(
    child: &mut Child,
    master: &mut File,
    output: &mut Vec<u8>,
    timeout: Duration,
) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        read_pty_available(master, output);
        if let Some(status) = child.try_wait().unwrap() {
            read_pty_available(master, output);
            return Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            read_pty_available(master, output);
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
fn pty_master_reached_eof(error: &io::Error) -> bool {
    cfg!(target_os = "linux") && error.raw_os_error() == Some(libc::EIO)
}
