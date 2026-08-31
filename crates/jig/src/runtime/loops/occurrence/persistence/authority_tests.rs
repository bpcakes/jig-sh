use std::fs::{self, OpenOptions};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::context::RepoContext;
use crate::runtime::loops::state::read_json_or_default;
use crate::test_env::{EnvVarGuard, TestRepoBuilder, lock_env};
use fs4::fs_std::FileExt;

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
fn legacy_migration_waits_for_the_protected_authority_lock() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    let protected = persistence.protected_authority().unwrap().unwrap().clone();
    fs::create_dir_all(&protected.dir).unwrap();
    write_json_durable(
        &persistence.root,
        &persistence.legacy_path,
        &ScheduleFile::default(),
    )
    .unwrap();
    let authority_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&protected.lock_path)
        .unwrap();
    authority_lock.lock_exclusive().unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let migrating = thread::spawn(move || {
        started_tx.send(()).unwrap();
        persistence.read_locked(|store| Ok(store.clone()))
    });
    started_rx.recv().unwrap();
    thread::sleep(Duration::from_millis(100));

    assert!(
        !protected.path.exists(),
        "legacy migration published protected state without owning its lock"
    );

    FileExt::unlock(&authority_lock).unwrap();
    migrating.join().unwrap().unwrap();
    assert!(protected.path.is_file());
}

#[test]
fn schedule_lock_pair_shares_one_operation_deadline() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    persistence.with_locked(|_| Ok(())).unwrap();
    let protected = persistence.protected_authority().unwrap().unwrap().clone();
    let authority_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&protected.lock_path)
        .unwrap();
    authority_lock.lock_exclusive().unwrap();

    let started = Instant::now();
    let error = persistence
        .with_locked_until(Instant::now(), |_| Ok(()))
        .unwrap_err()
        .to_string();

    FileExt::unlock(&authority_lock).unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(error.contains("operation deadline"), "{error}");
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

#[test]
fn protected_authority_is_commit_point_when_replica_publication_fails() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    persistence.with_locked(|_| Ok(())).unwrap();
    fs::remove_file(&persistence.path).unwrap();
    fs::create_dir(&persistence.path).unwrap();

    persistence
        .with_locked(|store| {
            store.occurrences.insert(
                "scheduled:example:100".into(),
                super::super::ScheduleOccurrence {
                    occurrence_id: "scheduled:example:100".into(),
                    workflow_id: "example".into(),
                    scheduled_at_ms: 100,
                    owner: "fixture-owner".into(),
                    claim_expires_at_ms: 200,
                    started_at_ms: 100,
                    uses_shared_checkout: Some(false),
                    finished_at_ms: Some(150),
                    acknowledged_at_ms: None,
                    status: super::super::OccurrenceStatus::Succeeded,
                    worker_receipt_id: Some("receipt_fixture".into()),
                    worktree: None,
                    error: None,
                },
            );
            Ok(())
        })
        .unwrap();

    let protected = persistence.protected_authority().unwrap().unwrap();
    let authoritative: ScheduleFile = read_json_or_default(&protected.path).unwrap();
    assert!(
        authoritative
            .occurrences
            .contains_key("scheduled:example:100")
    );

    fs::remove_dir(&persistence.path).unwrap();
    persistence.with_locked(|_| Ok(())).unwrap();
    assert_eq!(
        fs::read(&protected.path).unwrap(),
        fs::read(&persistence.path).unwrap()
    );
}

#[test]
fn authority_resolution_does_not_execute_the_configured_git_binary() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let _git = EnvVarGuard::set(
        crate::bootstrap::GIT_BIN_ENV,
        "authority-probe-must-not-run",
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let persistence = SchedulePersistence::new(&ctx);

    assert!(persistence.protected_authority().unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn symbolic_link_git_metadata_is_rejected_as_mutable_redirection() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    let metadata = temp.path().join("git-metadata");
    fs::rename(temp.path().join(".git"), &metadata).unwrap();
    symlink(&metadata, temp.path().join(".git")).unwrap();
    git(temp.path(), &["rev-parse", "--git-dir"]);

    let error = resolve_protected_schedule_authority(temp.path()).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must be a directory or regular pointer file"),
        "{error:#}"
    );
}

#[cfg(unix)]
#[test]
fn dangling_symbolic_link_git_metadata_is_rejected_instead_of_treated_as_non_git() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    symlink(
        temp.path().join("missing-git-metadata"),
        temp.path().join(".git"),
    )
    .unwrap();

    let error = resolve_protected_schedule_authority(temp.path()).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must be a directory or regular pointer file"),
        "{error:#}"
    );
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
        error.to_string().contains("component is a symlink"),
        "{error:#}"
    );
    assert!(fs::read_dir(redirected.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn checkout_replica_ancestor_cannot_redirect_schedule_writes_through_a_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let redirected = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    git(temp.path(), &["init"]);
    fs::create_dir_all(redirected.path().join("loop")).unwrap();
    symlink(redirected.path(), temp.path().join(".agent/runtime")).unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);

    let error = persistence.with_locked(|_| Ok(())).unwrap_err();

    assert!(
        error.to_string().contains("component is a symlink"),
        "{error:#}"
    );
    assert!(
        fs::read_dir(redirected.path().join("loop"))
            .unwrap()
            .next()
            .is_none()
    );
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
    write_json_durable(&persistence.root, &persistence.path, &public).unwrap();
    write_json_durable(
        &protected.root,
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
