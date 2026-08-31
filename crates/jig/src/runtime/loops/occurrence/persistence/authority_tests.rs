use std::fs;
use std::process::Command;

use super::*;
use crate::context::RepoContext;
use crate::runtime::loops::state::read_json_or_default;
use crate::test_env::TestRepoBuilder;

#[test]
fn mutation_runs_only_after_schedule_authority_is_published() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);

    persistence
        .with_locked(|_| {
            assert!(persistence.path.is_file());
            assert!(persistence.initialized_path.is_file());
            let mut legacy: ScheduleFile = read_json_or_default(&persistence.legacy_path).unwrap();
            assert!(legacy_is_migration_marker(&mut legacy, &persistence.legacy_path).unwrap());
            Ok(())
        })
        .unwrap();
}

#[test]
fn git_repository_publishes_a_protected_ledger_and_lock() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);

    persistence.with_locked(|_| Ok(())).unwrap();

    let protected = persistence
        .protected_authority()
        .unwrap()
        .expect("Git repositories need out-of-checkout schedule authority");
    assert!(protected.path.is_file(), "{}", protected.path.display());
    assert!(protected.initialized_path.is_file());
    assert!(protected.lock_path.is_file());
    assert_eq!(
        fs::read(&protected.path).unwrap(),
        fs::read(&persistence.path).unwrap()
    );
}

#[test]
fn protected_ledger_ignores_a_forged_checkout_replica() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    persistence
        .with_locked(|store| {
            store.schema_version = SCHEDULE_SCHEMA_VERSION;
            Ok(())
        })
        .unwrap();
    fs::write(&persistence.path, b"not valid schedule JSON").unwrap();
    fs::write(&persistence.initialized_path, b"not valid marker JSON").unwrap();

    let snapshot = persistence.read_locked(|store| Ok(store.clone())).unwrap();

    assert_eq!(snapshot.schema_version, SCHEDULE_SCHEMA_VERSION);
    assert!(snapshot.occurrences.is_empty());
    let marker = read_json_if_exists_with_cancellation::<ScheduleInitializationMarker>(
        &persistence.initialized_path,
        &|| false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        marker.schema_version,
        SCHEDULE_INITIALIZATION_SCHEMA_VERSION
    );
    assert_eq!(marker.state_path, SCHEDULE_STATE_PATH);
}

#[cfg(unix)]
#[test]
fn checkout_replica_directory_cannot_redirect_schedule_writes_through_a_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let redirected = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    fs::create_dir_all(temp.path().join(".agent/runtime")).unwrap();
    symlink(redirected.path(), temp.path().join(".agent/runtime/loop")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);

    let error = persistence.with_locked(|_| Ok(())).unwrap_err();

    assert!(
        error.to_string().contains("is not a directory"),
        "{error:#}"
    );
    assert!(fs::read_dir(redirected.path()).unwrap().next().is_none());
}

#[test]
fn protected_initialization_witness_upgrades_without_losing_public_state() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    let protected = persistence.protected_authority().unwrap().unwrap();
    let mut public = ScheduleFile::default();
    public.occurrences.insert(
        "scheduled:example:100".into(),
        super::super::ScheduleOccurrence {
            occurrence_id: "scheduled:example:100".into(),
            workflow_id: "example".into(),
            scheduled_at_ms: 100,
            owner: "fixture-owner".into(),
            claim_expires_at_ms: 200,
            started_at_ms: 100,
            uses_shared_checkout: Some(false),
            finished_at_ms: None,
            acknowledged_at_ms: None,
            status: super::super::OccurrenceStatus::Running,
            worker_receipt_id: None,
            worktree: None,
            error: None,
        },
    );
    write_json_durable(&persistence.path, &public).unwrap();
    write_json_durable(
        &protected.initialized_path,
        &ScheduleInitializationMarker {
            schema_version: SCHEDULE_INITIALIZATION_SCHEMA_VERSION,
            state_path: SCHEDULE_STATE_PATH.into(),
        },
    )
    .unwrap();

    let snapshot = persistence.read_locked(|store| Ok(store.clone())).unwrap();

    assert!(snapshot == public);
    assert!(read_json_or_default::<ScheduleFile>(&protected.path).unwrap() == public);
    let marker = read_json_if_exists_with_cancellation::<ScheduleInitializationMarker>(
        &protected.initialized_path,
        &|| false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        marker.schema_version,
        PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION
    );
    assert_eq!(marker.state_path, PROTECTED_SCHEDULE_STATE_PATH);
}

#[test]
fn linked_worktree_uses_its_own_protected_schedule_authority() {
    let container = tempfile::tempdir().unwrap();
    let main = container.path().join("main");
    let linked = container.path().join("linked");
    fs::create_dir(&main).unwrap();
    TestRepoBuilder::new(&main).write();
    for args in [
        &["init"][..],
        &["config", "user.email", "fixture@example.com"],
        &["config", "user.name", "Fixture"],
        &["add", "."],
        &["commit", "-m", "fixture"],
    ] {
        git(&main, args);
    }
    git(
        &main,
        &["worktree", "add", "--detach", linked.to_str().unwrap()],
    );

    let main_ctx = RepoContext::load_from(&main).unwrap();
    let linked_ctx = RepoContext::load_from(&linked).unwrap();
    let main_persistence = SchedulePersistence::new(&main_ctx);
    let linked_persistence = SchedulePersistence::new(&linked_ctx);
    let main_authority = main_persistence.protected_authority().unwrap();
    let linked_authority = linked_persistence.protected_authority().unwrap();

    assert!(main_authority.is_some());
    assert!(linked_authority.is_some());
    assert_ne!(main_authority.unwrap().path, linked_authority.unwrap().path);
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
}
