#![cfg(unix)]

#[path = "shared/pty.rs"]
mod pty_support;
mod support;

use jig_vault::{FieldKind, SecretBytes, Vault};
use pty_support::{ChildGuard, read_available, wait_for_child_while_draining};
use secrecy::SecretString;
use std::{
    fs::File,
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const PASSPHRASE: &str = "correct horse battery staple";
const TIMEOUT: Duration = Duration::from_secs(30);

#[test]
fn cli_field_prompts_echo_only_text_and_cancel_without_overwriting() {
    let temp = support::tempdir().unwrap();
    let home = temp.path().join("ExampleVault");
    let vault = Vault::resolve_for_test(Some(home.clone())).unwrap();
    let passphrase = SecretString::from(PASSPHRASE.to_owned());
    vault.init(&passphrase).unwrap();
    let reference = "jig://ExampleProject/FIELD";
    // Exercise both implicit and explicit terminal prompt selection.
    for (text, explicit, input, expected, success) in [
        (
            true,
            false,
            b"visible-sentinel\n".as_slice(),
            b"visible-sentinel".as_slice(),
            true,
        ),
        (
            false,
            true,
            b"concealed-sentinel\n".as_slice(),
            b"concealed-sentinel".as_slice(),
            true,
        ),
        (
            true,
            true,
            b"\x04".as_slice(),
            b"preserved-value".as_slice(),
            false,
        ),
        (
            true,
            true,
            b"partial\x04\x04".as_slice(),
            b"preserved-value".as_slice(),
            false,
        ),
        (true, true, b"\n".as_slice(), b"".as_slice(), true),
    ] {
        vault
            .set_field(
                &passphrase,
                reference.parse().unwrap(),
                FieldKind::Text,
                SecretBytes::new(b"preserved-value".to_vec()),
            )
            .unwrap();
        let (mut master, slave) =
            pseudo_terminal(100, 30).expect("field prompt test requires a PTY");
        let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
        command
            .args(["vault", "field", "set", reference, "--home"])
            .arg(&home)
            .env("JIG_VAULT_PASSPHRASE", PASSPHRASE)
            .env_remove("JIG_VAULT_NEW_PASSPHRASE")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        if text {
            command.arg("--text");
        }
        if explicit {
            command.arg("--value-prompt");
        }
        make_stdin_controlling_terminal(&mut command);
        let mut child = ChildGuard::new(command.spawn().unwrap());
        set_nonblocking(&master);
        let mut output = Vec::new();
        read_until(&mut master, &mut output, "Field value: ", TIMEOUT);
        master.write_all(input).unwrap();
        let status = wait_for_child_while_draining(&mut child, &mut master, &mut output, TIMEOUT)
            .expect("field prompt did not finish");
        let rendered = String::from_utf8_lossy(&output);
        assert_eq!(status.success(), success, "{rendered}");
        assert!(!rendered.contains(PASSPHRASE));
        if text && input.starts_with(b"visible") {
            assert!(rendered.contains("visible-sentinel"));
        }
        if !text {
            assert!(!rendered.contains("concealed-sentinel"), "{rendered}");
        }
        let mut saved = Vec::new();
        vault
            .read_field_to(&passphrase, reference.parse().unwrap(), &mut saved)
            .unwrap();
        assert_eq!(saved, expected);
    }
}

fn pseudo_terminal(columns: u16, rows: u16) -> std::io::Result<(File, File)> {
    // SAFETY: each successful descriptor is immediately wrapped exactly once.
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `master` is a newly owned descriptor.
    let master = unsafe { File::from_raw_fd(master) };
    set_close_on_exec(&master)?;
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
    set_close_on_exec(&slave)?;
    resize_terminal(&slave, columns, rows);
    Ok((master, slave))
}

fn make_stdin_controlling_terminal(command: &mut Command) {
    // SAFETY: the child closure runs after stdio remapping and invokes only
    // async-signal-safe session and terminal ioctls before exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as libc::c_ulong, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn set_close_on_exec(file: &File) -> std::io::Result<()> {
    // SAFETY: fcntl reads descriptor flags for this live file.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
    if flags == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains live and F_SETFD preserves its existing flags.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
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
