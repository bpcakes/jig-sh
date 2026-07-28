#[cfg(any(windows, test))]
use std::collections::HashMap;
use std::io;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
#[cfg(windows)]
use std::process::Command;
use std::process::{Child, ExitStatus};
#[cfg(windows)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

#[cfg(unix)]
use crate::unix_pid;

use super::cleanup::force_cleanup_requested;

const REAP_TIMEOUT: Duration = Duration::from_secs(2);
const TERMINATE_TIMEOUT: Duration = Duration::from_secs(2);
const KILL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(1);

#[cfg(any(windows, test))]
struct WindowsAppJobEntry<H> {
    generation: u64,
    handle: H,
}

#[cfg(windows)]
static WINDOWS_APP_JOBS: OnceLock<Mutex<HashMap<u32, WindowsAppJobEntry<OwnedHandle>>>> =
    OnceLock::new();
#[cfg(windows)]
static NEXT_WINDOWS_APP_JOB_GENERATION: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
fn windows_app_jobs() -> &'static Mutex<HashMap<u32, WindowsAppJobEntry<OwnedHandle>>> {
    WINDOWS_APP_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(windows)]
pub(super) struct AppProcessLease {
    pid: u32,
    generation: u64,
}

#[cfg(not(windows))]
#[derive(Default)]
pub(super) struct AppProcessLease;

#[cfg(windows)]
impl Drop for AppProcessLease {
    fn drop(&mut self) {
        let mut jobs = match windows_app_jobs().lock() {
            Ok(jobs) => jobs,
            Err(poisoned) => {
                eprintln!(
                    "jig proxy app process job registry mutex was poisoned during final lease cleanup; recovering the owned registry entry"
                );
                poisoned.into_inner()
            }
        };
        // An old lease may outlive successful explicit cleanup and PID reuse.
        // Only its own generation is allowed to close a kill-on-close handle.
        drop(remove_job_entry_if_generation(
            &mut jobs,
            self.pid,
            self.generation,
        ));
    }
}

#[cfg(any(windows, test))]
fn remove_job_entry_if_generation<H>(
    jobs: &mut HashMap<u32, WindowsAppJobEntry<H>>,
    pid: u32,
    generation: u64,
) -> Option<H> {
    if jobs.get(&pid).map(|entry| entry.generation) != Some(generation) {
        return None;
    }
    jobs.remove(&pid).map(|entry| entry.handle)
}

#[cfg(not(windows))]
pub(super) const fn register_app_child(_child: &mut Child) -> Result<AppProcessLease> {
    Ok(AppProcessLease)
}

#[cfg(windows)]
pub(super) fn register_app_child(child: &mut Child) -> Result<AppProcessLease> {
    let pid = child.id();
    let generation = NEXT_WINDOWS_APP_JOB_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
            generation.checked_add(1)
        })
        .map_err(|_| anyhow!("app process job generation space exhausted"))?;
    let raw_job = unsafe {
        // SAFETY: null attributes/name request a private job with default
        // security. The returned owned handle is checked before conversion.
        CreateJobObjectW(std::ptr::null(), std::ptr::null())
    };
    if raw_job.is_null() {
        return Err(io::Error::last_os_error()).context("failed to create app process job");
    }
    let job = unsafe {
        // SAFETY: CreateJobObjectW returned a new, non-null owned handle.
        OwnedHandle::from_raw_handle(raw_job as _)
    };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        // SAFETY: job is live and limits is a fully initialized structure of
        // the exact size supplied to SetInformationJobObject.
        SetInformationJobObject(
            job.as_raw_handle() as _,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error())
            .context("failed to configure app process job cleanup");
    }
    if unsafe {
        // SAFETY: both handles are live. The child was created suspended, so
        // it cannot create an untracked descendant before this assignment.
        AssignProcessToJobObject(job.as_raw_handle() as _, child.as_raw_handle() as _)
    } == 0
    {
        return Err(io::Error::last_os_error()).context("failed to assign app process to job");
    }

    {
        let mut jobs = windows_app_jobs()
            .lock()
            .map_err(|_| anyhow!("app process job registry mutex was poisoned"))?;
        if jobs.contains_key(&pid) {
            bail!("app process job registry already contained child PID {pid}");
        }
        jobs.insert(
            pid,
            WindowsAppJobEntry {
                generation,
                handle: job,
            },
        );
    }
    let lease = AppProcessLease { pid, generation };
    if let Err(error) = resume_suspended_windows_process(pid) {
        drop(lease);
        return Err(error);
    }
    Ok(lease)
}

