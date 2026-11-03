use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MARKER_ENV: &str = "JIG_OWNED_PROCESS_PARENT_LOSS_MARKER";
const READY: &str = "parent-loss-child-ready";
const REFUSED: &str = "parent-loss-child-refused";

#[test]
fn parent_loss_supervisor_helper() {
    let Some(marker) = std::env::var_os(MARKER_ENV) else {
        return;
    };
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "printf executed > \"$1\"", "parent-loss-payload"])
        .arg(marker)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::null());
    // SAFETY: getpid only reads the supervisor's identity.
    let parent = unsafe { libc::getpid() };
    // SAFETY: the callback uses only stack data and async-signal-safe syscalls.
    // It deliberately waits before arming PDEATHSIG to expose the race.
    unsafe {
        command.pre_exec(move || {
            let ready = b"parent-loss-child-ready\n";
            if libc::write(libc::STDOUT_FILENO, ready.as_ptr().cast(), ready.len())
                != ready.len() as isize
            {
                return Err(std::io::Error::last_os_error());
            }
            let mut byte = 0_u8;
            // The test closes the pipe after killing the supervisor. EOF
            // releases this child, which still holds the inherited read end.
            loop {
                match libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1) {
                    0 => break,
                    -1 if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => {}
                    _ => return Err(std::io::Error::from_raw_os_error(libc::EIO)),
                }
            }
            // kill() returning does not itself prove reparenting has happened.
            let pause = libc::timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            };
            for _ in 0..5_000 {
                if libc::getppid() != parent {
                    let result = crate::unix::set_parent_death_signal(parent);
                    if result
                        .as_ref()
                        .is_err_and(|error| error.raw_os_error() == Some(libc::ESRCH))
                    {
                        let refused = b"parent-loss-child-refused\n";
                        libc::write(libc::STDOUT_FILENO, refused.as_ptr().cast(), refused.len());
                    }
                    return result;
                }
                libc::nanosleep(&pause, std::ptr::null_mut());
            }
            Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT))
        });
    }
    // The supervisor blocks inside spawn until the child attempts exec; the
    // test kills it while the child's pre_exec callback waits at the gate.
    let _ = command.status();
}

#[test]
fn owned_child_refuses_exec_after_parent_loss_before_signal_setup() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("unexpected-exec");
    let mut supervisor = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::tests::cleanup_policy::parent_loss::parent_loss_supervisor_helper",
            "--nocapture",
        ])
        .env(MARKER_ENV, &marker)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = supervisor.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let wait_for = |message: &str| -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = receiver
                .recv_timeout(remaining)
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            if line == message {
                return Ok(());
            }
        }
    };
    let result = wait_for(READY).and_then(|()| {
        supervisor.kill().map_err(|error| error.to_string())?;
        drop(supervisor.stdin.take());
        wait_for(REFUSED)
    });

    // Keep the supervisor unreaped until teardown, pinning its process group
    // even on a failed assertion or when the exec-refusal check regresses.
    // SAFETY: this exact unreaped child pins the positive PGID we created.
    unsafe { libc::kill(-(supervisor.id() as libc::pid_t), libc::SIGKILL) };
    drop(supervisor.stdin.take());
    supervisor.wait().unwrap();
    reader.join().unwrap();
    assert!(result.is_ok(), "parent-loss handshake failed: {result:?}");
    assert!(
        !marker.exists(),
        "the child executed after losing its parent"
    );
}
