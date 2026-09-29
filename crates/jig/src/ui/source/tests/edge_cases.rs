use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Arc;

use jig_ui::dashboard::{
    CollectionDomain, DashboardSource, RecorderEpochId, RecorderMode, SnapshotErrorCode,
    SourceError, TimelineRow,
};
use serde_json::json;

use super::{recorder_request, source_fixture};

#[test]
fn oversized_run_record_is_a_recorder_partial_error_outside_status() {
    let (root, source) = source_fixture();
    let path = root.path().join(".agent/state/runs.jsonl");
    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(
        file,
        "{}",
        json!({"id": "oversized-run-event", "event": "x".repeat(1024 * 1024)})
    )
    .unwrap();
    let _clock = crate::state::set_test_now_ms(1_900_000_000_000);

    let legacy = crate::status::snapshot_with_cancellation(&source.context, &|| false).unwrap();
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();

    let run_errors = refresh
        .recorder
        .errors
        .iter()
        .filter(|error| error.scope() == CollectionDomain::Runs.as_str())
        .collect::<Vec<_>>();
    assert_eq!(run_errors.len(), 1);
    assert_eq!(
        run_errors[0].code(),
        SnapshotErrorCode::RecordTooLarge.as_str()
    );
    assert!(
        run_errors[0]
            .message()
            .contains("exceeds the 1048576-byte dashboard read limit")
    );
    let typed = serde_json::to_value(refresh.status_local).unwrap();
    assert_eq!(typed["errors"], legacy["errors"]);
    assert!(
        typed["errors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|error| error["scope"] == "repository"),
        "{typed:#}"
    );
}

#[test]
fn timeline_identity_survives_append_but_changes_with_replacement_bytes() {
    let (root, source) = source_fixture();
    let path = root.path().join(".agent/state/runs.jsonl");
    let original = fs::read_to_string(&path).unwrap();
    let run_id = serde_json::from_str::<serde_json::Value>(original.lines().next().unwrap())
        .unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let before_id = row_identity(&before.recorder.timeline, &run_id);

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(original.replace(&run_id, "run_example_later").as_bytes())
        .unwrap();
    let appended = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_eq!(
        row_identity(&appended.recorder.timeline, &run_id),
        before_id
    );

    fs::write(&path, original.replace("\"success\"", "\"failure\"")).unwrap();
    let replaced = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_ne!(
        row_identity(&replaced.recorder.timeline, &run_id),
        before_id
    );
}

#[test]
fn cancelled_recorder_refresh_keeps_the_retained_epoch() {
    let (_root, source) = source_fixture();
    let retained = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;

    let result = source.recorder(recorder_request(RecorderMode::Refresh), &|| true);
    assert_eq!(result.unwrap_err(), SourceError::Cancelled);
    assert_eq!(
        source
            .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
            .unwrap()
            .recorder
            .epoch_id,
        retained
    );
}

#[test]
fn epoch_exhaustion_keeps_the_retained_epoch() {
    let (_root, source) = source_fixture();
    let retained = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;

    source.state.lock().unwrap().last_epoch_id = RecorderEpochId::new(u64::MAX);
    assert!(matches!(
        source.recorder(recorder_request(RecorderMode::Refresh), &|| false),
        Err(SourceError::InternalContract { message }) if message == "recorder epoch exhausted"
    ));
    assert_eq!(
        source
            .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
            .unwrap()
            .recorder
            .epoch_id,
        retained
    );
}

#[test]
fn concurrent_refreshes_publish_only_the_newest_epoch() {
    let (_root, source) = source_fixture();
    let source = Arc::new(source);
    let handles = (0..2)
        .map(|_| {
            let source = Arc::clone(&source);
            std::thread::spawn(move || {
                source
                    .recorder(recorder_request(RecorderMode::Refresh), &|| false)
                    .unwrap()
                    .recorder
                    .epoch_id
            })
        })
        .collect::<Vec<_>>();
    let mut ids = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    ids.sort();
    assert_ne!(ids[0], ids[1]);
    let retained = source
        .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
        .unwrap()
        .recorder
        .epoch_id;
    assert_eq!(retained, ids[1]);
}

fn row_identity(rows: &[TimelineRow], run_id: &str) -> String {
    rows.iter()
        .find_map(|row| (row.run_id == run_id).then(|| row.stable_identity.clone()))
        .expect("target result should be present")
}