#[cfg(windows)]
fn resume_suspended_windows_process(pid: u32) -> Result<()> {
    let raw_snapshot = unsafe {
        // SAFETY: TH32CS_SNAPTHREAD takes no process pointer and returns a new
        // snapshot handle which is checked before conversion.
        CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    };
    if raw_snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error())
            .context("failed to enumerate suspended app process threads");
    }
    let snapshot = unsafe {
        // SAFETY: CreateToolhelp32Snapshot returned a valid owned handle.
        OwnedHandle::from_raw_handle(raw_snapshot as _)
    };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut found = false;
    let mut has_entry = unsafe {
        // SAFETY: snapshot is live and entry points to writable storage with
        // the required dwSize initialized.
        Thread32First(snapshot.as_raw_handle() as _, &mut entry)
    } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == pid {
            let raw_thread = unsafe {
                // SAFETY: the enumerated thread id is used only to request the
                // minimal resume right, and the returned handle is checked.
                OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID)
            };
            if raw_thread.is_null() {
                return Err(io::Error::last_os_error())
                    .with_context(|| format!("failed to open suspended app thread for PID {pid}"));
            }
            let thread = unsafe {
                // SAFETY: OpenThread returned a new, non-null owned handle.
                OwnedHandle::from_raw_handle(raw_thread as _)
            };
            if unsafe {
                // SAFETY: thread has THREAD_SUSPEND_RESUME access and remains
                // live for the duration of the call.
                ResumeThread(thread.as_raw_handle() as _)
            } == u32::MAX
            {
                return Err(io::Error::last_os_error())
                    .with_context(|| format!("failed to resume app thread for PID {pid}"));
            }
            found = true;
        }
        has_entry = unsafe {
            // SAFETY: snapshot and entry remain valid across enumeration.
            Thread32Next(snapshot.as_raw_handle() as _, &mut entry)
        } != 0;
    }
    let enumeration_error = io::Error::last_os_error();
    if enumeration_error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(enumeration_error)
            .with_context(|| format!("failed while enumerating app threads for PID {pid}"));
    }
    if !found {
        bail!("could not find the suspended primary app thread for PID {pid}");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildObservation {
    Running,
    ExitedUnreaped(ExitStatus),
    ExitedReaped(ExitStatus),
}

pub(super) fn try_wait_preserving_process_group(
    child: &mut Child,
) -> io::Result<Option<ExitStatus>> {
    Ok(match observe_child(child)? {
        ChildObservation::Running => None,
        ChildObservation::ExitedUnreaped(status) | ChildObservation::ExitedReaped(status) => {
            Some(status)
        }
    })
}

#[cfg(all(unix, not(target_os = "redox")))]
fn observe_child(child: &mut Child) -> io::Result<ChildObservation> {
    let pid = checked_pid_io(child.id())?;
    loop {
        let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `information` is writable `siginfo_t` storage, `pid` names our
        // direct child, and WNOWAIT deliberately preserves the wait status so
        // the zombie leader continues to pin its PID/process-group identity.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as _,
                information.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            // SAFETY: successful waitid initialized `information`.
            let information = unsafe { information.assume_init() };
            // SAFETY: si_pid/si_status access the SIGCHLD fields populated by
            // waitid for this direct child.
            return classify_waitid_child_observation(
                pid,
                unsafe { information.si_pid() },
                information.si_code,
                unsafe { information.si_status() },
            );
        }

        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.raw_os_error() == Some(libc::ECHILD) {
            // Child caches statuses it reaps itself. If some other SIGCHLD
            // consumer reaped the leader, propagate ECHILD rather than ever
            // treating the now-recyclable numeric PID as an owned group.
            return child
                .try_wait()?
                .map(ChildObservation::ExitedReaped)
                .ok_or(error);
        }
        return Err(error);
    }
}

#[cfg(all(unix, not(target_os = "redox")))]
fn classify_waitid_child_observation(
    expected_pid: libc::pid_t,
    observed_pid: libc::pid_t,
    code: libc::c_int,
    status: libc::c_int,
) -> io::Result<ChildObservation> {
    if observed_pid == 0 {
        return Ok(ChildObservation::Running);
    }
    if observed_pid != expected_pid {
        return Err(io::Error::other(format!(
            "waitid observed unexpected child PID {observed_pid} instead of {expected_pid}"
        )));
    }
    let raw = match code {
        libc::CLD_EXITED => status << 8,
        libc::CLD_KILLED => status,
        libc::CLD_DUMPED => status | 0x80,
        libc::CLD_STOPPED | libc::CLD_TRAPPED | libc::CLD_CONTINUED => {
            return Ok(ChildObservation::Running);
        }
        code => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("waitid returned unexpected child status code {code}"),
            ));
        }
    };
    Ok(ChildObservation::ExitedUnreaped(ExitStatus::from_raw(raw)))
}

#[cfg(any(not(unix), target_os = "redox"))]
fn observe_child(child: &mut Child) -> io::Result<ChildObservation> {
    Ok(match child.try_wait()? {
        Some(status) => ChildObservation::ExitedReaped(status),
        None => ChildObservation::Running,
    })
}

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

#[cfg(unix)]
fn child_cleanup_deadline(pid: u32, phase: &str, timeout: Duration) -> Result<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("child process {pid} {phase} deadline overflowed"))
}

