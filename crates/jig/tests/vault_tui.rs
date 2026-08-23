#![cfg(unix)]

// Load PTY helpers only in their two consumers; `tests/shared` holds helpers
// that are intentionally separate from the general integration-test support module.
#[path = "shared/pty.rs"]
mod pty_support;
mod support;

use std::{
    fs::File,
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use jig_vault::{FieldKind, SecretBytes, Vault};
use secrecy::SecretString;

use pty_support::{ChildGuard, read_available, wait_for_child_while_draining};

const ALLOW_PTY_SKIP_ENV: &str = "JIG_ALLOW_PTY_TEST_SKIP";
const FULL_CLEAR_MARKER: &str = "\u{1b}[2J";
const PASSPHRASE: &str = "correct horse battery staple";
const VALUE_SENTINEL: &str = "vault-tui-pty-secret-sentinel";
const CREATED_VALUE_SENTINEL: &str = "vault-tui-created-value-sentinel";
const PEEK_BEGIN_MARKER: &str = "BEGIN CONTROLLED VAULT PEEK";
const PEEK_END_MARKER: &str = "END CONTROLLED VAULT PEEK";
// Vault interactions perform deliberately expensive key derivation. This PTY
// binary runs serially in its own Nextest invocation, so 30 seconds remains a
// useful bound while allowing headroom on loaded machines.
const PTY_EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const PTY_EXIT_TIMEOUT: Duration = Duration::from_secs(15);

#[test]
fn browser_unlocks_resizes_locks_and_restores_the_terminal_on_quit() {
    let temp = support::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/API_TOKEN".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(VALUE_SENTINEL.as_bytes().to_vec()),
        )
        .unwrap();

    let Some((mut master, slave)) = required_pseudo_terminal("vault tui browser") else {
        return;
    };
    let stdin = slave.try_clone().unwrap();
    let stdout = slave.try_clone().unwrap();
    let stderr = slave.try_clone().unwrap();
    let original = terminal_attributes(&slave);
    let mut child = ChildGuard::new(
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .args(["vault", "tui", "--home"])
            .arg(&home)
            .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
            .env("JIG_VAULT_NEW_PASSPHRASE", "must-be-cleared-before-worker")
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap(),
    );
    set_nonblocking(&master);

    let mut output = Vec::new();
    read_until(&mut master, &mut output, "API_TOKEN", PTY_EVENT_TIMEOUT);
    assert!(!String::from_utf8_lossy(&output).contains(VALUE_SENTINEL));

    let create_offset = output.len();
    master
        .write_all(format!("aPTY_FIELD\t\t{CREATED_VALUE_SENTINEL}\r").as_bytes())
        .unwrap();
    read_until_from(
        &mut master,
        &mut output,
        create_offset,
        "Vault updated.",
        PTY_EVENT_TIMEOUT,
    );
    assert!(!String::from_utf8_lossy(&output).contains(CREATED_VALUE_SENTINEL));

    let tools_offset = output.len();
    master.write_all(b":").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        tools_offset,
        "Enter run/open",
        PTY_EVENT_TIMEOUT,
    );
    let activity_offset = output.len();
    master.write_all(b"activity\r").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        activity_offset,
        "Enter/Esc close",
        PTY_EVENT_TIMEOUT,
    );
    assert!(String::from_utf8_lossy(&output[activity_offset..]).contains("field_batch_apply"));
    assert!(!String::from_utf8_lossy(&output).contains(VALUE_SENTINEL));
    assert!(!String::from_utf8_lossy(&output).contains(CREATED_VALUE_SENTINEL));
    let browse_offset = output.len();
    master.write_all(b"\r").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        browse_offset,
        "Value hidden.",
        PTY_EVENT_TIMEOUT,
    );

    let confirmation_offset = output.len();
    master.write_all(b"p").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        confirmation_offset,
        "PEEK",
        PTY_EVENT_TIMEOUT,
    );
    let peek_offset = output.len();
    master.write_all(b"PEEK\r").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        peek_offset,
        PEEK_END_MARKER,
        PTY_EVENT_TIMEOUT,
    );
    assert!(
        String::from_utf8_lossy(&output[peek_offset..]).contains(CREATED_VALUE_SENTINEL),
        "controlled Peek did not emit the selected field"
    );
    let cleared_offset = output.len();
    master.write_all(b" ").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        cleared_offset,
        "Controlled preview cleared after",
        PTY_EVENT_TIMEOUT,
    );
    assert!(
        !String::from_utf8_lossy(&output[cleared_offset..]).contains(CREATED_VALUE_SENTINEL),
        "controlled Peek value survived into the metadata redraw"
    );

    let resize_offset = output.len();
    resize_terminal(&slave, 70, 22);
    // SAFETY: the child PID is live and SIGWINCH has its ordinary terminal
    // resize meaning.
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGWINCH) },
        0
    );
    read_until_from(
        &mut master,
        &mut output,
        resize_offset,
        FULL_CLEAR_MARKER,
        PTY_EVENT_TIMEOUT,
    );
    let resized_frame_offset = resize_offset
        + output[resize_offset..]
            .windows(FULL_CLEAR_MARKER.len())
            .position(|window| window == FULL_CLEAR_MARKER.as_bytes())
            .expect("resize redraw emitted the awaited full-clear marker")
        + FULL_CLEAR_MARKER.len();
    read_until_from(
        &mut master,
        &mut output,
        resized_frame_offset,
        "Production",
        PTY_EVENT_TIMEOUT,
    );
    let lock_offset = output.len();
    master.write_all(b"L").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        lock_offset,
        "Vault passphrase",
        PTY_EVENT_TIMEOUT,
    );

    let resume_offset = output.len();
    master.write_all(PASSPHRASE.as_bytes()).unwrap();
    master.write_all(b"\r").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        resume_offset,
        "API_TOKEN",
        PTY_EVENT_TIMEOUT,
    );
    master.write_all(b"\x03").unwrap();

    let status =
        wait_for_child_while_draining(&mut child, &mut master, &mut output, PTY_EXIT_TIMEOUT)
            .unwrap_or_else(|| {
                panic!(
                    "vault TUI did not exit after Ctrl-C; output: {}",
                    String::from_utf8_lossy(&output)
                )
            });
    assert!(status.success(), "vault TUI exited with {status}");
    let restored = terminal_attributes(&slave);
    assert_eq!(
        restored.c_lflag & (libc::ECHO | libc::ICANON),
        original.c_lflag & (libc::ECHO | libc::ICANON),
        "terminal echo/canonical flags were not restored"
    );

    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("\u{1b}[?1049h"),
        "alternate screen not entered"
    );
    assert!(
        output.contains("\u{1b}[?1049l"),
        "alternate screen not left"
    );
    assert!(
        output.contains("\u{1b}[?2004h"),
        "bracketed paste not enabled"
    );
    assert!(
        output.contains("\u{1b}[?2004l"),
        "bracketed paste not disabled"
    );
    assert!(!output.contains(VALUE_SENTINEL));
    assert_only_between_controlled_peek(&output, CREATED_VALUE_SENTINEL);

    let snapshot = vault.snapshot(&passphrase).unwrap();
    assert!(snapshot.fields.iter().any(|field| {
        field.reference.to_string() == "jig://Production/PTY_FIELD"
            && field.value_len == CREATED_VALUE_SENTINEL.len()
    }));
}

