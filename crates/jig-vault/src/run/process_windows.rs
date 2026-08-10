use std::io;
use std::process::Command;
use std::thread;
use std::time::Instant;

use anyhow::{Result as AnyResult, anyhow};

use super::process::{BrokeredProcess, LeaderObservation, terminate_spawn_failure_child};
use super::{BROKERED_PROCESS_CLEANUP_TIMEOUT, BROKERED_PROCESS_POLL_INTERVAL};
#[cfg(windows)]
pub(super) fn spawn_brokered_process(command: &mut Command) -> io::Result<BrokeredProcess> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = create_brokered_process_job()?;
    command.creation_flags(windows_brokered_process_creation_flags());
    let mut child = command.spawn()?;
    // SAFETY: both handles are live and the child remains suspended, so it
    // cannot create an unowned descendant before assignment completes.
    let assigned = unsafe {
        AssignProcessToJobObject(
            job.as_raw_handle() as HANDLE,
            child.as_raw_handle() as HANDLE,
        )
    };
    if assigned == 0 {
        let error = io::Error::last_os_error();
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        if let Some(deadline) = deadline {
            let _ = terminate_windows_job(&job, deadline);
        }
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(error);
    }
    if let Err(error) = resume_suspended_windows_process(child.id()) {
        let deadline = Instant::now().checked_add(BROKERED_PROCESS_CLEANUP_TIMEOUT);
        if let Some(deadline) = deadline {
            let _ = terminate_windows_job(&job, deadline);
        }
        terminate_spawn_failure_child(&mut child, deadline);
        return Err(error);
    }
    Ok(BrokeredProcess {
        child,
        job,
        reaped_status: None,
        cleanup_deadline: None,
        tree_cleanup_error: None,
        cleanup_complete: false,
    })
}

#[cfg(windows)]
pub(super) fn observe_brokered_leader(
    process: &mut BrokeredProcess,
) -> io::Result<LeaderObservation> {
    if process.reaped_status.is_some() {
        return Ok(LeaderObservation::Exited);
    }
    match process.child.try_wait()? {
        Some(status) => {
            // The Job Object remains a stable tree identity after try_wait
            // consumes the leader status on Windows.
            process.reaped_status = Some(status);
            Ok(LeaderObservation::Exited)
        }
        None => Ok(LeaderObservation::Running),
    }
}

#[cfg(windows)]
pub(super) fn terminate_brokered_process_tree(
    process: &mut BrokeredProcess,
    deadline: Instant,
) -> AnyResult<()> {
    let job_result = terminate_windows_job(&process.job, deadline);
    if job_result.is_ok() {
        return Ok(());
    }
    let direct_result = process.child.kill();
    match (job_result, direct_result) {
        (Err(job_error), Err(direct_error)) => Err(anyhow!(
            "Job Object termination failed: {job_error}; direct child termination also failed: {direct_error}"
        )),
        (Err(job_error), Ok(())) => Err(job_error.into()),
        (Ok(()), _) => Ok(()),
    }
}

#[cfg(windows)]
pub(super) const fn windows_brokered_process_creation_flags() -> u32 {
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP
}

#[cfg(windows)]
pub(super) fn create_brokered_process_job() -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    // SAFETY: null attributes and name request a private unnamed Job Object.
    let raw_job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw_job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw_job is a newly created owned handle, transferred once.
    let job = unsafe { OwnedHandle::from_raw_handle(raw_job as RawHandle) };
    let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the pointer and byte length describe the requested live value.
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectExtendedLimitInformation,
            (&raw const information).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

#[cfg(windows)]
pub(super) fn resume_suspended_windows_process(pid: u32) -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // SAFETY: the snapshot call has no borrowed pointer arguments.
    let raw_snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if raw_snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw_snapshot is a newly created owned handle.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw_snapshot as RawHandle) };
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    // SAFETY: entry has the required size and remains writable and live.
    let mut has_entry = unsafe { Thread32First(raw_snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the ID came from the live snapshot; request only resume.
            let raw_thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if raw_thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: raw_thread is a newly opened owned handle.
            let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread as RawHandle) };
            // SAFETY: this thread belongs to the suspended child and the
            // handle carries THREAD_SUSPEND_RESUME.
            let previous_count = unsafe { ResumeThread(raw_thread) };
            drop(thread);
            drop(snapshot);
            if previous_count == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: same live snapshot and initialized entry as above.
        has_entry = unsafe { Thread32Next(raw_snapshot, &mut entry) } != 0;
    }
    Err(io::Error::other(
        "could not find the suspended brokered process thread",
    ))
}

#[cfg(windows)]
pub(super) fn terminate_windows_job(
    job: &std::os::windows::io::OwnedHandle,
    deadline: Instant,
) -> io::Result<()> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, TerminateJobObject,
    };

    // SAFETY: job is a live Job Object owned by the process supervisor.
    if unsafe { TerminateJobObject(job.as_raw_handle() as HANDLE, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    loop {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: output points to a correctly sized accounting structure.
        let queried = unsafe {
            QueryInformationJobObject(
                job.as_raw_handle() as HANDLE,
                JobObjectBasicAccountingInformation,
                (&raw mut information).cast::<c_void>(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        if information.ActiveProcesses == 0 {
            return Ok(());
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "brokered Job Object cleanup timed out",
            ));
        };
        thread::sleep(remaining.min(BROKERED_PROCESS_POLL_INTERVAL));
    }
}