#[cfg(any(unix, test))]
fn remaining_phase_budget(deadline: Instant, now: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
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
    let direct_child_exited = match observe_child(child)
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
    {
        ChildObservation::Running => false,
        ChildObservation::ExitedUnreaped(_) => true,
        // Once reaped, the numeric PID/PGID may already have been recycled.
        // Refuse every process-group signal rather than risk another tree.
        ChildObservation::ExitedReaped(_) => return Ok(()),
    };
    if force_cleanup_requested() {
        return force_kill_child(child);
    }
    let term_deadline = child_cleanup_deadline(pid, "SIGTERM grace", TERMINATE_TIMEOUT)?;
    if direct_child_exited {
        let signal_result = signal_exited_process_group(child, pid, libc::SIGTERM);
        return finish_exited_process_group_after_term_signal(
            child,
            pid,
            term_deadline,
            signal_result,
            finish_exited_process_group_after_term,
            force_kill_child,
        );
    } else if let Err(error) = terminate_pid(pid) {
        return force_after_graceful_cleanup_error(
            child,
            &error,
            &format!("failed to send SIGTERM to child process/group {pid}"),
            force_kill_child,
        );
    }
    loop {
        let Some(_) = remaining_phase_budget(term_deadline, Instant::now()) else {
            break;
        };
        if force_cleanup_requested() {
            return force_kill_child(child);
        }
        if !direct_child_exited {
            let observation = match observe_child(child) {
                Ok(observation) => observation,
                Err(error) => {
                    return force_after_graceful_cleanup_error(
                        child,
                        &error,
                        &format!("failed to inspect child process {pid} during termination"),
                        force_kill_child,
                    );
                }
            };
            match observation {
                ChildObservation::Running => {}
                ChildObservation::ExitedUnreaped(_) => {
                    return finish_exited_process_group_after_term(child, pid, term_deadline);
                }
                ChildObservation::ExitedReaped(_) => return Ok(()),
            }
        }
        let target_alive = match running_child_target_alive(child, pid) {
            Ok(target_alive) => target_alive,
            Err(error) => {
                return force_after_graceful_cleanup_error(
                    child,
                    &error,
                    &format!("failed to confirm child process/group {pid} liveness after SIGTERM"),
                    force_kill_child,
                );
            }
        };
        if !target_alive {
            return Ok(());
        }
        let Some(remaining) = remaining_phase_budget(term_deadline, Instant::now()) else {
            break;
        };
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    force_kill_child(child)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn finish_exited_process_group_after_term_signal<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    signal_result: Result<bool>,
    finish_grace: impl FnOnce(&mut T, u32, Instant) -> Result<()>,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    match signal_result {
        // `false` means the group signal reported ESRCH (or macOS EPERM after
        // a sole-leader snapshot). Neither signal result alone replaces the
        // normal TERM-grace transition into forced signal and confirmation.
        Ok(_) => finish_grace(state, pid, deadline),
        Err(error) => force_after_graceful_cleanup_error(
            state,
            &error,
            &format!("failed to send SIGTERM to exited child process group {pid}"),
            force_cleanup,
        ),
    }
}

#[cfg(unix)]
fn finish_exited_process_group_after_term(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    // The unreaped leader pins the process-group identity. Give live
    // descendants their normal TERM grace period, but exclude that zombie
    // leader from liveness. An empty TERM-phase scan is only a transition to
    // SIGKILL: a TERM-ignoring member could fork and exit across snapshots, so
    // only post-SIGKILL empty scans can prove that the group is quiescent.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        wait_scannable_exited_group_term_grace(
            child,
            pid,
            deadline,
            exited_process_group_has_live_members,
            Instant::now,
            thread::sleep,
            force_cleanup_requested,
            force_kill_child,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        wait_unscannable_exited_group_term_grace(
            child,
            deadline,
            Instant::now,
            thread::sleep,
            force_cleanup_requested,
            force_kill_child,
        )
    }
}

