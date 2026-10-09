use super::*;

#[derive(Clone, Debug)]
pub(super) struct VerifiedProcessIdentity {
    pub(super) pid: libc::pid_t,
    pub(super) start_token: String,
}

#[derive(Debug)]
pub(super) struct ProcessSnapshot {
    start_token: String,
    zombie: bool,
}

impl VerifiedProcessIdentity {
    pub(super) fn capture(pid: libc::pid_t) -> Option<Self> {
        let snapshot = process_snapshot(pid)?;
        if snapshot.zombie {
            return None;
        }
        Some(Self {
            pid,
            start_token: snapshot.start_token,
        })
    }

    fn from_marker(pid: &str, start_token: &str) -> Option<Self> {
        let pid = pid.parse::<libc::pid_t>().ok()?;
        if pid <= 0 || start_token.is_empty() {
            return None;
        }
        Some(Self {
            pid,
            start_token: start_token.to_owned(),
        })
    }

    fn matching_snapshot(&self) -> Option<ProcessSnapshot> {
        let snapshot = process_snapshot(self.pid)?;
        (snapshot.start_token == self.start_token).then_some(snapshot)
    }

    pub(super) fn is_live(&self) -> bool {
        self.matching_snapshot()
            .is_some_and(|snapshot| !snapshot.zombie)
    }

    fn owns_process_group(&self) -> bool {
        if self.matching_snapshot().is_none() {
            return false;
        }
        unsafe {
            // SAFETY: pid is positive and identity-verified immediately before
            // this non-mutating process-group query.
            libc::getpgid(self.pid) == self.pid
        }
    }
}

#[test]
fn marker_derived_cleanup_identity_fails_closed_on_token_mismatch() {
    let mut identity = VerifiedProcessIdentity::capture(
        std::process::id()
            .try_into()
            .expect("test process pid fits pid_t"),
    )
    .expect("capture test process identity");
    assert!(identity.is_live());

    identity.start_token.push_str(":mismatch");

    assert!(!identity.is_live());
    assert!(!identity.owns_process_group());
}

