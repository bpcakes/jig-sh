use std::fs;
use std::path::Path;

use fs4::fs_std::FileExt;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::jsonl::{
    read_jsonl_with_cancellation, read_jsonl_with_data_lock, read_jsonl_with_io,
    read_receipt_window_with_bytes, receipts_for_plan_with_lock, state_lock_path,
    with_jsonl_write_lock, write_jsonl_locked,
};
use super::records::SessionEvent;
use super::*;
use crate::context::RepoContext;
use crate::git_receipts::DiffStat;
use crate::test_env::TestRepoBuilder;
use crate::tool_defs::tool;

fn write_fixture_repo(root: &Path) {
    TestRepoBuilder::new(root)
        .required_commands(["rust_fmt_check_command"])
        .write();
}

#[test]
fn appends_jsonl_records() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    append_jsonl(&path, &json!({ "id": 1 })).unwrap();
    append_jsonl(&path, &json!({ "id": 2 })).unwrap();

    let items: Vec<Value> = read_jsonl(&path).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[1]["id"], 2);
}

#[test]
fn missing_jsonl_read_does_not_materialize_state() {
    let temp = tempdir().unwrap();
    let parent = temp.path().join("missing/state");
    let path = parent.join("events.jsonl");

    let items = read_jsonl::<Value>(&path).unwrap();

    assert!(items.is_empty());
    assert!(!parent.exists());
}

#[test]
fn receipt_window_reads_a_bounded_tail_independent_of_old_history() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for index in 0..100 {
        let output = "x".repeat(4_000);
        record_receipt(
            &ctx,
            ReceiptInput {
                tool_name: "jig.test",
                args: json!({}),
                invoked_command_key: Some("test".into()),
                plan_id: None,
                started_at_ms: index as u64,
                ended_at_ms: index as u64 + 1,
                exit_status: i32::from(index == 50),
                stdout: &output,
                stderr: "",
                evidence: None,
                session_override: None,
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
                worktree_fingerprint_override: None,
            },
        )
        .unwrap();
    }

    let path = ctx.state_file("receipts.jsonl");
    let file_len = fs::metadata(&path).unwrap().len();
    let (recent, bytes_read) = read_receipt_window_with_bytes(&path, 1).unwrap();

    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].exit_status, 0);
    assert!(bytes_read <= 16 * 1024, "read {bytes_read} bytes");
    assert!(
        bytes_read < file_len / 2,
        "tail read should not scan old history"
    );
}

#[test]
fn receipt_plan_query_uses_stable_snapshot_when_advisory_locks_are_unsupported() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    record_receipt(
        &ctx,
        ReceiptInput {
            tool_name: "jig.test",
            args: json!({}),
            invoked_command_key: Some("test".into()),
            plan_id: Some("plan_fallback".into()),
            started_at_ms: 1,
            ended_at_ms: 2,
            exit_status: 0,
            stdout: "",
            stderr: "",
            evidence: None,
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
    .unwrap();

    let receipts = receipts_for_plan_with_lock(
        &ctx.state_file("receipts.jsonl"),
        "plan_fallback",
        50,
        |_| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
    )
    .unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].plan_id.as_deref(), Some("plan_fallback"));
}

#[test]
fn jsonl_read_without_cache_lock_does_not_create_one() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let cache_lock = state_lock_path(&path);
    assert!(!cache_lock.exists());

    let items = read_jsonl::<Value>(&path).unwrap();

    assert_eq!(items, vec![json!({ "id": 1 })]);
    assert!(!cache_lock.exists());
}

#[cfg(unix)]
#[test]
fn jsonl_read_accepts_read_only_data_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();

    let items = read_jsonl::<Value>(&path).unwrap();

    assert_eq!(items, vec![json!({ "id": 1 })]);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o444
    );
}

#[test]
fn unlocked_jsonl_snapshot_reports_stable_invalid_unterminated_tail() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n{\"id\":").unwrap();

    let error = read_jsonl_with_data_lock::<Value>(&path, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "test lock seam",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("record 2"));
}