#[cfg(any(test, all(unix, any(target_os = "linux", target_os = "macos"))))]
#[allow(clippy::too_many_arguments)]
fn wait_scannable_exited_group_term_grace<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    mut live_members: impl FnMut(&mut T, u32, Instant) -> Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    mut forced: impl FnMut() -> bool,
    mut force_cleanup: impl FnMut(&mut T) -> Result<()>,
) -> Result<()> {
    loop {
        let Some(_) = remaining_phase_budget(deadline, now()) else {
            return force_cleanup(state);
        };
        if forced() {
            return force_cleanup(state);
        }
        let live_members = live_members(state, pid, deadline);
        let keep_waiting = continue_exited_group_term_grace(
            state,
            live_members,
            &format!(
                "failed to confirm live membership of exited child process group {pid} after SIGTERM"
            ),
            |state| force_cleanup(state),
        )?;
        if !keep_waiting {
            return Ok(());
        }
        let Some(remaining) = remaining_phase_budget(deadline, now()) else {
            return force_cleanup(state);
        };
        sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(any(test, all(unix, not(any(target_os = "linux", target_os = "macos")))))]
fn wait_unscannable_exited_group_term_grace<T>(
    state: &mut T,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    mut forced: impl FnMut() -> bool,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    loop {
        let Some(remaining) = remaining_phase_budget(deadline, now()) else {
            break;
        };
        if forced() {
            return force_cleanup(state);
        }
        sleep(remaining.min(Duration::from_millis(10)));
    }
    force_cleanup(state)
}

#[cfg(unix)]
fn continue_exited_group_term_grace<T>(
    state: &mut T,
    live_members: Result<bool>,
    context: &str,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<bool> {
    match live_members {
        Ok(true) => Ok(true),
        Ok(false) => {
            force_cleanup(state)?;
            Ok(false)
        }
        Err(error) => {
            force_after_graceful_cleanup_error(state, &error, context, force_cleanup)?;
            Ok(false)
        }
    }
}

#[cfg(unix)]
fn force_after_graceful_cleanup_error<T>(
    state: &mut T,
    graceful_error: &dyn std::fmt::Display,
    context: &str,
    force_cleanup: impl FnOnce(&mut T) -> Result<()>,
) -> Result<()> {
    force_cleanup(state).with_context(|| {
        format!(
            "{context}; forced cleanup also failed after graceful cleanup error: {graceful_error}"
        )
    })
}

#[cfg(all(unix, target_os = "linux"))]
fn exited_process_group_has_live_members(
    _child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<bool> {
    linux_process_group_has_live_members(pid, deadline)
}

#[cfg(all(unix, target_os = "macos"))]
fn exited_process_group_has_live_members(
    child: &mut Child,
    pid: u32,
    _deadline: Instant,
) -> Result<bool> {
    macos_process_group_is_quiescent(child, pid)
        .map(|quiescent| !quiescent)
        .map_err(Into::into)
}

#[cfg(unix)]
fn force_kill_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    let deadline = child_cleanup_deadline(pid, "SIGKILL confirmation", KILL_CONFIRM_TIMEOUT)?;
    let observation = match observe_child(child) {
        Ok(observation) => observation,
        Err(error) if error.raw_os_error() == Some(libc::ECHILD) => {
            return Err(error).with_context(|| {
                format!(
                    "lost ownership of child process {pid} before forced cleanup; refusing to signal a recyclable process identity"
                )
            });
        }
        Err(error) => {
            // terminate_child established direct-child ownership before any
            // graceful work. A non-ECHILD waitid failure does not consume that
            // wait status, so force the pinned identity instead of allowing a
            // persistent observation error to suppress SIGKILL entirely.
            return force_kill_pinned_child(child, pid, deadline).with_context(|| {
                format!(
                    "failed to reobserve pinned child process {pid} before forced cleanup: {error}"
                )
            });
        }
    };
    match observation {
        ChildObservation::ExitedUnreaped(_) => {
            confirm_exited_process_group_not_live(child, pid, deadline)
        }
        ChildObservation::ExitedReaped(_) => Ok(()),
        ChildObservation::Running => force_kill_pinned_child(child, pid, deadline),
    }
}

#[cfg(unix)]
fn force_kill_pinned_child(child: &mut Child, pid: u32, deadline: Instant) -> Result<()> {
    let signal_result = kill_pid(pid);
    let child_result = child
        .kill()
        .with_context(|| format!("failed final Child::kill for process {pid}"));
    if let Err(signal_error) = signal_result {
        if let Err(child_error) = child_result {
            return Err(
                signal_error.context(format!("fallback child kill also failed: {child_error:#}"))
            );
        }
    }
    wait_for_killed_child_exit_and_confirm(child, pid, deadline)
}

#[cfg(unix)]
fn wait_for_killed_child_exit_and_confirm(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    wait_for_killed_child_exit_and_confirm_with(
        child,
        pid,
        deadline,
        observe_child,
        Instant::now,
        thread::sleep,
        confirm_exited_process_group_not_live,
    )
}

#[cfg(any(unix, test))]
#[allow(clippy::too_many_arguments)]
fn wait_for_killed_child_exit_and_confirm_with<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    mut observe: impl FnMut(&mut T) -> io::Result<ChildObservation>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    mut confirm: impl FnMut(&mut T, u32, Instant) -> Result<()>,
) -> Result<()> {
    loop {
        match observe(state)
            .with_context(|| format!("failed to observe child process {pid} after SIGKILL"))?
        {
            ChildObservation::Running => {
                let Some(remaining) = remaining_phase_budget(deadline, now()) else {
                    bail!(
                        "child process {pid} remained running through its SIGKILL confirmation deadline"
                    )
                };
                sleep(remaining.min(Duration::from_millis(10)));
            }
            ChildObservation::ExitedUnreaped(_) => {
                return confirm(state, pid, deadline);
            }
            ChildObservation::ExitedReaped(_) => {
                bail!(
                    "child process {pid} was reaped before its process group cleanup could be confirmed"
                )
            }
        }
    }
}

#[cfg(all(not(unix), not(windows)))]
pub(super) fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    if child
        .try_wait()
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
        .is_none()
    {
        if force_cleanup_requested() {
            kill_pid(pid)?;
        } else {
            terminate_pid(pid)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child.id();
    let exited = child
        .try_wait()
        .with_context(|| format!("failed to inspect child process {pid} before termination"))?
        .is_some();
    match windows_app_job_active_processes(pid)? {
        Some(0) => {
            remove_windows_app_job(pid)?;
            return Ok(());
        }
        Some(_) => {}
        None if exited => return Ok(()),
        None if force_cleanup_requested() => return kill_pid(pid),
        None => return terminate_pid(pid),
    }

    {
        return terminate_registered_windows_job_with(
            pid,
            force_cleanup_requested(),
            || generate_windows_console_break(pid),
            || run_taskkill(pid, false),
            |timeout| wait_for_windows_app_job_empty(pid, timeout),
            || terminate_windows_app_job(pid),
            |timeout| wait_for_windows_app_job_empty(pid, timeout),
            || remove_windows_app_job(pid),
        );
    }
}

#[cfg(any(windows, test))]
#[allow(clippy::too_many_arguments)]
fn terminate_registered_windows_job_with(
    pid: u32,
    forced: bool,
    console_break: impl FnOnce() -> Result<()>,
    fallback_taskkill: impl FnOnce() -> Result<()>,
    wait_after_graceful: impl FnOnce(Duration) -> Result<bool>,
    terminate_job: impl FnOnce() -> Result<()>,
    confirm_after_terminate: impl FnOnce(Duration) -> Result<bool>,
    release_job: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut graceful_error = None;
    if !forced {
        let delivered = match console_break() {
            Ok(()) => Ok(()),
            Err(console_error) => fallback_taskkill().with_context(|| {
                format!(
                    "fallback taskkill failed after targeted CTRL+BREAK delivery failed: {console_error:#}"
                )
            }),
        };
        match delivered {
            Ok(()) => match wait_after_graceful(TERMINATE_TIMEOUT) {
                Ok(true) => return release_job(),
                Ok(false) => {}
                Err(error) => graceful_error = Some(error),
            },
            Err(error) => graceful_error = Some(error),
        }
    }

    if let Err(error) = terminate_job() {
        return match graceful_error {
            Some(graceful_error) => Err(error.context(format!(
                "forced app-job termination also failed after graceful cleanup error: {graceful_error:#}"
            ))),
            None => Err(error),
        };
    }

    let confirmation = match confirm_after_terminate(KILL_CONFIRM_TIMEOUT) {
        Ok(true) => Ok(()),
        Ok(false) => Err(anyhow!(
            "app process job for child {pid} retained active descendants after forced termination"
        )),
        Err(error) => Err(error),
    };
    let release = release_job();
    match (confirmation, release) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(release_error)) => Err(error.context(format!(
            "app process job handle release also failed: {release_error:#}"
        ))),
    }
}

