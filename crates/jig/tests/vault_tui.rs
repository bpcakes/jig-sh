#![cfg(unix)]

mod support;

use std::{
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use jig_vault::{FieldKind, SecretBytes, Vault};
use secrecy::SecretString;
use wait_timeout::ChildExt;

const ALLOW_PTY_SKIP_ENV: &str = "JIG_ALLOW_PTY_TEST_SKIP";
const PASSPHRASE: &str = "correct horse battery staple";
const VALUE_SENTINEL: &str = "vault-tui-pty-secret-sentinel";
const CREATED_VALUE_SENTINEL: &str = "vault-tui-created-value-sentinel";

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["vault", "tui", "--home"])
        .arg(&home)
        .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
        .env("JIG_VAULT_NEW_PASSPHRASE", "must-be-cleared-before-worker")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    set_nonblocking(&master);

    let mut output = Vec::new();
    read_until(
        &mut master,
        &mut output,
        "API_TOKEN",
        Duration::from_secs(8),
    );
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
        Duration::from_secs(8),
    );
    assert!(!String::from_utf8_lossy(&output).contains(CREATED_VALUE_SENTINEL));

    resize_terminal(&slave, 70, 22);
    // SAFETY: the child PID is live and SIGWINCH has its ordinary terminal
    // resize meaning.
    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGWINCH) },
        0
    );
    master.write_all(b"L").unwrap();
    read_until(
        &mut master,
        &mut output,
        "Vault passphrase",
        Duration::from_secs(3),
    );

    let resume_offset = output.len();
    master.write_all(PASSPHRASE.as_bytes()).unwrap();
    master.write_all(b"\r").unwrap();
    read_until_from(
        &mut master,
        &mut output,
        resume_offset,
        "API_TOKEN",
        Duration::from_secs(8),
    );
    master.write_all(b"q").unwrap();

    let status = child
        .wait_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_or_else(|| {
            let _ = child.kill();
            panic!(
                "vault TUI did not exit after q; output: {}",
                String::from_utf8_lossy(&output)
            )
        });
    read_available(&mut master, &mut output);
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
    assert!(!output.contains(CREATED_VALUE_SENTINEL));

    let snapshot = vault.snapshot(&passphrase).unwrap();
    assert!(snapshot.fields.iter().any(|field| {
        field.reference.to_string() == "jig://Production/PTY_FIELD"
            && field.value_len == CREATED_VALUE_SENTINEL.len()
    }));
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
            "timed out waiting for {needle:?}; output: {}",
            String::from_utf8_lossy(output)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
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
