//! CLI lifetime supervision. The worker, not its invoking CLI, owns app trees.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::{
    ACTIVE_SESSION_GENERATION, OWNED_RESOURCE_STATE, RESOURCES_ARMED, arm_owned_resources,
    force_cleanup_requested, handle_termination_signal, start_termination_cleanup_session,
    termination_requested,
};

const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Internal CLI adapter. Runs an owning dev worker in a separate Unix session.
///
/// # Errors
///
/// Returns an error if lifetime-channel setup, worker spawn, signal forwarding,
/// or waiting for the owned worker fails.
#[doc(hidden)]
pub fn launch_dev_worker(mut command: Command) -> Result<ExitStatus> {
    let _signals = start_termination_cleanup_session()?;
    arm_owned_resources()?;
    let (mut launcher, worker) =
        UnixStream::pair().context("create dev worker lifetime channel")?;
    launcher
        .set_nonblocking(true)
        .context("configure dev launcher lifetime channel")?;
    let worker_fd = worker.as_raw_fd();
    command.arg(format!("--jig-worker-fd={worker_fd}"));
    // Both endpoints start close-on-exec. Only the worker endpoint crosses this
    // exec; the frontend endpoint must never be held by the worker or its apps.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            set_close_on_exec(worker_fd, false)
        });
    }
    let mut child = command.spawn().context("start owning dev worker")?;
    drop(worker);
    let outcome = wait_for_worker(&mut child, &mut launcher);
    // Also close on any failure: the worker can clean up even if the frontend
    // cannot continue waiting or forwarding signals.
    drop(launcher);
    outcome
}

fn wait_for_worker(child: &mut Child, launcher: &mut UnixStream) -> Result<ExitStatus> {
    let mut forwarded_first = false;
    let mut forwarded_force = false;
    loop {
        if let Some(status) = child.try_wait().context("wait for owning dev worker")? {
            return Ok(status);
        }
        if let Some(reason) = termination_requested() {
            if !forwarded_first {
                forwarded_first = forward_termination(launcher, reason.signal())?;
            } else if force_cleanup_requested() && !forwarded_force {
                forwarded_force = forward_termination(launcher, reason.signal())?;
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn forward_termination(launcher: &mut UnixStream, signal: i32) -> Result<bool> {
    let signal = u8::try_from(signal).context("invalid dev termination signal")?;
    // The private stream preserves first/force ordering even while the worker
    // is stopped. Sending the same OS signal twice could coalesce into one.
    match launcher.write(&[signal]) {
        Ok(1) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        // The worker can close its endpoint immediately before process exit.
        // Wait for that exact child's exit status instead of replacing it.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
            ) =>
        {
            Ok(true)
        }
        Err(error) => Err(error).context("forward termination to owning dev worker"),
        Ok(_) => bail!("dev worker lifetime channel accepted no termination request"),
    }
}

/// The worker's private connection to the invoking CLI.
#[doc(hidden)]
pub struct DevLauncherWatch {
    stopped: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl DevLauncherWatch {
    /// Takes the socket passed exclusively to a CLI worker by `launch_dev_worker`.
    ///
    /// # Safety
    ///
    /// `fd` must be an inherited descriptor with no other Rust owner. On success
    /// or failure after validation, this function takes ownership of it.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid descriptor, socket, or watcher setup.
    pub unsafe fn from_inherited_fd(fd: RawFd) -> Result<Self> {
        validate_worker_fd(fd)?;
        // SAFETY: the caller transfers this valid inherited socket exclusively.
        let socket = unsafe { UnixStream::from_raw_fd(fd) };
        set_close_on_exec(fd, true).context("protect dev worker lifetime descriptor")?;
        socket
            .peer_addr()
            .context("inspect dev worker lifetime peer")?;
        socket
            .set_nonblocking(true)
            .context("configure dev worker lifetime channel")?;
        let stopped = Arc::new(AtomicBool::new(false));
        let reader_stopped = Arc::clone(&stopped);
        let reader = thread::Builder::new()
            .name("jig-dev-launcher-watch".into())
            .spawn(move || watch_launcher(socket, &reader_stopped))
            .context("start dev launcher lifetime watcher")?;
        Ok(Self {
            stopped,
            reader: Some(reader),
        })
    }
}

impl Drop for DevLauncherWatch {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn validate_worker_fd(fd: RawFd) -> Result<()> {
    if fd < 3 {
        bail!("invalid private dev worker descriptor");
    }
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: getsockopt validates the numeric descriptor, and both output
    // pointers address initialized, correctly sized storage.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            std::ptr::from_mut(&mut kind).cast(),
            &mut length,
        )
    } == -1
    {
        return Err(io::Error::last_os_error()).context("invalid private dev worker socket");
    }
    if kind != libc::SOCK_STREAM {
        bail!("private dev worker channel must be a stream socket");
    }
    Ok(())
}

fn set_close_on_exec(fd: RawFd, enabled: bool) -> io::Result<()> {
    // SAFETY: fcntl accepts a numeric descriptor and validates it in the kernel.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    let flags = if enabled {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    // SAFETY: only descriptor flags are changed; no pointer argument is used.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn watch_launcher(mut socket: UnixStream, stopped: &AtomicBool) {
    let mut byte = [0u8; 1];
    while !stopped.load(Ordering::Acquire) {
        match socket.read(&mut byte) {
            Ok(1)
                if matches!(
                    i32::from(byte[0]),
                    libc::SIGINT | libc::SIGHUP | libc::SIGTERM
                ) =>
            {
                deliver_termination(i32::from(byte[0]), stopped);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // EOF, invalid input, or a broken lifetime channel requests ordinary
            // cleanup, including preflight and readiness cancellation.
            _ => {
                if !stopped.load(Ordering::Acquire) {
                    // Use the same generation and in-flight accounting as the
                    // signal handler. No process ID is used as signal authority.
                    deliver_termination(libc::SIGTERM, stopped);
                }
                return;
            }
        }
    }
}

fn deliver_termination(signal: i32, stopped: &AtomicBool) {
    // CLI validation precedes the lifecycle. Keep a queued request until its
    // handler and resource guard exist, so an early request cannot bypass JSON
    // error reporting with the no-resources immediate-exit path. If validation
    // fails, the CLI drops this watcher without ever spawning an app.
    while !stopped.load(Ordering::Acquire) {
        if ACTIVE_SESSION_GENERATION.load(Ordering::SeqCst) != 0
            && OWNED_RESOURCE_STATE.load(Ordering::SeqCst) == RESOURCES_ARMED
        {
            handle_termination_signal(signal);
            return;
        }
        thread::sleep(POLL_INTERVAL);
    }
}