#[cfg(windows)]
fn generate_windows_console_break(pid: u32) -> Result<()> {
    if unsafe {
        // SAFETY: app children are created with CREATE_NEW_PROCESS_GROUP, so
        // their PID is the process-group id. CTRL+BREAK is the only console
        // control event Windows permits targeting to one process group.
        GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid)
    } == 0
    {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("failed to send CTRL+BREAK to app process group {pid}"));
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn windows_app_job_active_processes(pid: u32) -> Result<Option<u32>> {
    let jobs = windows_app_jobs()
        .lock()
        .map_err(|_| anyhow!("app process job registry mutex was poisoned"))?;
    let Some(job) = jobs.get(&pid) else {
        return Ok(None);
    };
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    if unsafe {
        // SAFETY: job is live and accounting is writable storage of the exact
        // size supplied to QueryInformationJobObject.
        QueryInformationJobObject(
            job.handle.as_raw_handle() as _,
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast(),
            std::mem::size_of_val(&accounting) as u32,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("failed to inspect app process job for child {pid}"));
    }
    Ok(Some(accounting.ActiveProcesses))
}

#[cfg(windows)]
fn wait_for_windows_app_job_empty(pid: u32, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        match windows_app_job_active_processes(pid)? {
            Some(0) | None => return Ok(true),
            Some(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Some(_) => return Ok(false),
        }
    }
}

#[cfg(windows)]
fn terminate_windows_app_job(pid: u32) -> Result<()> {
    let jobs = windows_app_jobs()
        .lock()
        .map_err(|_| anyhow!("app process job registry mutex was poisoned"))?;
    let Some(job) = jobs.get(&pid) else {
        return Ok(());
    };
    if unsafe {
        // SAFETY: job is a live handle owned by the registry.
        TerminateJobObject(job.handle.as_raw_handle() as _, 1)
    } == 0
    {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("failed to terminate app process job for child {pid}"));
    }
    Ok(())
}

