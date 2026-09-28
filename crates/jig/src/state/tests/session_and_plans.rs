use super::*;

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
fn repository_execution_lease_allows_readers_and_excludes_a_writer() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let first_reader =
        acquire_repository_execution_lease(&ctx, &[jig_contract::ActionEffect::ReadOnly]).unwrap();
    assert!(first_reader.permits(&[jig_contract::ActionEffect::ReadOnly]));
    assert!(!first_reader.permits(&[jig_contract::ActionEffect::Worktree]));
    let second_reader = acquire_repository_execution_lease(
        &ctx,
        &[
            jig_contract::ActionEffect::ReadOnly,
            jig_contract::ActionEffect::Process,
        ],
    )
    .unwrap();
    let root = temp.path().to_path_buf();
    let (attempting_tx, attempting_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            let ctx = RepoContext::load_from(&root).unwrap();
            attempting_tx.send(()).unwrap();
            let _writer =
                acquire_repository_execution_lease(&ctx, &[jig_contract::ActionEffect::Worktree])
                    .unwrap();
            acquired_tx.send(()).unwrap();
        });

        attempting_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            acquired_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "worktree execution acquired its writer lease while readers were active"
        );
        drop(first_reader);
        drop(second_reader);
        acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
}
