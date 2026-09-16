use std::fs;
use std::os::unix::fs::symlink;

use super::*;
use crate::test_env::{EnvVarGuard, lock_env};

#[test]
fn unsafe_shared_memory_sidecar_blocks_mutation_before_readiness() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let shm = fixture.root.join(".beads/beads.db-shm");
    let outside = fixture._temp.path().join("outside-shm");
    fs::write(&shm, b"shared memory").unwrap();
    fs::hard_link(&shm, &outside).unwrap();
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::HardLinkedAuthority
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);

    fs::remove_file(&outside).unwrap();
    fs::remove_file(&shm).unwrap();
    fs::write(&outside, b"external shared memory").unwrap();
    symlink(&outside, &shm).unwrap();
    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::Database
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn sidecar_change_after_preparation_is_rejected_before_spawn() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let policy = TrackerProcessPolicy::default();
    let (adapter, _) = fixture.discover(policy);
    let runner = process::ProcessRunner::new(
        &adapter.root,
        &adapter.executable,
        adapter.store.as_ref(),
        policy,
    );
    let shm = fixture.root.join(".beads/beads.db-shm");
    let outside = fixture._temp.path().join("outside-shm");
    let mut never_cancelled = || false;

    assert_eq!(
        runner
            .run_json_after_prepare(
                TrackerOperation::ClaimIssue,
                &[
                    "update",
                    "--claim",
                    &format!("--actor={ACTOR}"),
                    "--",
                    ISSUE_ID,
                ],
                &mut never_cancelled,
                || {
                    fs::write(&shm, b"shared memory").unwrap();
                    fs::hard_link(&shm, &outside).unwrap();
                },
            )
            .unwrap_err(),
        TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::HardLinkedAuthority
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 2);
}

#[test]
fn unsafe_sidecar_observed_after_mutation_is_indeterminate() {
    let _env = lock_env();
    let fixture = Fixture::new("0.5.7");
    let _br = TestBrOverride::set(Some(&fixture.bin.join("br")));
    let _log = EnvVarGuard::set("JIG_TRACKER_TEST_LOG", &fixture.log);
    let _actor = EnvVarGuard::set("BD_ACTOR", "Ambient Actor");
    let (adapter, _) = fixture.discover(TrackerProcessPolicy::default());
    let shm = fixture.root.join(".beads/beads.db-shm");
    let outside = fixture._temp.path().join("outside-shm");
    let _hook = TestAfterProfiledProcessHook::set(move |operation| {
        if operation == TrackerOperation::ClaimIssue {
            fs::write(&shm, b"shared memory").unwrap();
            fs::hard_link(&shm, &outside).unwrap();
        }
    });
    let mut never_cancelled = || false;

    assert_eq!(
        adapter
            .claim_issue(ISSUE_ID, ACTOR, &mut never_cancelled)
            .unwrap_err(),
        TrackerError::IndeterminateWrite {
            operation: TrackerOperation::ClaimIssue
        }
    );
    assert_eq!(logged_argument_groups(&fixture.log).len(), 4);
}