#[cfg(windows)]
fn remove_windows_app_job(pid: u32) -> Result<()> {
    let mut jobs = windows_app_jobs()
        .lock()
        .map_err(|_| anyhow!("app process job registry mutex was poisoned"))?;
    jobs.remove(&pid);
    Ok(())
}

#[cfg(any(unix, test))]
#[allow(clippy::too_many_arguments)]
fn confirm_exited_process_group_not_live_with<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    required_empty_proofs: u8,
    mut resignal: impl FnMut(&mut T, u32, Instant) -> Result<()>,
    mut live_members: impl FnMut(&mut T, u32, Instant) -> Result<bool>,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> Result<()> {
    if required_empty_proofs == 0 {
        bail!("child process group {pid} confirmation requires at least one empty proof");
    }
    let mut consecutive_empty_proofs = 0_u8;
    loop {
        let Some(_) = remaining_phase_budget(deadline, now()) else {
            bail!(
                "child process group {pid} retained live members through its SIGKILL confirmation deadline"
            );
        };
        resignal(state, pid, deadline)?;
        let Some(_) = remaining_phase_budget(deadline, now()) else {
            bail!(
                "child process group {pid} retained live members through its SIGKILL confirmation deadline"
            );
        };
        if live_members(state, pid, deadline)? {
            consecutive_empty_proofs = 0;
        } else {
            consecutive_empty_proofs += 1;
        }
        let Some(remaining) = remaining_phase_budget(deadline, now()) else {
            bail!(
                "child process group {pid} retained live members through its SIGKILL confirmation deadline"
            )
        };
        if consecutive_empty_proofs == required_empty_proofs {
            return Ok(());
        }
        sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(all(unix, target_os = "macos"))]
fn running_child_target_alive(child: &mut Child, pid: u32) -> io::Result<bool> {
    match process_group_or_pid_alive(pid) {
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            macos_process_group_is_quiescent(child, pid).map(|quiescent| !quiescent)
        }
        result => result,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn running_child_target_alive(_child: &mut Child, pid: u32) -> io::Result<bool> {
    process_group_or_pid_alive(pid)
}
#[cfg(unix)]
pub(super) fn terminate_pid(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group_or_pid(pid, libc::SIGTERM)
        .with_context(|| format!("failed to send SIGTERM to child process/group {pid}"))
}
#[cfg(unix)]
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    let pid = checked_pid(pid)?;
    signal_group_or_pid(pid, libc::SIGKILL)
        .with_context(|| format!("failed to send SIGKILL to child process/group {pid}"))
}
#[cfg(unix)]
fn checked_pid(pid: u32) -> Result<i32> {
    unix_pid(pid).ok_or_else(|| anyhow!("child PID {pid} exceeds platform process-id range"))
}
#[cfg(unix)]
fn checked_pid_io(pid: u32) -> io::Result<i32> {
    unix_pid(pid).ok_or_else(|| io::Error::other(format!("child PID {pid} exceeds platform range")))
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
fn signal_exited_process_group(child: &mut Child, pid: u32, signal: i32) -> Result<bool> {
    #[cfg(not(target_os = "macos"))]
    let _ = child;
    let process_group = checked_pid(pid)?;
    match send_signal(-process_group, signal) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        #[cfg(target_os = "macos")]
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            // Darwin also returns EPERM when any live group member is not
            // signalable. Accept it only after a fresh exact-PID WNOWAIT
            // observation and one atomic snapshot prove the exited leader is
            // the sole remaining member of this pinned group generation.
            classify_macos_group_signal_eperm(macos_process_group_is_quiescent(child, pid))
                .map_err(Into::into)
        }
        Err(error) => Err(error).with_context(|| {
            format!("failed to send signal {signal} to child process group {process_group}")
        }),
    }
}

#[cfg(unix)]
fn resignal_pinned_exited_process_group(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    resignal_pinned_exited_process_group_with(
        child,
        pid,
        deadline,
        observe_child,
        Instant::now,
        send_signal,
    )
}