#[test]
fn unlocked_jsonl_snapshot_accepts_partial_then_complete_samples() {
    use std::collections::VecDeque;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"seed").unwrap();
    let mut snapshots = VecDeque::from([
        b"{\"id\":1}\n{\"id\":".to_vec(),
        b"{\"id\":1}\n{\"id\":2}\n".to_vec(),
    ]);

    let items = read_jsonl_with_io::<Value>(
        &path,
        |_| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
        |_| Ok(snapshots.pop_front().unwrap()),
    )
    .unwrap();

    assert_eq!(items, vec![json!({ "id": 1 }), json!({ "id": 2 })]);
    assert!(snapshots.is_empty());
}

#[test]
fn unlocked_jsonl_snapshot_returns_prefix_after_three_changing_tails() {
    use std::collections::VecDeque;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"seed").unwrap();
    let mut snapshots = VecDeque::from([
        b"{\"id\":1}\n{".to_vec(),
        b"{\"id\":1}\n{\"i".to_vec(),
        b"{\"id\":1}\n{\"id\":".to_vec(),
    ]);

    let items = read_jsonl_with_io::<Value>(
        &path,
        |_| Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
        |_| Ok(snapshots.pop_front().unwrap()),
    )
    .unwrap();

    assert_eq!(items, vec![json!({ "id": 1 })]);
    assert!(snapshots.is_empty());
}

#[test]
fn jsonl_read_retries_interrupted_data_lock() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let mut attempts = 0;

    let items = read_jsonl_with_io::<Value>(
        &path,
        |file| {
            attempts += 1;
            if attempts == 1 {
                Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            } else {
                FileExt::lock_shared(file)
            }
        },
        |_| panic!("locked read must use the original handle"),
    )
    .unwrap();

    assert_eq!(attempts, 2);
    assert_eq!(items, vec![json!({ "id": 1 })]);
}

#[test]
fn jsonl_read_propagates_nonunsupported_data_lock_errors() {
    for kind in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::WouldBlock,
        std::io::ErrorKind::Other,
    ] {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        fs::write(&path, b"{\"id\":1}\n").unwrap();

        let error = read_jsonl_with_io::<Value>(
            &path,
            |_| Err(std::io::Error::from(kind)),
            |_| panic!("non-unsupported lock error must not enter fallback"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("Failed to shared-lock"));
    }
}

#[test]
fn unlocked_jsonl_snapshot_rejects_malformed_terminated_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\nnot-json\n{\"id\":").unwrap();

    let error = read_jsonl_with_data_lock::<Value>(&path, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "test lock seam",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("record 2"));
}

#[test]
fn unlocked_jsonl_snapshot_keeps_valid_unterminated_final_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n{\"id\":2}").unwrap();

    let items = read_jsonl_with_data_lock::<Value>(&path, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "test lock seam",
        ))
    })
    .unwrap();

    assert_eq!(items, vec![json!({ "id": 1 }), json!({ "id": 2 })]);
}

#[test]
fn locked_jsonl_read_rejects_invalid_unterminated_final_record() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n{\"id\":").unwrap();

    let error = read_jsonl::<Value>(&path).unwrap_err();

    assert!(error.to_string().contains("record 2"));
}

#[test]
fn jsonl_read_waits_for_atomic_rewrite_and_observes_replacement() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    append_jsonl(&path, &json!({ "id": "before" })).unwrap();
    let writer_path = path.clone();
    let reader_path = path;
    let (rewritten_tx, rewritten_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            with_jsonl_write_lock(&writer_path, |guard| {
                write_jsonl_locked(guard, &writer_path, &[json!({ "id": "after" })])?;
                rewritten_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
        });
        rewritten_rx.recv().unwrap();
        scope.spawn(move || {
            let result = read_jsonl::<Value>(&reader_path).unwrap();
            read_tx.send(result).unwrap();
        });

        assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
        release_tx.send(()).unwrap();
        let values = read_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(values, vec![json!({ "id": "after" })]);
    });
}

