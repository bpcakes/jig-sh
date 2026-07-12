use std::io;
#[cfg(windows)]
use std::process::Command;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

const REAP_TIMEOUT: Duration = Duration::from_secs(2);
const TERMINATE_TIMEOUT: Duration = Duration::from_secs(2);
const KILL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) fn terminate_and_reap(child: &mut Child) -> Result<()> {
    terminate_child(child)?;
    wait_after_terminate(child)
}

pub(super) fn terminate_and_reap_logged(child: &mut Child, context: &str) -> bool {
    report_cleanup_result(terminate_and_reap(child), context)
}

fn report_cleanup_result(result: Result<()>, context: &str) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            eprintln!("jig proxy {context}; child cleanup also failed: {error:#}");
            false
        }
    }
}

pub(super) fn wait_after_terminate(child: &mut Child) -> Result<()> {
    let pid = child.id();
    wait_for_reap(pid, REAP_TIMEOUT, || child.try_wait())
}

fn wait_for_reap(
    pid: u32,
    timeout: Duration,
    mut try_wait: impl FnMut() -> io::Result<Option<ExitStatus>>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                bail!("child process {pid} was still running after {timeout:?} reap deadline")
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to reap child process {pid}"));
            }
        }
    }
}

#[cfg(unix)]
pub(super) fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    let mut direct_child_exited = child
        .try_wait()
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
        .is_some();
    if direct_child_exited {
        terminate_process_group(pid)?;
    } else {
        terminate_pid(pid)?;
    }
    let deadline = Instant::now() + TERMINATE_TIMEOUT;
    while Instant::now() < deadline {
        if !direct_child_exited
            && child
                .try_wait()
                .with_context(|| {
                    format!("failed to inspect child process {pid} during termination")
                })?
                .is_some()
        {
            direct_child_exited = true;
        }
        if !target_alive(pid, direct_child_exited) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    if direct_child_exited {
        kill_process_group(pid)?;
        confirm_not_alive(pid, KILL_CONFIRM_TIMEOUT, || process_group_alive(pid))
    } else {
        let signal_result = kill_pid(pid);
        let child_result = child
            .kill()
            .with_context(|| format!("failed final Child::kill for process {pid}"));
        if let Err(signal_error) = signal_result {
            if let Err(child_error) = child_result {
                return Err(signal_error
                    .context(format!("fallback child kill also failed: {child_error:#}")));
            }
        }
        Ok(())
    }
}

#[cfg(not(unix))]
pub(super) fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    if child
        .try_wait()
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
        .is_none()
    {
        terminate_pid(pid)?;
    }
    Ok(())
}

