use super::{Fixture, executable};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt;
use serde_json::Value;
use wait_timeout::ChildExt;

struct RuntimeRun {
    fixture: Fixture,
    child: Child,
}

impl RuntimeRun {
    fn start(stalled: bool, wrapper: &str, probe_timeout: bool) -> Self {
        let fixture = Fixture::with_preflight("exec python3 scripts/waiting-check");
        executable(&fixture.root.path().join("scripts/check-local"), wrapper);
        executable(
            &fixture.root.path().join("scripts/jig"),
            include_str!("runtime_owner.py"),
        );
        if stalled {
            fs::write(fixture.root.path().join(".agent/stall"), "").unwrap();
        }
        fs::write(
            fixture.root.path().join("scripts/waiting-check"),
            r#"import fcntl, pathlib, time
root = pathlib.Path('.agent')
with (root / 'action.lock').open('w') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    (root / 'launches').write_text('preflight\n')
    (root / 'heartbeat').write_text(str(time.monotonic_ns()))
    (root / 'ready').touch()
    deadline = time.monotonic() + 30
    while not (root / 'release').exists() and time.monotonic() < deadline:
        (root / 'heartbeat').write_text(str(time.monotonic_ns()))
        time.sleep(0.02)
"#,
        )
        .unwrap();
        let mut command = fixture.local();
        if probe_timeout {
            let bin = fixture.root.path().join("probe-bin");
            fs::create_dir(&bin).unwrap();
            executable(
                &bin.join("ps"),
                "#!/usr/bin/env python3\nimport time\ntime.sleep(10)\n",
            );
            let paths = std::iter::once(bin)
                .chain(std::env::split_paths(
                    &std::env::var_os("PATH").unwrap_or_default(),
                ))
                .collect::<Vec<_>>();
            command.env("PATH", std::env::join_paths(paths).unwrap());
        }
        let child = command
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(fs::File::create(fixture.root.path().join(".agent/stderr")).unwrap())
            .spawn()
            .unwrap();
        let mut run = Self { fixture, child };
        let ready = if stalled { "suspended" } else { "ready" };
        let deadline = Instant::now() + Duration::from_secs(15);
        while !run.fixture.root.path().join(".agent").join(ready).exists() {
            assert!(Instant::now() < deadline, "runtime action did not start");
            assert!(run.child.try_wait().unwrap().is_none());
            thread::sleep(Duration::from_millis(20));
        }
        run
    }

    fn signal(&self, signal: i32, foreground_group: bool) {
        let pid = self.child.id() as libc::pid_t;
        let target = if foreground_group { -pid } else { pid };
        // SAFETY: the unreaped owned wrapper pins this PID/process group.
        assert_eq!(unsafe { libc::kill(target, signal) }, 0);
    }

    fn released(&self, name: &str) -> bool {
        fs::File::open(self.fixture.root.path().join(".agent").join(name))
            .is_ok_and(|lock| FileExt::try_lock_exclusive(&lock).unwrap_or(false))
    }