#[test]
fn sigterm_clears_and_restores_the_vault_tui_before_redelivery() {
    let temp = support::tempdir().unwrap();
    let home = temp.path().join("vault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    vault
        .set_field(
            &passphrase,
            "jig://Production/API_TOKEN".parse().unwrap(),
            FieldKind::Concealed,
            SecretBytes::new(VALUE_SENTINEL.as_bytes().to_vec()),
        )
        .unwrap();

    let Some((mut master, slave)) = required_pseudo_terminal("vault tui signal") else {
        return;
    };
    let stdin = slave.try_clone().unwrap();
    let stdout = slave.try_clone().unwrap();
    let stderr = slave.try_clone().unwrap();
    let original = terminal_attributes(&slave);
    let mut child = ChildGuard::new(
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .args(["vault", "tui", "--home"])
            .arg(&home)
            .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap(),
    );
    set_nonblocking(&master);

    let mut output = Vec::new();
    read_until(&mut master, &mut output, "API_TOKEN", PTY_EVENT_TIMEOUT);
    // SAFETY: the child PID is live and the runtime's signal session owns
    // structured SIGTERM restoration and redelivery.
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let status =
        wait_for_child_while_draining(&mut child, &mut master, &mut output, PTY_EXIT_TIMEOUT)
            .unwrap_or_else(|| {
                panic!(
                    "vault TUI did not exit after SIGTERM; output: {}",
                    String::from_utf8_lossy(&output)
                )
            });
    assert!(
        status.signal() == Some(libc::SIGTERM) || status.code() == Some(143),
        "vault TUI exited with {status}; output: {}",
        String::from_utf8_lossy(&output)
    );
    let restored = terminal_attributes(&slave);
    assert_eq!(
        restored.c_lflag & (libc::ECHO | libc::ICANON),
        original.c_lflag & (libc::ECHO | libc::ICANON),
        "terminal echo/canonical flags were not restored after SIGTERM"
    );
    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("\u{1b}[2J"),
        "alternate screen was not cleared"
    );
    assert!(
        output.contains("\u{1b}[?1049l"),
        "alternate screen was not left"
    );
    assert!(
        output.contains("\u{1b}[?2004l"),
        "bracketed paste was not disabled"
    );
    assert!(!output.contains(VALUE_SENTINEL));
}

