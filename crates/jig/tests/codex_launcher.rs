#![cfg(unix)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

const ALLOW_PTY_SKIP_ENV: &str = "JIG_ALLOW_PTY_TEST_SKIP";

#[test]
fn repository_launcher_preserves_the_invocation_directory_for_codex() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let caller = temp.path().join("caller");
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::create_dir(&caller).unwrap();
    let launcher = write_executable(
        repo.join("scripts/jig"),
        include_str!("../../../scripts/jig"),
    );
    write_executable(
        repo.join("scripts/install-jig.sh"),
        "#!/bin/sh\nprintf '%s\\n' \"$JIG_TEST_FAKE_BIN\"\n",
    );
    let fake_bin = write_executable(
        temp.path().join("jig-stub.sh"),
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  printf '%s\n' 'jig 0.2.0-beta.1'
  exit 0
fi
pwd -P
printf '<%s>\n' "$@"
"#,
    );

    let output = Command::new(launcher)
        .args(["codex", "launch", "codex"])
        .env("JIG_TEST_FAKE_BIN", fake_bin)
        .current_dir(&caller)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut lines = stdout.lines();
    assert_eq!(
        lines.next(),
        Some(caller.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        ["<codex>", "<launch>", "<codex>"]
    );

    let output = Command::new(repo.join("scripts/jig"))
        .args(["--json", "codex", "launch", "./", "--dry-run"])
        .env("JIG_TEST_FAKE_BIN", temp.path().join("jig-stub.sh"))
        .current_dir(&caller)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "launcher with a leading global flag failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut lines = stdout.lines();
    assert_eq!(
        lines.next(),
        Some(caller.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        ["<--json>", "<codex>", "<launch>", "<./>", "<--dry-run>"]
    );
}

#[test]
fn interactive_picker_opens_before_inspection_and_launches_searched_exact_home() {
    let temp = tempfile::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    let launched = temp.path().join("launched-home");
    let invocations = temp.path().join("invocations");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let stub = write_executable(
        temp.path().join("codex-stub.sh"),
        r#"#!/bin/sh
printf '%s|%s\n' "${1:-launch}" "$CODEX_HOME" >> "$JIG_TEST_INVOCATIONS"
if [ "${1:-}" != "app-server" ]; then
  printf '%s\n' "$CODEX_HOME" > "$JIG_TEST_LAUNCHED"
  sleep 1
  exit 0
fi
read -r initialize
printf '%s\n' '{"id":0,"result":{}}'
read -r initialized
read -r account
read -r limits
sleep 30
"#,
    );
    let Some((mut master, stdin, stdout)) = required_pseudo_terminal("picker interaction") else {
        return;
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["codex", "launch"])
        .env("HOME", temp.path())
        .env("CODEX_HOME", &default)
        .env("JIG_CODEX_BIN", &stub)
        .env("JIG_TEST_LAUNCHED", &launched)
        .env("JIG_TEST_INVOCATIONS", &invocations)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    set_nonblocking(&master);

    let mut output = Vec::new();
    read_until(
        &mut master,
        &mut output,
        "Codex Home Picker",
        Duration::from_secs(3),
    );
    let initial = String::from_utf8_lossy(&output);
    assert!(initial.contains("codex-work"), "{initial}");
    assert!(initial.contains("loading"), "{initial}");
    assert!(!initial.contains("Select a Codex home"), "{initial}");

    master.write_all(b"/work").unwrap();
    let search_deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < search_deadline {
        read_available(&mut master, &mut output);
        std::thread::sleep(Duration::from_millis(10));
    }
    master.write_all(b"\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !launched.is_file() {
        read_available(&mut master, &mut output);
        if let Some(status) = child.try_wait().unwrap() {
            panic!(
                "picker exited with {status} before launching; invocations: {}; output: {}",
                fs::read_to_string(&invocations).unwrap_or_default(),
                String::from_utf8_lossy(&output)
            );
        }
        assert!(
            Instant::now() < deadline,
            "picker did not launch; invocations: {}; output: {}",
            fs::read_to_string(&invocations).unwrap_or_default(),
            String::from_utf8_lossy(&output)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    read_until(
        &mut master,
        &mut output,
        "\x1b[?1049l",
        Duration::from_secs(1),
    );
    let status = child
        .wait_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_or_else(|| {
            child.kill().unwrap();
            child.wait().unwrap()
        });
    assert!(status.success(), "picker exited with {status}");
    assert_eq!(
        fs::read_to_string(launched).unwrap().trim(),
        work.canonicalize().unwrap().to_string_lossy()
    );
}

#[test]
fn homes_shows_terminal_progress_while_accounts_are_inspected() {
    let temp = tempfile::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let stub = write_executable(
        temp.path().join("codex-stub.sh"),
        r#"#!/bin/sh
read -r initialize
printf '%s\n' '{"id":0,"result":{}}'
read -r initialized
read -r account
printf '%s\n' '{"id":1,"result":{"account":null}}'
sleep 30
"#,
    );
    let Some((mut master, stdout, stderr)) = required_pseudo_terminal("homes progress") else {
        return;
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["codex", "homes"])
        .env("HOME", temp.path())
        .env("CODEX_HOME", &default)
        .env("JIG_CODEX_BIN", &stub)
        .env("NO_COLOR", "1")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    set_nonblocking(&master);

    let mut output = Vec::new();
    read_until(
        &mut master,
        &mut output,
        "Codex homes: 2 found",
        Duration::from_secs(5),
    );
    let status = child
        .wait_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap_or_else(|| {
            child.kill().unwrap();
            child.wait().unwrap()
        });
    read_available(&mut master, &mut output);

    let output = String::from_utf8_lossy(&output);
    assert!(
        status.success(),
        "homes exited with {status}; output: {output}"
    );
    assert!(
        output.contains("jig codex homes | inspect local Codex homes"),
        "{output}"
    );
    assert!(output.contains("inspect homes"), "{output}");
    assert!(output.contains("0/2"), "{output}");
    assert!(output.contains("1/2"), "{output}");
    assert!(output.contains("2/2"), "{output}");
    assert!(output.contains("inspected 2 Codex homes"), "{output}");
}

#[test]
fn sigint_restores_picker_and_cancels_all_active_home_inspections() {
    let temp = tempfile::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let stub = write_executable(
        temp.path().join("codex-stub.sh"),
        r#"#!/bin/sh
printf '%s\n' "$$" > "$CODEX_HOME/app-server.pid"
sleep 30
"#,
    );
    let Some((mut master, stdin, stdout)) = required_pseudo_terminal("picker signal") else {
        return;
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["codex", "launch"])
        .env("HOME", temp.path())
        .env("CODEX_HOME", &default)
        .env("JIG_CODEX_BIN", &stub)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    set_nonblocking(&master);

    let mut output = Vec::new();
    read_until(
        &mut master,
        &mut output,
        "Codex Home Picker",
        Duration::from_secs(3),
    );
    let pid_paths = [default.join("app-server.pid"), work.join("app-server.pid")];
    for path in &pid_paths {
        wait_for_path(path, Duration::from_secs(3));
    }
    let app_server_pids = pid_paths.map(|path| {
        fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse::<libc::pid_t>()
            .unwrap()
    });

    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        read_available(&mut master, &mut output);
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.signal() == Some(libc::SIGINT) || status.code() == Some(130),
        "picker exited with {status}; output: {}",
        String::from_utf8_lossy(&output)
    );
    read_until(
        &mut master,
        &mut output,
        "\x1b[?1049l",
        Duration::from_secs(1),
    );
    for pid in app_server_pids {
        wait_for_process_exit(pid, Duration::from_secs(3));
    }
}

#[test]
fn sigint_cancels_all_active_home_inspections_before_redelivery() {
    let temp = tempfile::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let stub = write_executable(
        temp.path().join("codex-stub.sh"),
        r#"#!/bin/sh
printf '%s\n' "$$" > "$CODEX_HOME/app-server.pid"
sleep 30
"#,
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(["codex", "homes", "--usage"])
        .env("HOME", temp.path())
        .env("CODEX_HOME", &default)
        .env("JIG_CODEX_BIN", &stub)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid_paths = [default.join("app-server.pid"), work.join("app-server.pid")];
    for path in &pid_paths {
        wait_for_path(path, Duration::from_secs(3));
    }
    let app_server_pids = pid_paths.map(|path| {
        fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse::<libc::pid_t>()
            .unwrap()
    });

    assert_eq!(
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = child
        .wait_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_or_else(|| {
            child.kill().unwrap();
            child.wait().unwrap()
        });
    assert!(
        status.signal() == Some(libc::SIGINT) || status.code() == Some(130),
        "inspection exited with {status}"
    );
    for pid in app_server_pids {
        wait_for_process_exit(pid, Duration::from_secs(3));
    }
}

fn write_executable(path: impl AsRef<Path>, contents: &str) -> std::path::PathBuf {
    let path = path.as_ref();
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_path_buf()
}

fn pseudo_terminal() -> std::io::Result<(File, File, File)> {
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
    if master < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let master = unsafe { File::from_raw_fd(master) };
    if unsafe { libc::grantpt(master.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::unlockpt(master.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let name = unsafe { libc::ptsname(master.as_raw_fd()) };
    if name.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let slave = unsafe { libc::open(name, libc::O_RDWR | libc::O_NOCTTY) };
    if slave < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let slave = unsafe { File::from_raw_fd(slave) };
    let size = libc::winsize {
        ws_row: 30,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &size) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stderr = unsafe { libc::dup(slave.as_raw_fd()) };
    if stderr < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stderr = unsafe { File::from_raw_fd(stderr) };
    Ok((master, slave, stderr))
}

fn required_pseudo_terminal(label: &str) -> Option<(File, File, File)> {
    match pseudo_terminal() {
        Ok(terminal) => Some(terminal),
        Err(error) if pty_is_unavailable(&error) && pty_skip_is_explicitly_allowed() => {
            eprintln!("skipping {label} PTY test because {ALLOW_PTY_SKIP_ENV}=1: {error}");
            None
        }
        Err(error) if pty_is_unavailable(&error) => panic!(
            "{label} PTY test requires pseudo-terminal support: {error}; set {ALLOW_PTY_SKIP_ENV}=1 only when this environment is intentionally exempt"
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

fn set_nonblocking(file: &File) {
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "reading PTY flags failed");
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0,
        "setting PTY nonblocking mode failed"
    );
}

fn read_until(file: &mut File, output: &mut Vec<u8>, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        read_available(file, output);
        if String::from_utf8_lossy(output).contains(needle) {
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
            Err(error) if pty_master_reached_eof(&error) => return,
            Err(error) => panic!("reading PTY output failed: {error}"),
        }
    }
}

fn pty_master_reached_eof(error: &std::io::Error) -> bool {
    cfg!(target_os = "linux") && error.raw_os_error() == Some(libc::EIO)
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_process_exit(pid: libc::pid_t, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if unsafe { libc::kill(pid, 0) } == -1 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "process {pid} remained alive after cancellation"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
