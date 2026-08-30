use tempfile::tempdir;

use super::super::*;
use super::write_loop_fixture_repo;
use crate::runtime::loops::state::{LOOP_CACHE_DIR, LOOP_RUNTIME_DIR, with_exclusive_file_lock};

#[test]
fn schedule_state_migrates_out_of_disposable_cache_and_survives_cache_removal() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": LEGACY_SCHEDULE_SCHEMA_VERSION,
            "occurrences": {
                "nightly@100": {
                    "occurrence_id": "nightly@100",
                    "workflow_id": "nightly",
                    "scheduled_at_ms": 100,
                    "owner": "owner",
                    "claim_expires_at_ms": 200,
                    "started_at_ms": 100,
                    "finished_at_ms": 150,
                    "status": "succeeded"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let migrated = OccurrenceStore::new(&ctx).snapshot().unwrap();
    assert_eq!(migrated.len(), 1);
    assert_eq!(migrated[0].status, OccurrenceStatus::Succeeded);
    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    let durable_path = runtime_dir.join("schedule.json");
    assert!(durable_path.is_file());
    assert!(runtime_dir.join("schedule.initialized").is_file());
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(legacy_dir.join("schedule.json")).unwrap()).unwrap();
    assert_eq!(marker["schema_version"], SCHEDULE_SCHEMA_VERSION);
    assert_eq!(marker["migrated_to"], ".agent/runtime/loop/schedule.json");

    std::fs::remove_dir_all(&legacy_dir).unwrap();
    let after_cache_removal = OccurrenceStore::new(&ctx).snapshot().unwrap();
    assert_eq!(after_cache_removal, migrated);

    std::fs::remove_file(&durable_path).unwrap();
    let read_error = OccurrenceStore::new(&ctx)
        .snapshot_read_only_with_cancellation(&|| false)
        .unwrap_err()
        .to_string();
    let locked_error = OccurrenceStore::new(&ctx)
        .snapshot()
        .unwrap_err()
        .to_string();
    assert!(read_error.contains("Initialized loop schedule state is missing"));
    assert!(locked_error.contains("Initialized loop schedule state is missing"));
}

#[test]
fn locked_read_fails_if_an_initialized_ledger_disappears_while_waiting_for_the_lock() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);
    let OccurrenceClaim::Acquired(_) = store.claim_at("nightly", 100, 60, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    let root = temp.path().to_path_buf();
    let runtime_dir = root.join(LOOP_RUNTIME_DIR);
    let durable_path = runtime_dir.join("schedule.json");
    let lock_path = runtime_dir.join("schedule.lock");

    let reader = with_exclusive_file_lock(&runtime_dir, &lock_path, || {
        let root = root.clone();
        let reader = std::thread::spawn(move || {
            let ctx = RepoContext::load_from(&root).unwrap();
            OccurrenceStore::new(&ctx).snapshot()
        });
        std::fs::remove_file(&durable_path).unwrap();
        Ok(reader)
    })
    .unwrap();
    let error = reader.join().unwrap().unwrap_err().to_string();

    assert!(error.contains("Initialized loop schedule state is missing"));
}

#[test]
fn previous_durable_schedule_schema_migrates_forward() {
    let mut store = ScheduleFile {
        schema_version: PREVIOUS_SCHEDULE_SCHEMA_VERSION,
        migrated_to: None,
        occurrences: Default::default(),
    };

    migrate_schedule_schema(&mut store).unwrap();

    assert_eq!(store.schema_version, SCHEDULE_SCHEMA_VERSION);
}

#[test]
fn fresh_schedule_initialization_publishes_the_legacy_downgrade_marker() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut store = OccurrenceStore::new(&ctx);

    let OccurrenceClaim::Acquired(_) = store.claim_at("nightly", 100, 60, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };

    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    let legacy_path = temp.path().join(LOOP_CACHE_DIR).join("schedule.json");
    assert!(runtime_dir.join("schedule.json").is_file());
    assert!(runtime_dir.join("schedule.initialized").is_file());
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(legacy_path).unwrap()).unwrap();
    assert_eq!(marker["schema_version"], SCHEDULE_SCHEMA_VERSION);
    assert_eq!(marker["migrated_to"], ".agent/runtime/loop/schedule.json");
    let error = validate_schema_version(
        marker["schema_version"].as_u64().unwrap() as u32,
        LEGACY_SCHEDULE_SCHEMA_VERSION,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains(&format!(
            "version {SCHEDULE_SCHEMA_VERSION}; expected {LEGACY_SCHEDULE_SCHEMA_VERSION}"
        )),
        "{error}"
    );
}

#[test]
fn previous_runtime_rejects_the_current_schedule_schema() {
    let error = validate_schema_version(SCHEDULE_SCHEMA_VERSION, PREVIOUS_SCHEDULE_SCHEMA_VERSION)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains(&format!(
            "version {SCHEDULE_SCHEMA_VERSION}; expected {PREVIOUS_SCHEDULE_SCHEMA_VERSION}"
        )),
        "{error}"
    );
}

#[test]
fn previous_migration_marker_is_upgraded_to_the_downgrade_barrier() {
    let temp = tempdir().unwrap();
    write_loop_fixture_repo(temp.path());
    let legacy_dir = temp.path().join(LOOP_CACHE_DIR);
    let runtime_dir = temp.path().join(LOOP_RUNTIME_DIR);
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let previous = serde_json::json!({
        "schema_version": PREVIOUS_SCHEDULE_SCHEMA_VERSION,
        "occurrences": {}
    });
    std::fs::write(
        runtime_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&previous).unwrap(),
    )
    .unwrap();
    std::fs::write(
        legacy_dir.join("schedule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": PREVIOUS_SCHEDULE_SCHEMA_VERSION,
            "migrated_to": ".agent/runtime/loop/schedule.json",
            "occurrences": {}
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let mut store = OccurrenceStore::new(&ctx);
    assert!(store.snapshot().unwrap().is_empty());

    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(legacy_dir.join("schedule.json")).unwrap()).unwrap();
    assert_eq!(marker["schema_version"], SCHEDULE_SCHEMA_VERSION);
    let durable: serde_json::Value =
        serde_json::from_slice(&std::fs::read(runtime_dir.join("schedule.json")).unwrap()).unwrap();
    assert_eq!(
        durable["schema_version"], PREVIOUS_SCHEDULE_SCHEMA_VERSION,
        "a locked read must not rewrite the durable ledger"
    );

    let OccurrenceClaim::Acquired(_) = store.claim_at("nightly", 100, 60, 1_000).unwrap() else {
        panic!("expected occurrence claim");
    };
    let durable: serde_json::Value =
        serde_json::from_slice(&std::fs::read(runtime_dir.join("schedule.json")).unwrap()).unwrap();
    assert_eq!(durable["schema_version"], SCHEDULE_SCHEMA_VERSION);
}