#[cfg(unix)]
fn resignal_pinned_exited_process_group_with<T>(
    state: &mut T,
    pid: u32,
    deadline: Instant,
    mut observe: impl FnMut(&mut T) -> io::Result<ChildObservation>,
    mut now: impl FnMut() -> Instant,
    mut send_group_signal: impl FnMut(i32, i32) -> io::Result<()>,
) -> Result<()> {
    let Some(_) = remaining_phase_budget(deadline, now()) else {
        bail!("child process group {pid} SIGKILL confirmation deadline expired before revalidation")
    };
    match observe(state)
        .with_context(|| format!("failed to revalidate child process {pid} before SIGKILL"))?
    {
        ChildObservation::ExitedUnreaped(_) => {}
        ChildObservation::Running => {
            bail!("child process {pid} was still running before exited-group SIGKILL")
        }
        ChildObservation::ExitedReaped(_) => {
            bail!("child process {pid} was reaped before its process group could be re-signaled")
        }
    }
    let Some(_) = remaining_phase_budget(deadline, now()) else {
        bail!("child process group {pid} SIGKILL confirmation deadline expired before re-signaling")
    };

    let process_group = i32::try_from(pid)
        .ok()
        .filter(|process_group| *process_group > 0)
        .ok_or_else(|| anyhow!("child PID {pid} exceeds platform process-id range"))?;
    match send_group_signal(-process_group, libc::SIGKILL) {
        Ok(()) => Ok(()),
        // Neither ESRCH nor Darwin's zombie-only EPERM proves that the pinned
        // group is quiescent. The caller must still perform its platform proof.
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
        #[cfg(target_os = "macos")]
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!("failed to re-send SIGKILL to child process group {process_group}")
        }),
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
fn process_group_or_pid_alive(pid: u32) -> io::Result<bool> {
    let Some(pid) = unix_pid(pid) else {
        return Ok(false);
    };
    match probe_process_liveness(-pid) {
        Ok(true) => Ok(true),
        Ok(false) => probe_process_liveness(pid),
        Err(error) => Err(error),
    }
}
#[cfg(all(unix, test))]
pub(super) fn process_group_alive(pid: u32) -> io::Result<bool> {
    let Some(pid) = unix_pid(pid) else {
        return Ok(false);
    };
    probe_process_liveness(-pid)
}

#[cfg(target_os = "macos")]
fn macos_process_group_is_quiescent(child: &mut Child, pid: u32) -> io::Result<bool> {
    classify_macos_process_group_quiescence(observe_child(child)?, || {
        macos_process_group_contains_only_pinned_leader(pid)
    })
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn classify_macos_process_group_quiescence(
    observation: ChildObservation,
    snapshot: impl FnOnce() -> io::Result<bool>,
) -> io::Result<bool> {
    match observation {
        ChildObservation::Running => Ok(false),
        ChildObservation::ExitedUnreaped(_) => snapshot(),
        ChildObservation::ExitedReaped(_) => Err(io::Error::other(
            "child was reaped before its macOS process group could be confirmed",
        )),
    }
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_group_signal_eperm(quiescence: io::Result<bool>) -> io::Result<bool> {
    // The caller's boolean means that this pinned group still needs bounded
    // cleanup/confirmation, not that Darwin delivered the requested signal.
    // An additional member may be a transient zombie or a live unsignalable
    // process; neither is safe to accept as quiescent at this snapshot.
    quiescence.map(|quiescent| !quiescent)
}

#[cfg(target_os = "macos")]
fn macos_process_group_contains_only_pinned_leader(pid: u32) -> io::Result<bool> {
    let process_group = checked_pid_io(pid)?;
    let mut members = [0 as libc::pid_t; 2];
    let buffer_size = i32::try_from(std::mem::size_of_val(&members))
        .map_err(|_| io::Error::other("macOS process-group snapshot buffer was too large"))?;
    // SAFETY: members is writable storage for exactly two pid_t values and
    // buffer_size describes that complete live buffer. libproc returns a PID
    // count; a full buffer means at least two members because collection is
    // deliberately capped at the supplied capacity.
    let count =
        unsafe { libc::proc_listpgrppids(process_group, members.as_mut_ptr().cast(), buffer_size) };
    if count <= 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!(
                "failed to atomically list macOS process group {process_group} members: {error}"
            ),
        ));
    }
    classify_macos_process_group_snapshot(process_group, count, members)
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_process_group_snapshot(
    process_group: i32,
    count: i32,
    members: [i32; 2],
) -> io::Result<bool> {
    if process_group <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "macOS process-group snapshot used a non-positive pinned leader",
        ));
    }
    let count = usize::try_from(count).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "macOS process-group snapshot returned a negative member count",
        )
    })?;
    if count == 0 || count > members.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("macOS process-group snapshot returned an untrusted member count of {count}"),
        ));
    }
    let observed = &members[..count];
    if observed.iter().any(|pid| *pid <= 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "macOS process-group snapshot returned a non-positive member identifier",
        ));
    }
    if count == members.len() {
        // XNU scans live allproc entries before zombies and caps the result at
        // this buffer. Two positive PIDs therefore means "at least two"; the
        // pinned zombie leader need not appear in the returned pair.
        return Ok(false);
    }
    if observed[0] != process_group {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("macOS process-group snapshot did not contain pinned leader {process_group}"),
        ));
    }
    Ok(true)
}

#[cfg(all(unix, target_os = "linux"))]
fn confirm_exited_process_group_not_live(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    confirm_exited_process_group_not_live_with(
        child,
        pid,
        deadline,
        2,
        resignal_pinned_exited_process_group,
        |_child, pid, deadline| linux_process_group_has_live_members(pid, deadline),
        Instant::now,
        thread::sleep,
    )
}