    fn cleanup(&mut self) -> bool {
        let _ = fs::write(self.fixture.root.path().join(".agent/release"), "");
        // The fixture parent resumes/terminates only its own unreaped runtime.
        // This runs even when the wrapper has exited or an assertion panics.
        if self
            .child
            .wait_timeout(Duration::from_secs(5))
            .ok()
            .flatten()
            .is_none()
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.released("action.lock") && self.released("runtime-owner.lock") {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for RuntimeRun {
    fn drop(&mut self) {
        self.cleanup();
    }
}

const WRAPPER: &str = include_str!("../../../../scripts/check-local");

#[test]
fn sighup_cleans_real_runtime_action_before_wrapper_returns() {
    for foreground_group in [false, true] {
        exercise_runtime_cancellation(foreground_group, false, false);
    }
}

#[test]
fn forced_runtime_termination_reports_unconfirmed_descendant_cleanup() {
    exercise_runtime_cancellation(false, true, false);
}

#[test]
fn probe_timeout_reports_unconfirmed_descendant_cleanup() {
    exercise_runtime_cancellation(false, true, true);
}

fn exercise_runtime_cancellation(foreground_group: bool, stalled: bool, probe_timeout: bool) {
    let mut run = RuntimeRun::start(stalled, WRAPPER, probe_timeout);
    run.signal(
        if stalled { libc::SIGTERM } else { libc::SIGHUP },
        foreground_group,
    );
    let status = run.child.wait_timeout(Duration::from_secs(15)).unwrap();
    // Snapshot before releasing the fixture, including for a broken wrapper
    // that exits while runtime or action work still lives.
    let lock_released = run.released("action.lock");
    let timeout = if stalled {
        Duration::from_secs(5)
    } else {
        Duration::from_millis(100)
    };
    let stopped = !heartbeat_changes(&run.fixture, timeout);
    let launches = run.fixture.launches();
    let events = fs::read_to_string(run.fixture.root.path().join(".agent/state/runs.jsonl"))
        .unwrap_or_default();
    let stderr = fs::read_to_string(run.fixture.root.path().join(".agent/stderr")).unwrap();
    assert!(
        run.cleanup(),
        "fixture did not release its runtime and action locks"
    );
    assert_eq!(launches, "preflight\n");
    if stalled {
        assert_eq!(status.and_then(|status| status.code()), Some(1), "{stderr}");
        assert!(
            stderr.contains("descendant cleanup could not be confirmed"),
            "{stderr}"
        );
        assert!(
            stderr.contains("child processes may still be running"),
            "{stderr}"
        );
        if probe_timeout {
            assert!(stderr.contains("timed out"), "{stderr}");
        }
        assert!(!lock_released, "fixture did not leave an action behind");
        assert!(
            !stopped,
            "fixture action did not survive forced termination"
        );
        return;
    }
    assert_eq!(
        status.and_then(|status| status.code()),
        Some(129),
        "{stderr}"
    );
    assert!(
        lock_released,
        "action still held its lock after cancellation"
    );
    assert!(stopped, "action heartbeat continued after cancellation");
    assert!(events.lines().any(|line| {
        let event: Value = serde_json::from_str(line).unwrap();
        event["event"] == "completed" && event["conclusion"] == "cancelled"
    }));
}

#[test]
fn fixture_cleanup_reaps_stopped_runtime_after_early_wrapper_exit() {
    let broken = WRAPPER.replace("cancel_phase(child, interrupted)", "None");
    let mut run = RuntimeRun::start(true, &broken, false);
    run.signal(libc::SIGTERM, false);
    assert_eq!(
        run.child
            .wait_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .code(),
        Some(143)
    );
    assert!(!run.released("runtime-owner.lock"));
    assert!(run.cleanup());
    assert!(
        run.fixture
            .root
            .path()
            .join(".agent/runtime-reaped")
            .exists()
    );
}

#[test]
fn fixture_cleanup_reaps_stopped_runtime_when_wrapper_hangs() {
    let broken = WRAPPER.replace("cancel_phase(child, interrupted)", "time.sleep(60)");
    let mut run = RuntimeRun::start(true, &broken, false);
    run.signal(libc::SIGTERM, false);
    assert!(
        run.child
            .wait_timeout(Duration::from_millis(100))
            .unwrap()
            .is_none()
    );
    assert!(run.cleanup());
    assert!(
        run.fixture
            .root
            .path()
            .join(".agent/runtime-reaped")
            .exists()
    );
}

fn heartbeat_changes(fixture: &Fixture, timeout: Duration) -> bool {
    let path = fixture.root.path().join(".agent/heartbeat");
    let initial = fs::read(&path).ok();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if fs::read(&path).is_ok_and(|current| !current.is_empty() && Some(current) != initial) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}
