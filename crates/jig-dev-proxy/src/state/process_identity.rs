#[cfg(target_os = "linux")]
use std::fs;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::types::{Route, RouteMode};

pub(crate) fn route_is_alive(route: &Route) -> bool {
    match route.mode {
        RouteMode::Alias => true,
        RouteMode::Process => route
            .owner_pid
            .is_some_and(|pid| pid_matches_owner(pid, route.owner_start_token.as_deref())),
    }
}

fn pid_matches_owner(pid: u32, expected_start_token: Option<&str>) -> bool {
    if !pid_is_alive(pid) {
        return false;
    }
    let Some(expected_start_token) = expected_start_token else {
        return false;
    };
    if !process_start_tokens_supported() {
        return false;
    }
    process_start_token(pid)
        .as_deref()
        .is_some_and(|current_start_token| current_start_token == expected_start_token)
}

pub(crate) const fn process_start_tokens_supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

pub(crate) fn pid_is_alive(pid: u32) -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some(pid) = i32::try_from(pid).ok() else {
            return false;
        };
        #[cfg(target_os = "linux")]
        if linux_process_is_zombie(pid as u32) {
            return false;
        }
        unsafe {
            // SAFETY: pid was range-checked above. Signal 0 performs permission
            // and existence checks without delivering a signal.
            libc::kill(pid, 0) == 0
        }
    }
    #[cfg(target_os = "macos")]
    {
        macos_proc_bsdinfo(pid).is_some_and(|info| info.pbi_status != libc::SZOMB)
    }
    #[cfg(windows)]
    {
        unsafe {
            // SAFETY: OpenProcess does not take ownership of any Rust memory. The
            // returned handle is closed on every non-null path below.
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut exit_code = 0u32;
            // SAFETY: `handle` is a live process handle from OpenProcess and
            // `exit_code` is a valid out pointer for the duration of the call.
            let ok = GetExitCodeProcess(handle, &mut exit_code);
            // SAFETY: `handle` was returned by OpenProcess and has not been closed.
            let _ = CloseHandle(handle);
            ok != 0 && exit_code == STILL_ACTIVE as u32
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(target_os = "linux")]
fn linux_process_is_zombie(pid: u32) -> bool {
    // Hardened procfs mounts can hide status from non-owners. Treat an
    // unreadable status as non-zombie; route liveness still requires the
    // process start token to match before a process route is trusted.
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("State:"))
                .map(str::trim_start)
                .and_then(|state| state.chars().next())
        })
        == Some('Z')
}

#[cfg(test)]
pub(super) fn windows_tasklist_csv_pid(line: &str) -> Option<u32> {
    csv_fields(line).get(1)?.parse().ok()
}

#[cfg(test)]
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                let _ = chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start_token(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let start_time_ticks = fields.split_whitespace().nth(19)?;
    Some(format!("linux:{start_time_ticks}"))
}

#[cfg(target_os = "macos")]
pub(crate) fn process_start_token(pid: u32) -> Option<String> {
    let info = macos_proc_bsdinfo(pid)?;
    if info.pbi_status == libc::SZOMB {
        return None;
    }
    Some(format!(
        "macos:{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "macos")]
fn macos_proc_bsdinfo(pid: u32) -> Option<libc::proc_bsdinfo> {
    let pid = i32::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes = unsafe {
        // SAFETY: info points to a writable proc_bsdinfo-sized buffer, pid was
        // range-checked above, and PROC_PIDTBSDINFO writes at most the supplied
        // buffer size. The byte count is checked before assume_init.
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size.try_into().ok()?,
        )
    };
    if bytes < size.try_into().ok()? {
        return None;
    }
    Some(unsafe {
        // SAFETY: proc_pidinfo reported that it initialized the full
        // proc_bsdinfo-sized buffer above.
        info.assume_init()
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos",)))]
pub(crate) fn process_start_token(_pid: u32) -> Option<String> {
    None
}