fn assert_only_between_controlled_peek(output: &str, sentinel: &str) {
    let mut cursor = 0;
    let mut occurrences = 0;
    while let Some(relative) = output[cursor..].find(sentinel) {
        let position = cursor + relative;
        let begin = output[..position]
            .rfind(PEEK_BEGIN_MARKER)
            .expect("revealed value appeared without a controlled Peek start marker");
        if let Some(previous_end) = output[..position].rfind(PEEK_END_MARKER) {
            assert!(
                previous_end < begin,
                "revealed value appeared after the controlled Peek end marker"
            );
        }
        assert!(
            output[position + sentinel.len()..].contains(PEEK_END_MARKER),
            "revealed value appeared without a controlled Peek end marker"
        );
        occurrences += 1;
        cursor = position + sentinel.len();
    }
    assert!(occurrences > 0, "controlled Peek never emitted the value");
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
    // SAFETY: `ptsname` returns storage managed by libc for this live master.
    let name = unsafe { libc::ptsname(master.as_raw_fd()) };
    if name.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `name` is the NUL-terminated slave path returned above.
    let slave = unsafe { libc::open(name, libc::O_RDWR | libc::O_NOCTTY) };
    if slave < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `slave` is a newly owned descriptor.
    let slave = unsafe { File::from_raw_fd(slave) };
    resize_terminal(&slave, columns, rows);
    Ok((master, slave))
}

fn required_pseudo_terminal(label: &str) -> Option<(File, File)> {
    match pseudo_terminal(120, 30) {
        Ok(terminal) => Some(terminal),
        Err(error) if pty_is_unavailable(&error) && pty_skip_is_explicitly_allowed() => {
            eprintln!("skipping {label} PTY test because {ALLOW_PTY_SKIP_ENV}=1: {error}");
            None
        }
        Err(error) if pty_is_unavailable(&error) => panic!(
            "{label} requires pseudo-terminal support: {error}; set {ALLOW_PTY_SKIP_ENV}=1 only when intentionally exempt"
        ),
        Err(error) => panic!("creating pseudo-terminal for {label} failed: {error}"),
    }
}

fn pty_skip_is_explicitly_allowed() -> bool {
    std::env::var(ALLOW_PTY_SKIP_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

fn pty_is_unavailable(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EPERM | libc::EACCES | libc::ENOENT | libc::ENXIO | libc::ENODEV | libc::ENOTTY)
    )
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
        0,
        "resizing PTY failed: {}",
        std::io::Error::last_os_error()
    );
}

fn terminal_attributes(slave: &File) -> libc::termios {
    // SAFETY: zero is a valid initial representation and tcgetattr fills it.
    let mut attributes = unsafe { std::mem::zeroed::<libc::termios>() };
    // SAFETY: `slave` is live and `attributes` is writable.
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut attributes) },
        0,
        "reading PTY attributes failed: {}",
        std::io::Error::last_os_error()
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

fn read_until(file: &mut File, output: &mut Vec<u8>, needle: &str, timeout: Duration) {
    read_until_from(file, output, 0, needle, timeout);
}

fn read_until_from(
    file: &mut File,
    output: &mut Vec<u8>,
    offset: usize,
    needle: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        read_available(file, output);
        if String::from_utf8_lossy(&output[offset..]).contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {needle:?}; recent output: {}",
            String::from_utf8_lossy(&output[output.len().saturating_sub(4096)..])
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
