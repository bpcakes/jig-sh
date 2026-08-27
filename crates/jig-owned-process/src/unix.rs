use std::fmt;
use std::num::NonZeroI32;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(unix)]
use std::process::ExitStatus;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProcessGroupId(NonZeroI32);

impl ProcessGroupId {
    pub fn new(raw: i32) -> std::io::Result<Self> {
        NonZeroI32::new(raw)
            .filter(|raw| raw.get() > 0)
            .map(Self)
            .ok_or_else(|| std::io::Error::other("process-group identity must be positive"))
    }

    pub const fn as_raw(self) -> i32 {
        self.0.get()
    }
}

impl TryFrom<u32> for ProcessGroupId {
    type Error = std::io::Error;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        let raw = i32::try_from(raw)
            .map_err(|_| std::io::Error::other("process identifier is not representable"))?;
        Self::new(raw)
    }
}

#[cfg(unix)]
#[derive(Debug)]
pub struct WaitidStatus {
    observed_pid: i32,
    code: i32,
    status: i32,
}

#[cfg(unix)]
impl WaitidStatus {
    #[cfg(test)]
    fn new(observed_pid: i32, code: i32, status: i32) -> Self {
        Self {
            observed_pid,
            code,
            status,
        }
    }
}

#[cfg(unix)]
pub fn waitid_without_reaping(process_group: ProcessGroupId) -> std::io::Result<WaitidStatus> {
    let mut information = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: `information` is writable siginfo_t storage, the validated
    // identifier must name a direct child pinned by the caller, and WNOWAIT
    // preserves that child's wait status for subsequent process-group cleanup.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            process_group.as_raw() as _,
            information.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error());
    }

    // SAFETY: successful waitid initialized the siginfo value and its SIGCHLD
    // union member.
    let information = unsafe { information.assume_init() };
    Ok(WaitidStatus {
        // SAFETY: waitid populated the SIGCHLD fields read below.
        observed_pid: unsafe { information.si_pid() },
        code: information.si_code,
        // SAFETY: waitid populated the SIGCHLD fields read below.
        status: unsafe { information.si_status() },
    })
}

#[cfg(unix)]
#[derive(Debug)]
pub enum WaitidClassificationError {
    UnexpectedPid { expected: i32, observed: i32 },
    UnexpectedCode(i32),
}

#[cfg(unix)]
impl fmt::Display for WaitidClassificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedPid { expected, observed } => write!(
                formatter,
                "waitid observed unexpected child PID {observed} instead of {expected}"
            ),
            Self::UnexpectedCode(code) => {
                write!(
                    formatter,
                    "waitid returned an unexpected child state code {code}"
                )
            }
        }
    }
}

#[cfg(unix)]
impl std::error::Error for WaitidClassificationError {}

#[cfg(unix)]
#[derive(Debug, Eq, PartialEq)]
pub enum UnreapedChildObservation {
    Running,
    Exited(ExitStatus),
}

#[cfg(unix)]
pub fn classify_waitid_status(
    process_group: ProcessGroupId,
    status: WaitidStatus,
) -> Result<UnreapedChildObservation, WaitidClassificationError> {
    if status.observed_pid == 0 {
        return Ok(UnreapedChildObservation::Running);
    }
    if status.observed_pid != process_group.as_raw() {
        return Err(WaitidClassificationError::UnexpectedPid {
            expected: process_group.as_raw(),
            observed: status.observed_pid,
        });
    }
    let raw_status = match status.code {
        libc::CLD_EXITED => status.status << 8,
        libc::CLD_KILLED => status.status,
        libc::CLD_DUMPED => status.status | 0x80,
        libc::CLD_STOPPED | libc::CLD_TRAPPED | libc::CLD_CONTINUED => {
            return Ok(UnreapedChildObservation::Running);
        }
        code => return Err(WaitidClassificationError::UnexpectedCode(code)),
    };
    Ok(UnreapedChildObservation::Exited(ExitStatus::from_raw(
        raw_status,
    )))
}