impl std::fmt::Display for VerifiedProcessIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.pid.fmt(formatter)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn process_snapshot(pid: libc::pid_t) -> Option<ProcessSnapshot> {
    if pid <= 0 {
        return None;
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields = fields.split_whitespace().collect::<Vec<_>>();
    Some(ProcessSnapshot {
        start_token: format!("linux:{}", fields.get(19)?),
        zombie: matches!(*fields.first()?, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "macos")]
pub(super) fn process_snapshot(pid: libc::pid_t) -> Option<ProcessSnapshot> {
    if pid <= 0 {
        return None;
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes = unsafe {
        // SAFETY: info is writable proc_bsdinfo storage and proc_pidinfo writes
        // at most the supplied, checked buffer length.
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
    let info = unsafe {
        // SAFETY: proc_pidinfo reported a complete proc_bsdinfo result.
        info.assume_init()
    };
    Some(ProcessSnapshot {
        start_token: format!("macos:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        zombie: info.pbi_status == libc::SZOMB,
    })
}

pub(super) fn publish_started_marker(path: &Path, identities: &[VerifiedProcessIdentity]) {
    let marker = identities
        .iter()
        .map(|identity| format!("{} {}", identity.pid, identity.start_token))
        .collect::<Vec<_>>()
        .join(" ");
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, format!("{marker}\n")).expect("write temporary started marker");
    fs::rename(&temporary, path).expect("atomically publish started marker");
}

pub(super) fn parse_started_marker(marker: &str) -> Option<Vec<VerifiedProcessIdentity>> {
    let mut fields = marker.split_whitespace();
    let mut identities = Vec::new();
    while let Some(pid) = fields.next() {
        let start_token = fields.next()?;
        identities.push(VerifiedProcessIdentity::from_marker(pid, start_token)?);
    }
    (!identities.is_empty()).then_some(identities)
}

pub(super) struct ForegroundChildGuard {
    child: Child,
    process_group: libc::pid_t,
    started_path: PathBuf,
    armed: bool,
}

impl ForegroundChildGuard {
    pub(super) fn new(child: Child, started_path: PathBuf) -> Self {
        Self {
            process_group: child.id().try_into().expect("child pid fits pid_t"),
            child,
            started_path,
            armed: true,
        }
    }

    pub(super) const fn disarm(&mut self) {
        self.armed = false;
    }

    fn terminate_detached_helper_group(&self) {
        let Ok(marker) = fs::read_to_string(&self.started_path) else {
            return;
        };
        let Some(identities) = parse_started_marker(&marker) else {
            return;
        };
        if let Some(group_leader) = identities.first() {
            terminate_verified_group(group_leader);
        }
        for descendant in identities.iter().skip(1) {
            terminate_verified_process(descendant);
        }
    }
}

impl std::ops::Deref for ForegroundChildGuard {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ForegroundChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for ForegroundChildGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let running = self.child.try_wait().is_ok_and(|status| status.is_none());
        if running {
            // The unreaped Child value pins this direct PID while these signals
            // are sent; unlike marker-derived PIDs, it cannot have been reused.
            let _ = unsafe { libc::kill(self.process_group, libc::SIGINT) };
            let stopped = self
                .child
                .wait_timeout(Duration::from_secs(5))
                .ok()
                .flatten()
                .is_some();
            if !stopped {
                self.terminate_detached_helper_group();
                let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        self.terminate_detached_helper_group();
    }
}

pub(super) fn read_helper_marker(path: &Path) -> (VerifiedProcessIdentity, u16, VerifiedProcessIdentity) {
    let marker = fs::read_to_string(path).expect("read helper marker");
    let mut fields = marker.split_whitespace();
    let helper = VerifiedProcessIdentity::from_marker(
        fields.next().expect("helper marker pid"),
        fields.next().expect("helper marker start token"),
    )
    .expect("helper marker has a valid identity");
    let port = fields
        .next()
        .expect("helper marker port")
        .parse()
        .expect("helper marker port is numeric");
    let descendant = VerifiedProcessIdentity::from_marker(
        fields.next().expect("helper marker descendant pid"),
        fields.next().expect("helper marker descendant start token"),
    )
    .expect("helper descendant marker has a valid identity");
    assert!(fields.next().is_none(), "helper marker has extra fields");
    assert!(
        wait_for_verified_liveness(&helper, Duration::from_secs(5)),
        "helper marker identity does not become live"
    );
    assert!(
        wait_for_verified_liveness(&descendant, Duration::from_secs(5)),
        "helper descendant marker identity does not become live"
    );
    (helper, port, descendant)
}

pub(super) fn wait_for_verified_liveness(identity: &VerifiedProcessIdentity, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if identity.is_live() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    identity.is_live()
}

pub(super) fn wait_for_verified_exit(identity: &VerifiedProcessIdentity, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !identity.is_live() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    !identity.is_live()
}

pub(super) fn assert_verified_process_tree_exited(
    processes: &[(&str, &VerifiedProcessIdentity)],
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        if processes.iter().all(|(_, identity)| !identity.is_live()) {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            let live = processes
                .iter()
                .filter(|(_, identity)| identity.is_live())
                .map(|(label, identity)| format!("{label} {}", identity.pid))
                .collect::<Vec<_>>()
                .join(", ");
            if live.is_empty() {
                return;
            }
            panic!("verified process tree remained live after {timeout:?}: {live}");
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

pub(super) fn terminate_verified_process(identity: &VerifiedProcessIdentity) {
    if identity.is_live() {
        let _ = unsafe {
            // SAFETY: the positive pid and start token were reverified above.
            libc::kill(identity.pid, libc::SIGTERM)
        };
    }
    if wait_for_verified_exit(identity, Duration::from_millis(250)) {
        return;
    }
    if identity.is_live() {
        let _ = unsafe {
            // SAFETY: the positive pid and start token were reverified above.
            libc::kill(identity.pid, libc::SIGKILL)
        };
    }
    let _ = wait_for_verified_exit(identity, Duration::from_secs(2));
}

pub(super) fn terminate_verified_group(identity: &VerifiedProcessIdentity) {
    if !identity.owns_process_group() {
        return;
    }
    let _ = unsafe {
        // SAFETY: getpgid and the process start token verified that this
        // negative pid still names the helper-owned process group.
        libc::kill(-identity.pid, libc::SIGTERM)
    };
    thread::sleep(Duration::from_millis(250));
    if identity.owns_process_group() {
        let _ = unsafe {
            // SAFETY: the group leader's pinned identity was reverified above.
            libc::kill(-identity.pid, libc::SIGKILL)
        };
    }
    let _ = wait_for_verified_exit(identity, Duration::from_secs(2));
}
