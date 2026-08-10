use std::io;
#[cfg(target_os = "linux")]
use std::time::Instant;

use anyhow::{Context, Result as AnyResult, bail};

#[cfg(target_os = "linux")]
pub(super) fn linux_process_group_has_live_members(
    process_group: libc::pid_t,
    deadline: Instant,
) -> AnyResult<bool> {
    let mut within_budget = || {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| !remaining.is_zero())
    };
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
                .and_then(|name| name.parse::<libc::pid_t>().ok())
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
pub(super) fn collect_linux_process_ids_with<T>(
    process_group: i32,
    mut entries: impl Iterator<Item = io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<i32>,
    mut within_budget: impl FnMut() -> bool,
) -> AnyResult<Vec<i32>> {
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
pub(super) fn ensure_linux_group_scan_budget(
    process_group: i32,
    within_budget: &mut impl FnMut() -> bool,
) -> AnyResult<()> {
    if within_budget() {
        Ok(())
    } else {
        bail!("brokered process group {process_group} cleanup scan exceeded its deadline")
    }
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn linux_process_group_has_live_members_with(
    process_group: i32,
    pids: impl IntoIterator<Item = i32>,
    mut read_stat: impl FnMut(i32) -> io::Result<String>,
    mut process_group_for_pid: impl FnMut(i32) -> io::Result<Option<i32>>,
    mut within_budget: impl FnMut() -> bool,
) -> AnyResult<bool> {
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
pub(super) struct LinuxProcessObservation {
    pub(super) process_group: i32,
    pub(super) live: bool,
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_linux_process_stat(stat: String) -> io::Result<LinuxProcessObservation> {
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
        .parse::<i32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid process group"))?;
    Ok(LinuxProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
pub(super) fn linux_process_group_for_pid(pid: libc::pid_t) -> io::Result<Option<libc::pid_t>> {
    // SAFETY: pid is a positive process identifier read from /proc. getpgid
    // only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return Ok(Some(process_group));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
}