#[cfg(target_os = "linux")]
pub fn linux_process_group_has_live_members(
    process_group: ProcessGroupId,
    deadline: Instant,
) -> std::io::Result<bool> {
    let mut within_budget = || {
        deadline
            .checked_duration_since(Instant::now())
            .is_some_and(|remaining| !remaining.is_zero())
    };
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    let entries = std::fs::read_dir("/proc").map_err(|error| {
        linux_process_scan_error(
            error,
            format!(
                "failed to enumerate /proc while scanning Linux process group {}",
                process_group.as_raw()
            ),
        )
    })?;
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    let pids = collect_linux_process_ids_with(
        process_group,
        entries,
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
        // The parenthesized command name in /proc/<pid>/stat may contain
        // arbitrary bytes even though the process-state fields are ASCII.
        |pid| std::fs::read(format!("/proc/{pid}/stat")),
        linux_process_group_for_pid,
        &mut within_budget,
    )
}

#[cfg(any(target_os = "linux", test))]
fn collect_linux_process_ids_with<T>(
    process_group: ProcessGroupId,
    mut entries: impl Iterator<Item = std::io::Result<T>>,
    mut process_id: impl FnMut(T) -> Option<i32>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<Vec<i32>> {
    let mut pids = Vec::new();
    loop {
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let entry = entries.next();
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let Some(entry) = entry else {
            return Ok(pids);
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(linux_process_scan_error(
                    error,
                    format!(
                        "failed to enumerate /proc entry while scanning Linux process group {}",
                        process_group.as_raw()
                    ),
                ));
            }
        };
        if let Some(pid) = process_id(entry) {
            pids.push(pid);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn ensure_linux_process_scan_budget(
    process_group: ProcessGroupId,
    within_budget: &mut impl FnMut() -> bool,
) -> std::io::Result<()> {
    if within_budget() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "Linux process group {} cleanup scan exceeded its deadline",
                process_group.as_raw()
            ),
        ))
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_process_group_has_live_members_with(
    process_group: ProcessGroupId,
    pids: impl IntoIterator<Item = i32>,
    mut read_stat: impl FnMut(i32) -> std::io::Result<Vec<u8>>,
    mut process_group_for_pid: impl FnMut(i32) -> std::io::Result<Option<i32>>,
    mut within_budget: impl FnMut() -> bool,
) -> std::io::Result<bool> {
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    for pid in pids {
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let observation = read_stat(pid).and_then(|stat| parse_linux_process_stat(pid, &stat));
        ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
        let observation = match observation {
            Ok(observation) => observation,
            Err(stat_error) => {
                ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
                let observed_group = process_group_for_pid(pid);
                ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
                match observed_group {
                    Ok(None) => continue,
                    Ok(Some(other_group)) if other_group != process_group.as_raw() => continue,
                    Ok(Some(_)) => {
                        return Err(linux_process_scan_error(
                            stat_error,
                            format!(
                                "could not inspect process {pid}, which belongs to Linux process group {}",
                                process_group.as_raw()
                            ),
                        ));
                    }
                    Err(group_error) => {
                        return Err(linux_process_scan_error(
                            stat_error,
                            format!(
                                "could not inspect process {pid} or prove it is outside Linux process group {}: {group_error}",
                                process_group.as_raw()
                            ),
                        ));
                    }
                }
            }
        };
        if observation.process_group == process_group.as_raw() && observation.live {
            ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
            return Ok(true);
        }
    }
    ensure_linux_process_scan_budget(process_group, &mut within_budget)?;
    Ok(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinuxProcessObservation {
    process_group: i32,
    live: bool,
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(
    expected_pid: i32,
    stat: &[u8],
) -> std::io::Result<LinuxProcessObservation> {
    let expected_prefix = format!("{expected_pid} (");
    if expected_pid <= 0 || !stat.starts_with(expected_prefix.as_bytes()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Linux process stat did not begin with the expected process identifier",
        ));
    }
    let command_end = stat
        .windows(2)
        .rposition(|window| window == b") ")
        .filter(|command_end| *command_end >= expected_prefix.len())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing Linux process stat command field",
            )
        })?;
    let fields = std::str::from_utf8(&stat[command_end + 2..]).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Linux process stat fields are not valid UTF-8",
        )
    })?;
    let mut fields = fields.split_whitespace();
    let state = fields.next().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing process state")
    })?;
    let process_group = fields
        .nth(1)
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing process group")
        })?
        .parse::<i32>()
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid process group")
        })?;
    Ok(LinuxProcessObservation {
        process_group,
        live: !matches!(state, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "linux")]
fn linux_process_group_for_pid(pid: libc::pid_t) -> std::io::Result<Option<libc::pid_t>> {
    // SAFETY: `pid` is a positive identifier enumerated from /proc and this
    // call only observes its current process-group membership.
    let process_group = unsafe { libc::getpgid(pid) };
    if process_group >= 0 {
        return Ok(Some(process_group));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(None)
    } else {
        Err(error)
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_process_scan_error(error: std::io::Error, message: String) -> std::io::Error {
    let kind = error.kind();
    std::io::Error::new(kind, LinuxProcessScanContext { message, error })
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
struct LinuxProcessScanContext {
    message: String,
    error: std::io::Error,
}

#[cfg(any(target_os = "linux", test))]
impl fmt::Display for LinuxProcessScanContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.message, self.error)
    }
}

#[cfg(any(target_os = "linux", test))]
impl std::error::Error for LinuxProcessScanContext {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

#[derive(Debug)]
pub enum MacosProcessGroupSnapshotError {
    BufferSize,
    List(std::io::Error),
    NegativeMemberCount,
    UntrustedMemberCount(usize),
    NonPositiveMember,
    MissingPinnedLeader(ProcessGroupId),
}

impl fmt::Display for MacosProcessGroupSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferSize => {
                formatter.write_str("macOS process-group snapshot buffer was not representable")
            }
            Self::List(error) => write!(formatter, "macOS process-group snapshot failed: {error}"),
            Self::NegativeMemberCount => {
                formatter.write_str("macOS process-group snapshot returned a negative member count")
            }
            Self::UntrustedMemberCount(count) => write!(
                formatter,
                "macOS process-group snapshot returned an untrusted member count of {count}"
            ),
            Self::NonPositiveMember => formatter.write_str(
                "macOS process-group snapshot returned a non-positive member identifier",
            ),
            Self::MissingPinnedLeader(process_group) => write!(
                formatter,
                "macOS process-group snapshot did not contain pinned leader {}",
                process_group.as_raw()
            ),
        }
    }
}

