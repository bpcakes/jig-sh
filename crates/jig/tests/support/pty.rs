#![cfg(unix)]

use std::{
    fs::File,
    io::Read,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

pub fn read_available(file: &mut File, output: &mut Vec<u8>) {
    let mut buffer = [0_u8; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return,
            Err(error) if pty_master_reached_eof(&error) => return,
            Err(error) => panic!("reading PTY output failed: {error}"),
        }
    }
}

pub fn wait_for_child_while_draining(
    child: &mut Child,
    master: &mut File,
    output: &mut Vec<u8>,
    timeout: Duration,
) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        read_available(master, output);
        if let Some(status) = child.try_wait().unwrap() {
            read_available(master, output);
            return Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            read_available(master, output);
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pty_master_reached_eof(error: &std::io::Error) -> bool {
    cfg!(target_os = "linux") && error.raw_os_error() == Some(libc::EIO)
}
