use std::process::Child;
#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use crate::state::StateStore;

use super::{CTRL_C_HANDLER, CTRL_C_REQUESTED};

pub(super) struct RunningChild {
    pub(super) name: String,
    pub(super) hostname: String,
    pub(super) proxied: bool,
    pub(super) store: StateStore,
    pub(super) child: Child,
    pub(super) cleanup_armed: bool,
}

impl RunningChild {
    fn cleanup(&mut self) {
        if !self.cleanup_armed {
            return;
        }
        if self.proxied {
            if let Err(error) = self.store.remove_route(&self.hostname) {
                eprintln!(
                    "jig proxy could not remove route '{}' while cleaning up '{}': {error}",
                    self.hostname, self.name
                );
            }
        }
        terminate_child(&mut self.child);
        // This guard also covers panic/unwind cleanup. Keep reaping bounded so
        // cleanup cannot hang after best-effort termination.
        wait_after_terminate(&mut self.child);
        self.cleanup_armed = false;
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub(super) fn cleanup_children(children: &mut [RunningChild]) {
    for running in children {
        running.cleanup();
    }
}

pub(super) fn wait_after_terminate(child: &mut Child) {
    // Cleanup paths keep the original startup/watch error as primary. Waiting
    // here only tries to reap the terminated child; failures are not actionable
    // after terminate_child has already performed the best-effort kill/escalation.
    // If the child is still present after the deadline, process exit will reap
    // it; this path must not block the user-facing startup error.
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                eprintln!("jig proxy could not reap terminated child process: {error}");
                return;
            }
        }
    }
}

#[cfg(unix)]
pub(super) fn terminate_child(child: &mut Child) {
    let pid = child.id();
    let mut direct_child_exited = child.try_wait().ok().flatten().is_some();
    if direct_child_exited {
        terminate_process_group(pid);
    } else {
        terminate_pid(pid);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !direct_child_exited && child.try_wait().ok().flatten().is_some() {
            direct_child_exited = true;
        }
        let alive = if direct_child_exited {
            process_group_alive(pid)
        } else {
            process_group_or_pid_alive(pid)
        };
        if !alive {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if direct_child_exited {
        kill_process_group(pid);
    } else {
        kill_pid(pid);
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
pub(super) fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        terminate_pid(child.id());
    }
}

pub(super) fn start_ctrlc_cleanup_session() {
    CTRL_C_REQUESTED.store(false, Ordering::SeqCst);
    CTRL_C_HANDLER.get_or_init(|| {
        if let Err(error) = ctrlc::set_handler(|| {
            CTRL_C_REQUESTED.store(true, Ordering::SeqCst);
        }) {
            eprintln!("jig proxy could not install Ctrl-C cleanup handler: {error}");
        }
    });
}

pub(super) fn ctrl_c_requested() -> bool {
    CTRL_C_REQUESTED.load(Ordering::SeqCst)
}

#[cfg(unix)]
pub(super) fn terminate_pid(pid: u32) {
    if let Some(pid) = unix_pid(pid) {
        signal_unix_process_group_or_pid(pid, libc::SIGTERM);
    }
}

#[cfg(unix)]
pub(super) fn terminate_process_group(pid: u32) {
    if let Some(pid) = unix_pid(pid) {
        signal_unix_process_group(pid, libc::SIGTERM);
    }
}

#[cfg(unix)]
pub(super) fn kill_pid(pid: u32) {
    if let Some(pid) = unix_pid(pid) {
        signal_unix_process_group_or_pid(pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
pub(super) fn kill_process_group(pid: u32) {
    if let Some(pid) = unix_pid(pid) {
        signal_unix_process_group(pid, libc::SIGKILL);
    }
}

#[cfg(unix)]
pub(super) fn signal_unix_process_group_or_pid(pid: i32, signal: i32) {
    unsafe {
        // SAFETY: pid was range-checked before this helper is called and signal
        // is one of the libc termination constants used by this module.
        if libc::kill(-pid, signal) == -1 {
            let _ = libc::kill(pid, signal);
        }
    }
}

#[cfg(unix)]
pub(super) fn signal_unix_process_group(pid: i32, signal: i32) {
    unsafe {
        // SAFETY: pid was range-checked before this helper is called and signal
        // is one of the libc termination constants used by this module.
        let _ = libc::kill(-pid, signal);
    }
}

#[cfg(unix)]
pub(super) fn process_group_or_pid_alive(pid: u32) -> bool {
    let Some(pid) = unix_pid(pid) else {
        return false;
    };
    unsafe {
        // SAFETY: pid was range-checked above. Signal 0 performs permission and
        // existence checks without delivering a signal.
        libc::kill(-pid, 0) == 0 || libc::kill(pid, 0) == 0
    }
}

#[cfg(unix)]
pub(super) fn process_group_alive(pid: u32) -> bool {
    let Some(pid) = unix_pid(pid) else {
        return false;
    };
    unsafe {
        // SAFETY: pid was range-checked above. Signal 0 performs permission and
        // existence checks without delivering a signal.
        libc::kill(-pid, 0) == 0
    }
}

#[cfg(unix)]
pub(super) fn unix_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok()
}

#[cfg(windows)]
pub(super) fn terminate_pid(pid: u32) {
    let _ = Command::new(windows_system32_tool("taskkill.exe"))
        .env_clear()
        .args(["/PID", &pid.to_string(), "/T"])
        .status();
    if wait_for_pid_exit(pid, Duration::from_secs(2)) {
        return;
    }
    let _ = Command::new(windows_system32_tool("taskkill.exe"))
        .env_clear()
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
    let _ = wait_for_pid_exit(pid, Duration::from_secs(1));
}

#[cfg(windows)]
pub(super) fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
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
pub(super) fn windows_system32_tool(name: &str) -> std::path::PathBuf {
    // Use the canonical system directory instead of a mutable environment
    // variable so cleanup keeps using the OS taskkill binary.
    std::path::PathBuf::from(r"C:\Windows\System32").join(name)
}

#[cfg(windows)]
pub(super) fn kill_pid(pid: u32) {
    terminate_pid(pid);
}

#[cfg(not(any(unix, windows)))]
pub(super) fn terminate_pid(_pid: u32) {}

#[cfg(not(any(unix, windows)))]
pub(super) fn kill_pid(_pid: u32) {}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn terminate_child_kills_process_group_after_wrapper_exits() {
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
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = command.spawn().unwrap();
        let child_pid = child.id();

        for _ in 0..50 {
            if pid_path.exists() && child.try_wait().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(pid_path.exists(), "wrapper did not write grandchild pid");
        assert!(
            child.try_wait().unwrap().is_some(),
            "wrapper did not exit before cleanup"
        );
        assert!(
            process_group_alive(child_pid),
            "grandchild process group was not alive before cleanup"
        );

        terminate_child(&mut child);

        for _ in 0..50 {
            if !process_group_alive(child_pid) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        if process_group_alive(child_pid) {
            kill_process_group(child_pid);
        }
        assert!(
            !process_group_alive(child_pid),
            "cleanup left the wrapper process group alive; grandchild pid file: {}",
            fs::read_to_string(pid_path).unwrap_or_default().trim()
        );
    }
}
