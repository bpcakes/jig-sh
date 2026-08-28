#![cfg(unix)]

use std::{
    fs::File,
    io::Read,
    ops::{Deref, DerefMut},
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

pub struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    pub fn new(child: Child) -> Self {
        Self { child }
    }
}

impl Deref for ChildGuard {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

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
