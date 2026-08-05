use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub(crate) struct TestProcessIdentity {
    pid: libc::pid_t,
    start_token: String,
}

#[derive(Debug)]
struct TestProcessSnapshot {
    start_token: String,
    zombie: bool,
}

impl TestProcessIdentity {
    pub(crate) fn capture(pid: libc::pid_t) -> Option<Self> {
        let snapshot = test_process_snapshot(pid)?;
        (!snapshot.zombie).then_some(Self {
            pid,
            start_token: snapshot.start_token,
        })
    }

    pub(crate) fn capture_current() -> Option<Self> {
        Self::capture(std::process::id().try_into().ok()?)
    }

    pub(crate) fn from_marker(marker: &str) -> Option<Self> {
        let mut fields = marker.split_whitespace();
        let pid = fields.next()?.parse().ok()?;
        let start_token = fields.next()?.to_owned();
        (pid > 0 && !start_token.is_empty() && fields.next().is_none())
            .then_some(Self { pid, start_token })
    }

    pub(crate) fn is_live(&self) -> bool {
        test_process_snapshot(self.pid)
            .is_some_and(|snapshot| snapshot.start_token == self.start_token && !snapshot.zombie)
    }
}

#[cfg(target_os = "linux")]
fn test_process_snapshot(pid: libc::pid_t) -> Option<TestProcessSnapshot> {
    if pid <= 0 {
        return None;
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields = fields.split_whitespace().collect::<Vec<_>>();
    Some(TestProcessSnapshot {
        start_token: format!("linux:{}", fields.get(19)?),
        zombie: matches!(*fields.first()?, "Z" | "X" | "x"),
    })
}

#[cfg(target_os = "macos")]
fn test_process_snapshot(pid: libc::pid_t) -> Option<TestProcessSnapshot> {
    if pid <= 0 {
        return None;
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes = unsafe {
        // SAFETY: info is writable proc_bsdinfo storage and proc_pidinfo writes
        // at most the supplied checked buffer length.
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
    Some(TestProcessSnapshot {
        start_token: format!("macos:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        zombie: info.pbi_status == libc::SZOMB,
    })
}

pub(crate) fn publish_test_process_identity(path: &Path, identity: &TestProcessIdentity) {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(
        &temporary,
        format!("{} {}\n", identity.pid, identity.start_token),
    )
    .unwrap();
    fs::rename(&temporary, path).unwrap();
}

pub(crate) fn read_test_process_identity(path: &Path) -> TestProcessIdentity {
    wait_for_test_file(path);
    TestProcessIdentity::from_marker(&fs::read_to_string(path).unwrap())
        .expect("test-process marker contains an exact PID/start identity")
}

fn wait_for_test_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        path.exists(),
        "test marker {} was not published",
        path.display()
    );
}

pub(crate) fn terminate_and_confirm_test_process(identity: &TestProcessIdentity) {
    if identity.is_live() {
        // SAFETY: the start token was rechecked immediately before signalling,
        // so this PID still identifies the exact process published by the test.
        let result = unsafe { libc::kill(identity.pid, libc::SIGKILL) };
        if result == -1 {
            let error = std::io::Error::last_os_error();
            assert!(
                error.raw_os_error() == Some(libc::ESRCH) && !identity.is_live(),
                "failed to terminate exact test process {}: {error}",
                identity.pid,
            );
        }
    }
    wait_until_stopped(identity);
    assert!(
        !identity.is_live(),
        "exact test process {} remained live after SIGKILL",
        identity.pid,
    );
}

pub(crate) fn assert_test_process_stopped(identity: &TestProcessIdentity) {
    wait_until_stopped(identity);
    if identity.is_live() {
        terminate_and_confirm_test_process(identity);
        panic!(
            "owned descendant {} remained live after cleanup",
            identity.pid
        );
    }
}

fn wait_until_stopped(identity: &TestProcessIdentity) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while identity.is_live() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn marker_parser_rejects_pid_only_and_extra_fields() {
    assert!(TestProcessIdentity::from_marker("73").is_none());
    assert!(TestProcessIdentity::from_marker("73 linux:9 extra").is_none());
    assert!(TestProcessIdentity::from_marker("73 linux:9").is_some());
}