impl std::error::Error for MacosProcessGroupSnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::List(error) => Some(error),
            _ => None,
        }
    }
}

pub fn classify_macos_process_group_snapshot(
    process_group: ProcessGroupId,
    count: i32,
    members: [i32; 2],
) -> Result<bool, MacosProcessGroupSnapshotError> {
    let count =
        usize::try_from(count).map_err(|_| MacosProcessGroupSnapshotError::NegativeMemberCount)?;
    if count == 0 || count > members.len() {
        return Err(MacosProcessGroupSnapshotError::UntrustedMemberCount(count));
    }
    let observed = &members[..count];
    if observed.iter().any(|pid| *pid <= 0) {
        return Err(MacosProcessGroupSnapshotError::NonPositiveMember);
    }
    if count == members.len() {
        // XNU scans live allproc entries before zombies and caps the result at
        // this buffer. Two positive PIDs therefore means "at least two"; the
        // pinned zombie leader need not appear in the returned pair.
        return Ok(false);
    }
    if observed[0] != process_group.as_raw() {
        return Err(MacosProcessGroupSnapshotError::MissingPinnedLeader(
            process_group,
        ));
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
pub fn macos_process_group_contains_only_pinned_leader(
    process_group: ProcessGroupId,
) -> Result<bool, MacosProcessGroupSnapshotError> {
    let mut members = [0 as libc::pid_t; 2];
    let buffer_size = i32::try_from(std::mem::size_of_val(&members))
        .map_err(|_| MacosProcessGroupSnapshotError::BufferSize)?;
    // SAFETY: members is writable storage for exactly two pid_t values and the
    // byte count describes that complete buffer. Collection is deliberately
    // capped at two because a full buffer already disproves sole membership.
    let count = unsafe {
        libc::proc_listpgrppids(
            process_group.as_raw(),
            members.as_mut_ptr().cast(),
            buffer_size,
        )
    };
    if count <= 0 {
        return Err(MacosProcessGroupSnapshotError::List(
            std::io::Error::last_os_error(),
        ));
    }
    classify_macos_process_group_snapshot(process_group, count, members)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsecutiveQuiescence {
    required: u8,
    observed: u8,
}

impl ConsecutiveQuiescence {
    pub fn new(required: u8) -> std::io::Result<Self> {
        if required == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "process-group confirmation requires at least one proof",
            ));
        }
        Ok(Self {
            required,
            observed: 0,
        })
    }

    pub fn observe(&mut self, quiescent: bool) -> bool {
        if quiescent {
            self.observed = self.observed.saturating_add(1).min(self.required);
        } else {
            self.observed = 0;
        }
        self.observed == self.required
    }

    pub const fn observed(self) -> u8 {
        self.observed
    }
}