fn confirm_not_alive(pid: u32, timeout: Duration, mut alive: impl FnMut() -> bool) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if !alive() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "child process {pid} remained alive after final termination attempt and {timeout:?} confirmation deadline"
            )
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn target_alive(pid: u32, direct_exited: bool) -> bool {
    if direct_exited {
        process_group_alive(pid)
    } else {
        process_group_or_pid_alive(pid)
    }
}
#[cfg(unix)]
pub(super) fn terminate_pid(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group_or_pid(pid, libc::SIGTERM)
        .with_context(|| format!("failed to send SIGTERM to child process/group {pid}"))
}
#[cfg(unix)]
fn terminate_process_group(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group(pid, libc::SIGTERM)
        .with_context(|| format!("failed to send SIGTERM to child process group {pid}"))
}
#[cfg(unix)]
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group_or_pid(pid, libc::SIGKILL)
        .with_context(|| format!("failed to send SIGKILL to child process/group {pid}"))
}
#[cfg(unix)]
fn kill_process_group(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group(pid, libc::SIGKILL)
        .with_context(|| format!("failed to send SIGKILL to child process group {pid}"))
}
#[cfg(unix)]
fn checked_pid(pid: u32) -> Result<i32> {
    unix_pid(pid).ok_or_else(|| anyhow!("child PID {pid} exceeds platform process-id range"))
}
#[cfg(unix)]
fn signal_group_or_pid(pid: i32, signal: i32) -> io::Result<()> {
    match send_signal(-pid, signal) {
        Ok(()) => Ok(()),
        Err(group_error) => match send_signal(pid, signal) {
            Ok(()) => Ok(()),
            Err(pid_error) if pid_error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            Err(pid_error) => Err(io::Error::new(
                pid_error.kind(),
                format!("group signal failed: {group_error}; direct signal failed: {pid_error}"),
            )),
        },
    }
}
#[cfg(unix)]
fn signal_group(pid: i32, signal: i32) -> io::Result<()> {
    match send_signal(-pid, signal) {
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
        result => result,
    }
}
#[cfg(unix)]
fn send_signal(pid: i32, signal: i32) -> io::Result<()> {
    if unsafe { libc::kill(pid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[cfg(unix)]
fn process_group_or_pid_alive(pid: u32) -> bool {
    let Some(pid) = unix_pid(pid) else {
        return false;
    };
    unsafe { libc::kill(-pid, 0) == 0 || libc::kill(pid, 0) == 0 }
}
#[cfg(unix)]
fn process_group_alive(pid: u32) -> bool {
    let Some(pid) = unix_pid(pid) else {
        return false;
    };
    unsafe { libc::kill(-pid, 0) == 0 }
}
#[cfg(unix)]
pub(super) fn unix_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok()
}

#[cfg(windows)]
pub(super) fn terminate_pid(pid: u32) -> Result<()> {
    if !crate::state::pid_is_alive(pid) {
        return Ok(());
    }
    run_taskkill(pid, false)?;
    if wait_for_pid_exit(pid, Duration::from_secs(2)) {
        return Ok(());
    }
    run_taskkill(pid, true)?;
    if wait_for_pid_exit(pid, Duration::from_secs(1)) {
        Ok(())
    } else {
        bail!("child process {pid} remained alive after forced taskkill")
    }
}
#[cfg(windows)]
fn run_taskkill(pid: u32, force: bool) -> Result<()> {
    let mut command = Command::new(windows_system32_tool("taskkill.exe"));
    command.env_clear().args(["/PID", &pid.to_string(), "/T"]);
    if force {
        command.arg("/F");
    }
    let status = command
        .status()
        .with_context(|| format!("failed to launch taskkill for child process {pid}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("taskkill for child process {pid} exited with status {status}")
    }
}
#[cfg(windows)]
fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !crate::state::pid_is_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    !crate::state::pid_is_alive(pid)
}
#[cfg(windows)]
fn windows_system32_tool(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(r"C:\Windows\System32").join(name)
}
#[cfg(windows)]
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    terminate_pid(pid)
}
#[cfg(not(any(unix, windows)))]
pub(super) fn terminate_pid(pid: u32) -> Result<()> {
    bail!("terminating child process {pid} is unsupported on this platform")
}
#[cfg(not(any(unix, windows)))]
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    terminate_pid(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_reap_and_termination_are_pid_specific() {
        let reap = wait_for_reap(4242, Duration::ZERO, || Ok(None)).unwrap_err();
        assert!(reap.to_string().contains("4242"));
        let termination = confirm_not_alive(4343, Duration::ZERO, || true).unwrap_err();
        assert!(termination.to_string().contains("4343"));
    }

    #[test]
    fn cleanup_reporting_preserves_the_callers_primary_error() {
        let primary = anyhow!("primary startup failure");
        assert!(!report_cleanup_result(
            Err(anyhow!("child process 4444 remained alive")),
            "could not clean up after primary failure",
        ));
        assert_eq!(primary.to_string(), "primary startup failure");
    }

    #[cfg(unix)]
    #[test]
    fn terminate_child_kills_process_group_after_wrapper_exits() {
        use std::fs;
        use std::os::unix::process::CommandExt;
        use std::process::Command;
        use tempfile::tempdir;
        let temp = tempdir().unwrap();
        let pid_path = temp.path().join("grandchild.pid");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("trap '' HUP; sleep 60 & echo $! > \"$1\"")
            .arg("sh")
            .arg(&pid_path);
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        for _ in 0..50 {
            if pid_path.exists() && child.try_wait().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(pid_path.exists());
        assert!(child.try_wait().unwrap().is_some());
        assert!(process_group_alive(pid));
        terminate_child(&mut child).unwrap();
        assert!(
            !process_group_alive(pid),
            "grandchild remained: {}",
            fs::read_to_string(pid_path).unwrap_or_default()
        );
    }
}