#[test]
fn jsonl_read_waits_for_existing_data_file_lock() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();
    let reader_path = path;
    let (started_tx, started_rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            started_tx.send(()).unwrap();
            read_tx.send(read_jsonl::<Value>(&reader_path)).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
        FileExt::unlock(&lock).unwrap();
        let values = read_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(values, vec![json!({ "id": 1 })]);
    });
}

#[test]
fn cancellable_jsonl_read_stops_while_data_file_lock_is_held() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    fs::write(&path, b"{\"id\":1}\n").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let reader_path = path;
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let (started_tx, started_rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = read_jsonl_with_cancellation::<Value>(&reader_path, &|| {
            reader_cancelled.load(Ordering::SeqCst)
        });
        read_tx.send(result).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let result = match read_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            FileExt::unlock(&lock).unwrap();
            reader.join().unwrap();
            panic!("cancellable read stayed blocked on the data lock: {error}");
        }
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        "status collection was cancelled"
    );
    FileExt::unlock(&lock).unwrap();
    reader.join().unwrap();
}

#[test]
fn cancellable_jsonl_read_checks_between_records() {
    use std::cell::Cell;

    let temp = tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let records = (0..100)
        .map(|id| format!("{{\"id\":{id}}}\n"))
        .collect::<String>();
    fs::write(&path, records).unwrap();
    let checks = Cell::new(0);

    let error = read_jsonl_with_cancellation::<Value>(&path, &|| {
        let current = checks.get();
        checks.set(current + 1);
        current >= 12
    })
    .unwrap_err();

    assert_eq!(error.to_string(), "status collection was cancelled");
    assert!(checks.get() > 12);
}

#[test]
fn session_summary_includes_open_plans() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::open(
            "1".into(),
            "plan_1".into(),
            1,
            "Example".into(),
            Some(".agent/plans/plan_1.md".into()),
        ),
    )
    .unwrap();

    let summary = build_summary(&ctx).unwrap();
    assert_eq!(summary["open_plans"][0]["plan_id"], "plan_1");
}

#[test]
fn session_summary_reference_discards_an_in_memory_nested_snapshot() {
    let event = SessionEvent::start(
        "event".into(),
        "session".into(),
        1,
        json!({
            "recent_sessions": [{
                "event": "start",
                "summary": { "must_not_survive": true },
            }],
        }),
    );

    let reference = serde_json::to_value(event.into_summary_reference()).unwrap();

    assert_eq!(reference["event"], "start");
    assert_eq!(reference["session_id"], "session");
    assert!(reference["summary"].is_null());
}

#[test]
fn recursive_legacy_session_summaries_stay_readable_and_append_shallow_history() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let sessions_path = ctx.state_file("sessions.jsonl");

    let mut nested_summary = "null".to_string();
    for index in 0..48 {
        nested_summary = format!(
            r#"{{"recent_sessions":[{{"id":"nested-{index}","session_id":"nested-{index}","event":"start","timestamp_ms":{index},"outcome":null,"summary":{nested_summary}}}]}}"#
        );
    }
    let legacy_record = format!(
        r#"{{"id":"legacy-event","session_id":"legacy-session","event":"start","timestamp_ms":1,"outcome":null,"summary":{nested_summary}}}
"#
    );
    fs::write(&sessions_path, legacy_record.as_bytes()).unwrap();
    let original = fs::read(&sessions_path).unwrap();

    let status = state_summary(&ctx).unwrap();
    assert_eq!(status["counts"]["sessions"], 1);
    let summary = build_summary(&ctx).unwrap();
    assert_eq!(
        summary["recent_sessions"][0]["session_id"],
        "legacy-session"
    );
    assert!(summary["recent_sessions"][0]["summary"].is_null());
    let streams = state_streams(&ctx, 10).unwrap();
    assert_eq!(streams.session_events.len(), 1);
    assert_eq!(streams.session_events[0].event, "start");
    assert_eq!(streams.session_events[0].session_id, "legacy-session");
    assert_eq!(fs::read(&sessions_path).unwrap(), original);

    let started = session_start(&ctx).unwrap();
    assert_eq!(
        started["summary"]["recent_sessions"][0]["session_id"],
        "legacy-session"
    );
    assert!(started["summary"]["recent_sessions"][0]["summary"].is_null());

    let contents = fs::read_to_string(&sessions_path).unwrap();
    assert!(contents.as_bytes().starts_with(&original));
    let records = contents.lines().collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    let appended: Value = serde_json::from_str(records[1]).unwrap();
    assert!(appended["summary"]["recent_sessions"][0]["summary"].is_null());
    assert!(records[1].len() < 8 * 1024);
    assert_eq!(state_summary(&ctx).unwrap()["counts"]["sessions"], 2);
}