#[cfg(test)]
#[path = "unix/linux_tests.rs"]
mod linux_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn process_group_id_accepts_only_positive_representable_values() {
        assert_eq!(ProcessGroupId::new(73).unwrap().as_raw(), 73);
        assert!(ProcessGroupId::new(0).is_err());
        assert!(ProcessGroupId::new(-1).is_err());
        assert!(ProcessGroupId::try_from(u32::MAX).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn waitid_classification_accepts_only_the_exact_terminal_child() {
        let process_group = ProcessGroupId::new(73).unwrap();
        for (code, status, raw) in [
            (libc::CLD_EXITED, 7, 7 << 8),
            (libc::CLD_KILLED, libc::SIGTERM, libc::SIGTERM),
            (libc::CLD_DUMPED, libc::SIGSEGV, libc::SIGSEGV | 0x80),
        ] {
            assert_eq!(
                classify_waitid_status(process_group, WaitidStatus::new(73, code, status)).unwrap(),
                UnreapedChildObservation::Exited(ExitStatus::from_raw(raw))
            );
        }
        for code in [libc::CLD_STOPPED, libc::CLD_TRAPPED, libc::CLD_CONTINUED] {
            assert_eq!(
                classify_waitid_status(process_group, WaitidStatus::new(73, code, libc::SIGSTOP))
                    .unwrap(),
                UnreapedChildObservation::Running
            );
        }
        assert_eq!(
            classify_waitid_status(process_group, WaitidStatus::new(0, i32::MAX, i32::MAX))
                .unwrap(),
            UnreapedChildObservation::Running
        );
        assert!(
            classify_waitid_status(process_group, WaitidStatus::new(74, libc::CLD_EXITED, 0))
                .is_err()
        );
        assert!(classify_waitid_status(process_group, WaitidStatus::new(73, i32::MAX, 0)).is_err());
    }

    #[test]
    fn macos_snapshot_requires_the_exact_sole_pinned_leader() {
        let process_group = ProcessGroupId::new(73).unwrap();
        assert!(classify_macos_process_group_snapshot(process_group, 1, [73, 0]).unwrap());
        assert!(!classify_macos_process_group_snapshot(process_group, 2, [73, 74]).unwrap());
        assert!(!classify_macos_process_group_snapshot(process_group, 2, [74, 73]).unwrap());
        assert!(!classify_macos_process_group_snapshot(process_group, 2, [74, 75]).unwrap());
        assert!(!classify_macos_process_group_snapshot(process_group, 2, [73, 73]).unwrap());
        assert!(classify_macos_process_group_snapshot(process_group, 1, [74, 0]).is_err());
        assert!(classify_macos_process_group_snapshot(process_group, 0, [0, 0]).is_err());
        assert!(classify_macos_process_group_snapshot(process_group, -1, [0, 0]).is_err());
        assert!(classify_macos_process_group_snapshot(process_group, 3, [73, 74]).is_err());
        assert!(classify_macos_process_group_snapshot(process_group, 2, [73, 0]).is_err());
    }

    #[test]
    fn consecutive_quiescence_resets_and_cannot_require_zero_proofs() {
        assert!(ConsecutiveQuiescence::new(0).is_err());
        let mut proof = ConsecutiveQuiescence::new(2).unwrap();
        assert!(!proof.observe(true));
        assert_eq!(proof.observed(), 1);
        assert!(!proof.observe(false));
        assert_eq!(proof.observed(), 0);
        assert!(!proof.observe(true));
        assert!(proof.observe(true));
        assert!(proof.observe(true));
        assert_eq!(proof.observed(), 2);
    }
}
