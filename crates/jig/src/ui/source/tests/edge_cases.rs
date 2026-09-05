use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Arc;

use jig_ui::dashboard::{
    CollectionDomain, DashboardSource, PlanBasis, PlanSnapshotResult, RecorderEpochId,
    RecorderMode, SourceError, TimelineRow,
};
use serde_json::json;

use super::{recorder_request, source_fixture};

#[test]
fn session_duplicates_are_canonical_and_conflicts_are_scoped() {
    let (root, source) = source_fixture();
    let event = json!({
        "id": "session-event-example",
        "session_id": "session-example",
        "event": "start",
        "timestamp_ms": 10,
        "summary": {"ignored": true}
    });
    fs::write(
        root.path().join(".agent/state/sessions.jsonl"),
        format!("{event}\n{event}\n"),
    )
    .unwrap();

    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_eq!(refresh.recorder.counts.sessions, 1);
    assert_eq!(refresh.recorder.counts.session_events, 1);
    assert_eq!(
        refresh
            .recorder
            .timeline
            .iter()
            .filter(|row| matches!(row, TimelineRow::Session(_)))
            .count(),
        1
    );

    let mut file = OpenOptions::new()
        .append(true)
        .open(root.path().join(".agent/state/sessions.jsonl"))
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({
            "id": "session-event-example",
            "session_id": "session-example",
            "event": "start",
            "timestamp_ms": 11
        })
    )
    .unwrap();
    let partial = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(partial.recorder.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Sessions.as_str() && error.code() == "stream_read_failed"
    }));
}

#[test]
fn current_session_and_oversized_plan_failures_are_visible_without_losing_other_facts() {
    let (root, source) = source_fixture();
    fs::create_dir(source.context.current_session_path()).unwrap();
    let current_session_partial = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(current_session_partial.recorder.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Sessions.as_str()
            && error.code() == "stream_read_failed"
            && error.message().contains("current session")
    }));
    assert!(current_session_partial.status_local.work.state.is_none());
    assert!(
        current_session_partial
            .status_local
            .errors
            .iter()
            .any(|error| { error.scope == "work.state" && error.code == "work_state_unavailable" })
    );
    fs::remove_dir(source.context.current_session_path()).unwrap();

    let path = root.path().join(".agent/state/plans.jsonl");
    let mut records = fs::read_to_string(&path).unwrap();
    records.push_str(&format!(
        "{}\n",
        json!({
            "id": "oversized-plan-event",
            "plan_id": "oversized-plan",
            "event": "open",
            "timestamp_ms": 99,
            "title": "x".repeat(1024 * 1024),
            "body_path": null,
            "baseline": null
        })
    ));
    fs::write(path, records).unwrap();
    let oversized_partial = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_eq!(
        oversized_partial.recorder.open_plans[0].plan_id,
        "plan_example"
    );
    assert!(oversized_partial.recorder.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Plans.as_str() && error.code() == "record_too_large"
    }));
    assert!(matches!(
        source.plan(
            PlanBasis::RecorderEpoch(oversized_partial.recorder.epoch_id),
            "plan_example".to_string(),
            &|| false,
        ),
        Err(SourceError::Collection {
            domain: CollectionDomain::Plans,
            ..
        })
    ));
}