#[test]
fn ignored_legacy_session_summary_is_still_json_validated() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("sessions.jsonl");
    fs::write(
        &path,
        r#"{"id":"broken","session_id":"broken","event":"start","timestamp_ms":1,"summary":{"nested":[1,]}}
"#,
    )
    .unwrap();

    let error = read_jsonl::<SessionEvent>(&path).unwrap_err().to_string();

    assert!(error.contains("Failed to parse JSONL record 1"));
}

#[test]
fn repeated_session_snapshots_have_bounded_depth_and_size() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let sessions_path = ctx.state_file("sessions.jsonl");

    for index in 0..150 {
        let summary = build_summary(&ctx).unwrap();
        append_jsonl(
            &sessions_path,
            &SessionEvent::start(
                format!("event-{index}"),
                format!("session-{index}"),
                index,
                summary,
            ),
        )
        .unwrap();
        append_jsonl(
            &sessions_path,
            &SessionEvent::end(
                format!("end-{index}"),
                format!("session-{index}"),
                index,
                Some("done".into()),
            ),
        )
        .unwrap();
    }

    let contents = fs::read_to_string(&sessions_path).unwrap();
    let start_records = contents
        .lines()
        .filter_map(|record| {
            let value = serde_json::from_str::<Value>(record).unwrap();
            (value["event"] == "start").then_some((record.len(), value))
        })
        .collect::<Vec<_>>();
    assert_eq!(start_records.len(), 150);
    assert!(start_records.iter().all(|(len, _)| *len < 8 * 1024));
    assert!(start_records.iter().skip(1).all(|(_, event)| {
        event["summary"]["recent_sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|recent| recent["summary"].is_null())
    }));
    assert_eq!(
        read_jsonl::<SessionEvent>(&sessions_path).unwrap().len(),
        300
    );
}

#[test]
fn legacy_unknown_plan_events_stay_readable() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("plans.jsonl");
    fs::write(
        &path,
        r#"{"id":"1","plan_id":"plan_1","event":"pause","timestamp_ms":1}
"#,
    )
    .unwrap();

    let events = read_jsonl::<PlanEvent>(&path).unwrap();

    assert_eq!(events.len(), 1);
    assert!(super::plans::open_plans(&events).is_empty());
}

#[test]
fn ensure_plan_exists_requires_open_event_but_allows_closed_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();

    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::append("1".into(), "plan_1".into(), 1, None),
    )
    .unwrap();

    let error = ensure_plan_exists(&ctx, "plan_1").unwrap_err().to_string();
    assert!(error.contains("Plan not found: plan_1"));

    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::open("2".into(), "plan_1".into(), 2, "Example".into(), None),
    )
    .unwrap();
    append_jsonl(
        &ctx.state_file("plans.jsonl"),
        &PlanEvent::close("3".into(), "plan_1".into(), 3, Some("done".into())),
    )
    .unwrap();

    ensure_plan_exists(&ctx, "plan_1").unwrap();
}

#[test]
fn truncate_handles_multibyte_boundaries() {
    let value = format!("{}{}", "a".repeat(3999), "é");
    let truncated = truncate(&value);

    assert!(truncated.ends_with('…'));
    assert!(truncated.starts_with(&"a".repeat(3999)));
    assert_eq!(truncated.chars().last(), Some('…'));
}

