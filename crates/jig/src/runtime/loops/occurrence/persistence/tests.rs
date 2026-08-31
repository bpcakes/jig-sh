use std::cell::RefCell;

use super::*;
use crate::context::RepoContext;
use crate::runtime::loops::state::read_json_or_default;
use crate::test_env::TestRepoBuilder;

#[test]
fn durable_publish_orders_sync_replace_and_directory_sync() {
    let steps = RefCell::new(Vec::new());

    publish_durable(
        || {
            steps.borrow_mut().push("sync_file");
            Ok(())
        },
        || {
            steps.borrow_mut().push("rename");
            Ok(())
        },
        || {
            steps.borrow_mut().push("sync_directory");
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(
        steps.into_inner(),
        ["sync_file", "rename", "sync_directory"]
    );
}

#[cfg(unix)]
#[test]
fn unchanged_legacy_marker_is_not_rewritten() {
    use std::os::unix::fs::MetadataExt;

    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    persistence.with_locked(|_| Ok(())).unwrap();
    let original_inode = fs::metadata(&persistence.legacy_path).unwrap().ino();

    persistence.read_locked(|_| Ok(())).unwrap();

    assert_eq!(
        fs::metadata(&persistence.legacy_path).unwrap().ino(),
        original_inode
    );
}

#[test]
fn durable_publish_stops_before_replace_when_file_sync_fails() {
    let steps = RefCell::new(Vec::new());

    let error = publish_durable(
        || {
            steps.borrow_mut().push("sync_file");
            anyhow::bail!("injected sync failure")
        },
        || {
            steps.borrow_mut().push("rename");
            Ok(())
        },
        || {
            steps.borrow_mut().push("sync_directory");
            Ok(())
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("injected sync failure"));
    assert_eq!(steps.into_inner(), ["sync_file"]);
}

#[test]
fn durable_directory_creation_builds_missing_parent_chain() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join(".agent/runtime/loop");

    ensure_durable_directory(&nested).unwrap();

    assert!(nested.is_dir());
}

#[test]
fn durable_write_publishes_a_readable_schedule_without_a_temp_leftover() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("schedule.json");

    write_json_durable(&path, &ScheduleFile::default()).unwrap();

    let published: ScheduleFile = read_json_or_default(&path).unwrap();
    assert_eq!(published.schema_version, SCHEDULE_SCHEMA_VERSION);
    assert!(published.migrated_to.is_none());
    assert!(published.occurrences.is_empty());
    assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("schedule.tmp-")
    }));
}

#[test]
fn durable_directory_rejects_a_non_directory_path() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("loop");
    fs::write(&path, "not a directory").unwrap();

    let error = ensure_durable_directory(&path).unwrap_err().to_string();

    assert!(error.contains("is not a directory"), "{error}");
}

#[test]
fn durable_write_removes_temporary_file_after_rename_failure() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("schedule.json");
    fs::create_dir(&path).unwrap();

    let error = write_json_durable(&path, &ScheduleFile::default())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Failed to replace loop schedule state"),
        "{error}"
    );
    let leftovers = fs::read_dir(temp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with("schedule.tmp-"))
        .collect::<Vec<_>>();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn read_only_rechecks_durable_state_when_marker_follows_initial_miss() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .required_commands(Vec::<String>::new())
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let persistence = SchedulePersistence::new(&ctx);
    let durable = ScheduleFile::default();
    write_json_durable(&persistence.path, &durable).unwrap();
    let marker = ScheduleFile {
        schema_version: SCHEDULE_SCHEMA_VERSION,
        migrated_to: Some(SCHEDULE_STATE_PATH.into()),
        occurrences: BTreeMap::new(),
    };

    let resolved = persistence
        .resolve_read_only(None, Some(marker), &|| false)
        .unwrap();

    assert!(resolved == durable);
}