#[test]
fn oversized_status_record_has_only_the_documented_partial_delta() {
    let (root, source) = source_fixture();
    let path = root.path().join(".agent/state/plans.jsonl");
    let mut records = fs::read_to_string(&path).unwrap();
    records.push_str(&format!(
        "{}\n",
        json!({
            "id": "oversized-plan-event",
            "plan_id": "oversized-plan",
            "event": "open",
            "timestamp_ms": 99,
            "title": "x".repeat(1024 * 1024),
            "body_path": null,
            "baseline": null
        })
    ));
    fs::write(path, records).unwrap();
    let _clock = crate::state::set_test_now_ms(1_900_000_000_000);

    let legacy = crate::status::snapshot_with_cancellation(&source.context, &|| false).unwrap();
    let refresh = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let typed = serde_json::to_value(refresh.status_local).unwrap();

    assert_eq!(typed["work"], legacy["work"]);
    assert_eq!(typed["errors"], legacy["errors"]);
    assert!(typed["work"]["state"].is_null());
    assert_eq!(typed["work"]["gates"], json!([]));
    let oversized_errors = typed["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|error| error["code"] == "work_state_unavailable")
        .collect::<Vec<_>>();
    assert_eq!(oversized_errors.len(), 1);
    assert_eq!(oversized_errors[0]["scope"], "work.state");
    assert!(
        oversized_errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("exceeds the 1048576-byte dashboard read limit")
    );
}

#[test]
fn timeline_identity_survives_append_but_changes_with_replacement_bytes() {
    let (root, source) = source_fixture();
    let path = root.path().join(".agent/state/decisions.jsonl");
    let first = json!({
        "id": "decision-example",
        "session_id": null,
        "plan_id": "plan_example",
        "timestamp_ms": 30,
        "title": "Choose",
        "selected_option": "A",
        "alternatives": [],
        "rationale": "Stable"
    });
    fs::write(&path, format!("{first}\n")).unwrap();
    let before = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    let before_id = decision_identity(&before.recorder.timeline, "decision-example");

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(
        file,
        "{}",
        json!({
            "id": "decision-later",
            "session_id": null,
            "plan_id": "plan_example",
            "timestamp_ms": 31,
            "title": "Choose later",
            "selected_option": "B",
            "alternatives": [],
            "rationale": "Append"
        })
    )
    .unwrap();
    let appended = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_eq!(
        decision_identity(&appended.recorder.timeline, "decision-example"),
        before_id
    );

    let mut replacement = first;
    replacement["selected_option"] = json!("C");
    fs::write(&path, format!("{replacement}\n")).unwrap();
    let replaced = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_ne!(
        decision_identity(&replaced.recorder.timeline, "decision-example"),
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
fn fresh_detail_is_monotonic_and_transient_and_epoch_exhaustion_keeps_cache() {
    let (_root, source) = source_fixture();
    let retained = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap()
        .recorder
        .epoch_id;
    crate::state::reset_dashboard_scan_counts();
    let PlanSnapshotResult::Found(fresh) = source
        .plan(PlanBasis::Fresh, "plan_example".to_string(), &|| false)
        .unwrap()
    else {
        panic!("fresh plan should exist");
    };
    assert!(fresh.basis_epoch > retained);
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("plans.jsonl")),
        1
    );
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("decisions.jsonl")),
        1
    );
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("receipts.jsonl")),
        1
    );
    assert_eq!(
        crate::state::dashboard_scan_count(&source.context.state_file("sessions.jsonl")),
        0,
        "Fresh plan detail must not collect unrelated dashboard state"
    );
    assert_eq!(
        source
            .recorder(recorder_request(RecorderMode::ReuseCurrent), &|| false)
            .unwrap()
            .recorder
            .epoch_id,
        retained
    );

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
fn duplicate_open_is_gate_corruption_and_close_remains_sticky() {
    let (root, source) = source_fixture();
    let plans_path = root.path().join(".agent/state/plans.jsonl");
    let mut plans = fs::read_to_string(&plans_path).unwrap();
    plans.push_str(&format!(
        "{}\n",
        json!({
            "id": "plan-open-duplicate",
            "plan_id": "plan_example",
            "event": "open",
            "timestamp_ms": 50,
            "title": "Duplicate",
            "body_path": null,
            "baseline": null
        })
    ));
    fs::write(&plans_path, &plans).unwrap();
    let epoch = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert!(
        epoch.recorder.open_plans[0]
            .gates_error
            .as_deref()
            .is_some_and(|error| error.contains("multiple Open records"))
    );
    assert!(epoch.status_local.errors.iter().any(|error| {
        error.scope == "work.gates.plan_example" && error.code == "work_gates_unavailable"
    }));
    let PlanSnapshotResult::Found(detail) = source
        .plan(
            PlanBasis::RecorderEpoch(epoch.recorder.epoch_id),
            "plan_example".to_string(),
            &|| false,
        )
        .unwrap()
    else {
        panic!("duplicate-open plan should remain inspectable");
    };
    assert!(detail.errors.iter().any(|error| {
        error.scope() == CollectionDomain::Gates.as_str()
            && error.message().contains("multiple Open records")
    }));

    plans.push_str(&format!(
        "{}\n{}\n{}\n",
        json!({
            "id": "plan-close-sticky",
            "plan_id": "plan_example",
            "event": "close",
            "timestamp_ms": 60,
            "resolution": "complete"
        }),
        json!({
            "id": "plan-open-after-close",
            "plan_id": "plan_example",
            "event": "open",
            "timestamp_ms": 70,
            "title": "Must remain closed",
            "body_path": null,
            "baseline": null
        }),
        json!({
            "id": "plan-open-other",
            "plan_id": "plan_other",
            "event": "open",
            "timestamp_ms": 80,
            "title": "Other plan",
            "body_path": null,
            "baseline": null
        })
    ));
    fs::write(plans_path, plans).unwrap();
    let refreshed = source
        .recorder(recorder_request(RecorderMode::Refresh), &|| false)
        .unwrap();
    assert_eq!(refreshed.recorder.open_plans.len(), 1);
    assert_eq!(refreshed.recorder.open_plans[0].plan_id, "plan_other");
    assert!(refreshed.recorder.open_plans[0].gates_error.is_none());
    assert_eq!(refreshed.recorder.history[0].plan_id, "plan_example");
    assert_eq!(refreshed.recorder.history[0].state, "closed");
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
    assert!(matches!(
        source
            .plan(
                PlanBasis::RecorderEpoch(ids[0]),
                "plan_example".to_string(),
                &|| false,
            )
            .unwrap(),
        PlanSnapshotResult::StaleRecorderEpoch
    ));
}

fn decision_identity(rows: &[TimelineRow], id: &str) -> String {
    rows.iter()
        .find_map(|row| match row {
            TimelineRow::Decision(row) if row.id == id => Some(row.stable_identity.clone()),
            _ => None,
        })
        .expect("decision should be present")
}