#[test]
fn plans_append_serializes_concurrent_writers() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Concurrent plan".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();

    let ctx_a = ctx.clone();
    let ctx_b = ctx.clone();
    let plan_id = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))
        .unwrap()
        .into_iter()
        .find(PlanEvent::is_open)
        .unwrap()
        .plan_id()
        .to_string();

    let plan_id_a = plan_id.clone();
    let plan_id_b = plan_id.clone();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            plans_append(
                &ctx_a,
                PlanAppendRequest {
                    plan_id: plan_id_a,
                    body: Some("First append".into()),
                    body_file: None,
                },
            )
            .unwrap();
        });
        scope.spawn(|| {
            plans_append(
                &ctx_b,
                PlanAppendRequest {
                    plan_id: plan_id_b,
                    body: Some("Second append".into()),
                    body_file: None,
                },
            )
            .unwrap();
        });
    });

    let body = fs::read_to_string(ctx.plan_body_path(&plan_id)).unwrap();
    assert!(body.contains("Initial body"));
    assert!(body.contains("First append"));
    assert!(body.contains("Second append"));
}

#[test]
fn plans_close_rejects_unknown_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: "plan_missing".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
}

#[test]
fn plans_close_rejects_already_closed_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Close once".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();

    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done again".into()),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(&format!("Plan is already closed: {plan_id}")));
}

#[test]
fn plans_append_rejects_closed_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Append after close".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = plans_append(
        &ctx,
        PlanAppendRequest {
            plan_id: plan_id.clone(),
            body: Some("late append".into()),
            body_file: None,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(&format!("Plan is already closed: {plan_id}")));
}

#[test]
fn structured_work_keeps_legacy_state_receipt_tool_names() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Receipt compatibility".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    plans_append(
        &ctx,
        PlanAppendRequest {
            plan_id: plan_id.clone(),
            body: Some("Append body".into()),
            body_file: None,
        },
    )
    .unwrap();
    decisions_add(
        &ctx,
        DecisionAddRequest {
            title: "Decision".into(),
            selected_option: "Keep compatibility".into(),
            rationale: "Receipt filters depend on historical tool names.".into(),
            alternatives: vec!["Rename receipts".into()],
            plan_id: Some(plan_id.clone()),
        },
    )
    .unwrap();
    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id,
            resolution: Some("done".into()),
        },
    )
    .unwrap();
    session_end(
        &ctx,
        SessionEndRequest {
            session_id: None,
            outcome: Some("done".into()),
        },
    )
    .unwrap();

    let tool_names = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .into_iter()
        .map(|receipt| receipt.tool_name)
        .collect::<Vec<_>>();

    assert!(tool_names.contains(&tool::SESSION_START.to_string()));
    assert!(tool_names.contains(&tool::PLANS_OPEN.to_string()));
    assert!(tool_names.contains(&tool::PLANS_APPEND.to_string()));
    assert!(tool_names.contains(&tool::DECISIONS_ADD.to_string()));
    assert!(tool_names.contains(&tool::PLANS_CLOSE.to_string()));
    assert!(tool_names.contains(&tool::SESSION_END.to_string()));
}

#[test]
fn receipts_list_is_read_only() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let before = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_list(&ctx, receipt_list_filter()).unwrap();

    let after = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(before.len(), after.len());
    assert!(output.get("receipt_id").is_none());
}

#[test]
fn receipts_list_filters_by_tool_and_failure_and_adds_diff_summary() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    append_jsonl(
        &ctx.state_file("receipts.jsonl"),
        &receipt_record(
            "receipt_failed",
            tool::TEST,
            1,
            DiffStat {
                files: 1,
                insertions: 2,
                deletions: 3,
            },
        ),
    )
    .unwrap();
    append_jsonl(
        &ctx.state_file("receipts.jsonl"),
        &receipt_record("receipt_success", tool::CLIPPY, 0, DiffStat::default()),
    )
    .unwrap();

    let output = receipts_list(
        &ctx,
        ReceiptListFilter {
            tool_name: Some(tool::TEST.into()),
            failed_only: true,
            ..receipt_list_filter()
        },
    )
    .unwrap();
    let receipts = output["receipts"].as_array().unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["id"], "receipt_failed");
    assert_eq!(receipts[0]["diff_summary"], "1 file, +2 -3");
}