#[cfg(all(unix, target_os = "macos"))]
fn confirm_exited_process_group_not_live(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    confirm_exited_process_group_not_live_with(
        child,
        pid,
        deadline,
        1,
        resignal_pinned_exited_process_group,
        |child, pid, _deadline| {
            macos_process_group_is_quiescent(child, pid)
                .map(|quiescent| !quiescent)
                .map_err(Into::into)
        },
        Instant::now,
        thread::sleep,
    )
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn confirm_exited_process_group_not_live(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
) -> Result<()> {
    // Other supported Unix targets do not expose a portable post-SIGKILL
    // membership proof. Revalidate the unreaped leader and re-signal once,
    // then retain the documented best-effort behavior on those platforms.
    resignal_pinned_exited_process_group(child, pid, deadline)
}

#[cfg(target_os = "linux")]
fn linux_process_group_has_live_members(process_group: u32, deadline: Instant) -> Result<bool> {
    let mut within_budget = || remaining_phase_budget(deadline, Instant::now()).is_some();
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    let entries = std::fs::read_dir("/proc");
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    let pids = collect_linux_process_ids_with(
        process_group,
        entries.context("failed to enumerate /proc")?,
        |entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
        },
        &mut within_budget,
    )?;
    linux_process_group_has_live_members_with(
        process_group,
        pids,
        |pid| std::fs::read_to_string(format!("/proc/{pid}/stat")),
        linux_process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_process_ids_with<T>(
    process_group: u32,
    mut entries: impl Iterator<Item = io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<u32>,
    mut within_budget: impl FnMut() -> bool,
) -> Result<Vec<u32>> {
    let mut pids = Vec::new();
    loop {
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let entry = entries.next();
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let Some(entry) = entry else {
            return Ok(pids);
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("failed to enumerate /proc entry"),
        };
        if let Some(pid) = process_id(entry) {
            pids.push(pid);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn ensure_linux_group_scan_budget(
    process_group: u32,
    within_budget: &mut impl FnMut() -> bool,
) -> Result<()> {
    if within_budget() {
        Ok(())
    } else {
        bail!("child process group {process_group} cleanup scan exceeded its deadline")
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_process_group_has_live_members_with(
    process_group: u32,
    pids: impl IntoIterator<Item = u32>,
    mut read_stat: impl FnMut(u32) -> io::Result<String>,
    mut process_group_for_pid: impl FnMut(u32) -> io::Result<Option<u32>>,
    mut within_budget: impl FnMut() -> bool,
) -> Result<bool> {
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    for pid in pids {
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let observation = read_stat(pid).and_then(parse_linux_process_stat);
        ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
        let observation = match observation {
            Ok(observation) => observation,
            Err(stat_error) => {
                ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
                let observed_group = process_group_for_pid(pid);
                ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
                match observed_group {
                    Ok(None) => continue,
                    Ok(Some(other_group)) if other_group != process_group => continue,
                    Ok(Some(_)) => {
                        return Err(stat_error).with_context(|| {
                            format!(
                                "could not inspect process {pid}, which belongs to owned process group {process_group}"
                            )
                        });
                    }
                    Err(group_error) => {
                        return Err(stat_error).with_context(|| {
                            format!(
                                "could not inspect process {pid} or prove it is outside owned process group {process_group}: {group_error}"
                            )
                        });
                    }
                }
            }
        };
        if observation.process_group == process_group && observation.live {
            ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
            return Ok(true);
        }
    }
    ensure_linux_group_scan_budget(process_group, &mut within_budget)?;
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxProcessObservation {
    process_group: u32,
    live: bool,
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(stat: String) -> io::Result<LinuxProcessObservation> {
    let (_, fields) = stat
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing stat command field"))?;
    let mut fields = fields.split_whitespace();
    let state = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process state"))?;
    let process_group = fields
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process group"))?
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid process group"))?;
    Ok(LinuxProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
fn linux_process_group_for_pid(pid: u32) -> io::Result<Option<u32>> {
    let pid = checked_pid_io(pid)?;
    // SAFETY: `pid` is a positive, representable process identifier. `getpgid`
    // only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return u32::try_from(process_group)
            .map(Some)
            .map_err(|_| io::Error::other("process group is not representable"));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
}
#[cfg(unix)]
fn probe_process_liveness(pid: i32) -> io::Result<bool> {
    let result = unsafe { libc::kill(pid, 0) };
    classify_liveness_probe(result, io::Error::last_os_error)
}
#[cfg(unix)]
fn classify_liveness_probe(result: i32, error: impl FnOnce() -> io::Error) -> io::Result<bool> {
    if result == 0 {
        return Ok(true);
    }
    let error = error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        // EPERM and unexpected probe failures are not evidence of absence.
        Err(error)
    }
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
    let taskkill = crate::windows_system::native_system_executable("taskkill.exe")
        .context("failed to resolve the native Windows taskkill executable")?;
    let mut command = Command::new(taskkill);
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
pub(super) fn kill_pid(pid: u32) -> Result<()> {
    if !crate::state::pid_is_alive(pid) {
        return Ok(());
    }
    run_taskkill(pid, true)?;
    if wait_for_pid_exit(pid, Duration::from_secs(1)) {
        Ok(())
    } else {
        bail!("child process {pid} remained alive after forced taskkill")
    }
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
mod tests;
