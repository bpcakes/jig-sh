use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::super::group_cleanup::{MEMBERSHIP_PROOFS, signal_owned_process_tree_once_with};
use super::*;

#[cfg(target_os = "linux")]
mod parent_loss;

#[test]
fn cleanup_policy_controls_membership_proof_and_reaps_the_child() {
    assert_eq!(ProcessTreeCleanup::default(), ProcessTreeCleanup::Confirmed);
    for policy in [
        ProcessTreeCleanup::BestEffort,
        ProcessTreeCleanup::default(),
    ] {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut process = spawn_owned_process(&mut command).unwrap();
        process.cleanup = policy;
        MEMBERSHIP_PROOFS.with(|count| count.set(0));

        let status = process.terminate_and_reap().unwrap();

        assert_eq!(status.signal(), Some(libc::SIGKILL));
        assert!(process.reaped_status.is_some());
        assert!(process.process_group.is_none());
        let proofs = MEMBERSHIP_PROOFS.with(std::cell::Cell::get);
        match policy {
            ProcessTreeCleanup::BestEffort => assert_eq!(proofs, 0),
            ProcessTreeCleanup::Confirmed => assert!(proofs > 0),
        }
    }
}

#[test]
fn best_effort_retains_post_signal_timeout_even_when_the_child_can_be_reaped() {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = spawn_owned_process(&mut command).unwrap();
    let wait_deadline = Instant::now() + Duration::from_secs(10);
    while observe_owned_process(&mut process).unwrap() != OwnedProcessObservation::Exited {
        assert!(Instant::now() < wait_deadline, "child did not exit");
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut signaled = false;
    let error = process
        .terminate_and_reap_with(|process, deadline| {
            signal_owned_process_tree_once_with(
                process,
                deadline,
                |process, _, _| {
                    signal_owned_process_tree_once(process, deadline)?;
                    signaled = true;
                    Ok(ProcessGroupSignalResult::Delivered)
                },
                || deadline,
            )
        })
        .unwrap_err();

    assert!(signaled);
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("after process-group SIGKILL"));
    assert!(process.reaped_status.unwrap().success());
    assert!(process.process_group.is_none());
    assert!(!process.cleanup_complete);
    assert_eq!(
        process.terminate_and_reap().unwrap_err().kind(),
        error.kind()
    );
}

#[cfg(target_os = "linux")]
const OWNED_PROCESS_PARENT_DEATH_MARKER_ENV: &str = "JIG_OWNED_PROCESS_PARENT_DEATH_MARKER";

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_until_test_process_stops(identity: &TestProcessIdentity, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while identity.is_live() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    !identity.is_live()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn best_effort_cleanup_still_kills_group_members_left_by_the_leader() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("best-effort-descendant");
    let script = format!(
        "{marker_env}={marker} {test_exe} --exact process::tests::owned_process_descendant_helper --nocapture >/dev/null 2>&1 & while [ ! -f {marker} ]; do :; done; exit 0",
        marker_env = OWNED_PROCESS_DESCENDANT_MARKER_ENV,
        marker = shell_quote_test_path(&marker),
        test_exe = shell_quote_test_path(&std::env::current_exe().unwrap()),
    );
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = run_owned_process_tree_with_output_and_cleanup(
        &mut command,
        Duration::from_secs(10),
        ProcessTreeCleanup::BestEffort,
        || false,
    )
    .unwrap();
    let descendant = read_test_process_identity(&marker);
    let stopped = wait_until_test_process_stops(&descendant, Duration::from_secs(5));
    terminate_and_confirm_test_process(&descendant);

    assert!(output.status.success());
    assert!(
        stopped,
        "best-effort cleanup must still signal members the leader left in its group"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn owned_process_parent_death_supervisor_helper() {
    let Some(marker) = std::env::var_os(OWNED_PROCESS_PARENT_DEATH_MARKER_ENV) else {
        return;
    };
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "process::tests::owned_process_descendant_helper",
            "--nocapture",
        ])
        .env(OWNED_PROCESS_DESCENDANT_MARKER_ENV, &marker)
        .env_remove(OWNED_PROCESS_PARENT_DEATH_MARKER_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let _ = run_owned_process_tree_with_output(&mut command, Duration::from_secs(60), || false);
}

#[cfg(target_os = "linux")]
#[test]
fn owned_process_is_killed_when_its_supervisor_is_killed() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("parent-death-child");
    let mut supervisor = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "process::tests::cleanup_policy::owned_process_parent_death_supervisor_helper",
            "--nocapture",
        ])
        .env(OWNED_PROCESS_PARENT_DEATH_MARKER_ENV, &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let child = read_test_process_identity(&marker);

    supervisor.kill().unwrap();
    supervisor.wait().unwrap();
    let stopped = wait_until_test_process_stops(&child, Duration::from_secs(5));
    terminate_and_confirm_test_process(&child);

    assert!(
        stopped,
        "an owned child must not outlive a supervisor that was killed"
    );
}