fn receipt_list_filter() -> ReceiptListFilter {
    ReceiptListFilter {
        session_id: None,
        plan_id: None,
        tool_name: None,
        failed_only: false,
        limit: 20,
    }
}

fn receipt_record(
    id: &str,
    tool_name: &str,
    exit_status: i32,
    diff_stat: DiffStat,
) -> ReceiptRecord {
    ReceiptRecord {
        id: id.into(),
        session_id: Some("session_1".into()),
        plan_id: Some("plan_1".into()),
        tool_name: tool_name.into(),
        args: json!({}),
        invoked_command_key: None,
        started_at_ms: 1,
        ended_at_ms: 2,
        exit_status,
        stdout_preview: String::new(),
        stderr_preview: String::new(),
        evidence: None,
        changed_paths: Vec::new(),
        diff_stat,
        git_status_error: None,
        git_diff_stat_error: None,
        worktree_fingerprint: None,
        worktree_fingerprint_error: None,
    }
}

#[test]
fn state_summary_is_read_only_and_counts_state_records() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let before = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();

    let output = state_summary(&ctx).unwrap();

    let after = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(before.len(), after.len());
    assert!(output.get("receipt_id").is_none());
    assert_eq!(output["ok"], true);
    assert_eq!(output["counts"]["sessions"], 1);
    assert_eq!(output["counts"]["plans"], 0);
    assert_eq!(output["counts"]["receipts"], 1);
    assert_eq!(output["counts"]["failed_receipts"], 0);
    assert_eq!(
        output["recent_receipts"][0]["tool_name"],
        tool::SESSION_START
    );
}

#[test]
fn cancellable_state_summary_stops_during_stream_collection() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    fs::create_dir_all(ctx.state_dir()).unwrap();
    let plans_path = ctx.state_file("plans.jsonl");
    fs::write(&plans_path, b"").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&plans_path)
        .unwrap();
    FileExt::lock_exclusive(&lock).unwrap();

    let reader_ctx = ctx;
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader_cancelled = Arc::clone(&cancelled);
    let (started_tx, started_rx) = mpsc::channel();
    let (summary_tx, summary_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = super::sessions::state_summary_with_cancellation(&reader_ctx, &|| {
            reader_cancelled.load(Ordering::SeqCst)
        });
        summary_tx.send(result).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(summary_rx.recv_timeout(Duration::from_millis(100)).is_err());
    cancelled.store(true, Ordering::SeqCst);
    let result = match summary_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            FileExt::unlock(&lock).unwrap();
            reader.join().unwrap();
            panic!("state summary stayed blocked on a state stream: {error}");
        }
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        "status collection was cancelled"
    );
    FileExt::unlock(&lock).unwrap();
    reader.join().unwrap();
}

#[test]
fn state_summary_on_uninitialized_repo_creates_nothing() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".agent/.cache").exists());

    let output = state_summary(&ctx).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["counts"]["sessions"], 0);
    assert_eq!(output["counts"]["receipts"], 0);
    assert!(!ctx.state_dir().exists());
    assert!(!temp.path().join(".agent/.cache").exists());
    assert!(!temp.path().join(".agent/plans").exists());
}

#[cfg(unix)]
#[test]
fn state_summary_reads_existing_read_only_state() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    session_start(&ctx).unwrap();
    let state_dir = ctx.state_dir();
    let cache_dir = temp.path().join(".agent/.cache");
    let lock_dir = cache_dir.join("state-locks");
    for path in [
        ctx.state_file("sessions.jsonl"),
        ctx.state_file("receipts.jsonl"),
        ctx.current_session_path(),
        lock_dir.join("sessions.jsonl.lock"),
        lock_dir.join("receipts.jsonl.lock"),
    ] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o444)).unwrap();
    }
    for path in [&state_dir, &lock_dir, &cache_dir] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }

    let output = state_summary(&ctx).unwrap();

    assert_eq!(output["counts"]["sessions"], 1);
    assert_eq!(output["counts"]["receipts"], 1);
    for path in [&cache_dir, &lock_dir, &state_dir] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn receipts_archive_moves_old_unprotected_receipts() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_receipt = receipt_record("receipt_old", tool::CLIPPY, 0, DiffStat::default());
    old_receipt.ended_at_ms = 10;
    let mut new_receipt = receipt_record("receipt_new", tool::CLIPPY, 0, DiffStat::default());
    new_receipt.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_receipt).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &new_receipt).unwrap();

    let output = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(output["receipts_archived"], 1);
    assert_eq!(output["receipts_retained"], 1);
    let retained = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].id, "receipt_new");
    let archive_path = output["archive_path"].as_str().unwrap();
    let archived = read_jsonl::<ReceiptRecord>(Path::new(archive_path)).unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, "receipt_old");
}

