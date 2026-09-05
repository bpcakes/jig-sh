#![cfg(unix)]

use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::fd::{AsRawFd, FromRawFd},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use jig_tui::TerminalSession;

const DIRECT_OUTPUT_MARKER: &str = "panic-direct-output-marker";

#[test]
fn panic_unwind_clears_and_restores_the_terminal_session() {
    if std::env::var_os("JIG_TUI_PANIC_CHILD").is_some() {
        let mut terminal =
            TerminalSession::enter_with_bracketed_paste("panic lifecycle test").unwrap();
        terminal
            .with_direct_output(|output| {
                output.write_all(DIRECT_OUTPUT_MARKER.as_bytes())?;
                output.flush()
            })
            .unwrap();
        panic!("intentional terminal-session unwind");
    }

    let (mut master, slave) = pseudo_terminal(80, 24).unwrap();
    let original = terminal_attributes(&slave);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "panic_unwind_clears_and_restores_the_terminal_session",
            "--nocapture",
        ])
        .env("JIG_TUI_PANIC_CHILD", "1")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()))
        .spawn()
        .unwrap();
    set_nonblocking(&master);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    let status = loop {
        read_available(&mut master, &mut output);
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "panic child did not exit; output: {}",
            String::from_utf8_lossy(&output)
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    read_available(&mut master, &mut output);

    assert!(
        !status.success(),
        "intentional panic unexpectedly succeeded"
    );
    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("\u{1b}[?1049h"),
        "alternate screen not entered"
    );
    let marker = output
        .find(DIRECT_OUTPUT_MARKER)
        .expect("direct output was not emitted before the panic");
    assert!(
        output[marker + DIRECT_OUTPUT_MARKER.len()..].contains("\u{1b}[2J"),
        "direct output was not cleared during panic unwind"
    );
    assert!(
        output.contains("\u{1b}[?1049l"),
        "alternate screen not left"
    );
    assert!(output.contains("\u{1b}[?2004l"), "paste mode not disabled");
    let restored = terminal_attributes(&slave);
    assert_eq!(
        restored.c_lflag & (libc::ECHO | libc::ICANON),
        original.c_lflag & (libc::ECHO | libc::ICANON),
        "terminal flags were not restored during panic unwind"
    );
}

#[test]
fn startup_output_failure_restores_terminal_attributes() {
    if std::env::var_os("JIG_TUI_STARTUP_FAILURE_CHILD").is_some() {
        let full = OpenOptions::new().write(true).open("/dev/full").unwrap();
        // SAFETY: this isolated child saves and restores its own stdout around
        // the setup call, and closes the saved duplicate exactly once.
        let saved_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
        assert!(saved_stdout >= 0);
        // SAFETY: both descriptors are live and owned by this child.
        assert!(unsafe { libc::dup2(full.as_raw_fd(), libc::STDOUT_FILENO) } >= 0);
        let result = TerminalSession::enter_with_bracketed_paste("startup failure test");
        // SAFETY: restore stdout for the test harness, then close the duplicate.
        assert!(unsafe { libc::dup2(saved_stdout, libc::STDOUT_FILENO) } >= 0);
        // SAFETY: `saved_stdout` is no longer needed after the successful dup2.
        unsafe { libc::close(saved_stdout) };
        let error = result
            .err()
            .expect("failing stdout unexpectedly accepted terminal setup");
        assert!(error.to_string().contains("failed to enter"));
        return;
    }

    let (_master, slave) = pseudo_terminal(80, 24).unwrap();
    let original = terminal_attributes(&slave);

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "startup_output_failure_restores_terminal_attributes",
            "--nocapture",
        ])
        .env("JIG_TUI_STARTUP_FAILURE_CHILD", "1")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "startup-failure child hung");
        std::thread::sleep(Duration::from_millis(10));
    };

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        status.success(),
        "startup-failure child failed: {status}: {stderr}"
    );
    let restored = terminal_attributes(&slave);
    assert_eq!(
        restored.c_lflag & (libc::ECHO | libc::ICANON),
        original.c_lflag & (libc::ECHO | libc::ICANON),
        "terminal flags were not restored after startup failure"
    );
}

fn pseudo_terminal(columns: u16, rows: u16) -> std::io::Result<(File, File)> {
    // SAFETY: each successful descriptor is immediately wrapped exactly once.
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `master` is a newly owned descriptor.
    let master = unsafe { File::from_raw_fd(master) };
    // SAFETY: the descriptor is a live PTY master owned by `master`.
    if unsafe { libc::grantpt(master.as_raw_fd()) } != 0
        || unsafe { libc::unlockpt(master.as_raw_fd()) } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: libc owns the NUL-terminated path for this live PTY master.
    let name = unsafe { libc::ptsname(master.as_raw_fd()) };
    if name.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `name` is the live PTY slave path returned above.
    let slave = unsafe { libc::open(name, libc::O_RDWR | libc::O_NOCTTY) };
    if slave < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `slave` is a newly owned descriptor.
    let slave = unsafe { File::from_raw_fd(slave) };
    resize_terminal(&slave, columns, rows);
    Ok((master, slave))
}

fn resize_terminal(slave: &File, columns: u16, rows: u16) {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `slave` is live and `size` points to initialized storage.
    assert_eq!(
        unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &size) },
        0
    );
}

fn terminal_attributes(slave: &File) -> libc::termios {
    // SAFETY: zero is a valid initial representation and tcgetattr fills it.
    let mut attributes = unsafe { std::mem::zeroed::<libc::termios>() };
    // SAFETY: `slave` is live and `attributes` is writable.
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut attributes) },
        0
    );
    attributes
}

fn set_nonblocking(file: &File) {
    // SAFETY: fcntl reads and updates flags for this live descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    // SAFETY: the descriptor and flag combination are valid.
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
}

fn read_available(file: &mut File, output: &mut Vec<u8>) {
    let mut buffer = [0_u8; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return,
            Err(error) if cfg!(target_os = "linux") && error.raw_os_error() == Some(libc::EIO) => {
                return;
            }
            Err(error) => panic!("reading PTY failed: {error}"),
        }
    }
}