#[test]
fn receipts_archive_preserves_latest_gate_evidence_and_supporting_receipts() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_direct = receipt_record("receipt_direct", tool::TEST, 0, DiffStat::default());
    old_direct.ended_at_ms = 10;
    let mut old_batch = receipt_record("receipt_batch", tool::WORK_CHECK, 0, DiffStat::default());
    old_batch.ended_at_ms = 20;
    old_batch.args = json!({
        "plan_id": "plan_1",
        "tools": [tool::TEST],
        "receipt_ids": ["receipt_direct"],
    });
    let mut old_review =
        receipt_record("receipt_review", tool::WORK_REVIEW, 1, DiffStat::default());
    old_review.ended_at_ms = 30;
    old_review.args = json!({
        "plan_id": "plan_1",
        "gate_id": "rust-review",
    });
    let mut unrelated_old = receipt_record(
        "receipt_unrelated_old",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated_old.plan_id = Some("plan_2".into());
    unrelated_old.ended_at_ms = 40;
    let mut unrelated_new = receipt_record(
        "receipt_unrelated_new",
        tool::CLIPPY,
        0,
        DiffStat::default(),
    );
    unrelated_new.plan_id = Some("plan_2".into());
    unrelated_new.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_direct).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_batch).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_review).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated_old).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &unrelated_new).unwrap();

    let output = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();

    assert_eq!(output["receipts_archived"], 1);
    assert_eq!(output["protected_receipts_retained"], 3);
    let retained = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .into_iter()
        .map(|receipt| receipt.id)
        .collect::<Vec<_>>();
    assert!(retained.contains(&"receipt_direct".into()));
    assert!(retained.contains(&"receipt_batch".into()));
    assert!(retained.contains(&"receipt_review".into()));
    assert!(!retained.contains(&"receipt_unrelated_old".into()));
    assert!(retained.contains(&"receipt_unrelated_new".into()));
}

#[test]
fn receipts_archive_dry_run_does_not_rewrite_state() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    let mut old_receipt = receipt_record("receipt_old", tool::CLIPPY, 0, DiffStat::default());
    old_receipt.ended_at_ms = 10;
    let mut new_receipt = receipt_record("receipt_new", tool::CLIPPY, 0, DiffStat::default());
    new_receipt.ended_at_ms = 2_000;
    append_jsonl(&ctx.state_file("receipts.jsonl"), &old_receipt).unwrap();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &new_receipt).unwrap();
    let before = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "1970-01-02".into(),
            dry_run: true,
        },
    )
    .unwrap();

    assert_eq!(output["receipts_archived"], 1);
    assert_eq!(output["protected_receipts_retained"], 1);
    assert!(output["archive_path"].is_null());
    assert_eq!(
        fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap(),
        before
    );
    assert!(!ctx.state_dir().join("archive").exists());
}

#[test]
fn receipts_archive_rejects_malformed_before_cutoff() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = receipts_archive(
        &ctx,
        StateArchiveRequest {
            before: "2026-02-31".into(),
            dry_run: true,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Invalid --before date"));
}

#[test]
fn state_tool_receipts_skip_git_metadata_collection() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();

    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.tool_name == tool::SESSION_START)
        .unwrap();
    assert_eq!(receipt.args["operation"], "session_start");
    assert!(receipt.changed_paths.is_empty());
    assert_eq!(receipt.diff_stat.files, 0);
    assert!(receipt.git_status_error.is_none());
    assert!(receipt.git_diff_stat_error.is_none());
}
