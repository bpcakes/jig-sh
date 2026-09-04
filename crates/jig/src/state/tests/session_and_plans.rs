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
            base: None,
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

    let body = fs::read_to_string(plan_body_path(&ctx, &plan_id).unwrap()).unwrap();
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
            base: None,
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
fn plans_close_rejects_an_active_linked_repository_run() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Run before close".into(),
            body: Some("Initial body".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    let run_plan = jig_contract::RunPlan::new(
        "run-plan_empty",
        "sha256:config",
        jig_contract::SourceIdentity::new(Some("abc".into()), "sha256:worktree"),
        Vec::new(),
        Vec::new(),
    );
    let (run, lease) = start_run(&ctx, run_plan, Some(plan_id.clone())).unwrap();

    let error = plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("too early".into()),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("active linked repository runs"), "{error}");
    assert_eq!(plan_status(&ctx, &plan_id).unwrap(), Some(PlanStatus::Open));

    complete_run(
        &ctx,
        &run.result.run_id,
        jig_contract::RunConclusion::Success,
    )
    .unwrap();
    drop(lease);
    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id,
            resolution: Some("done".into()),
        },
    )
    .unwrap();
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
            base: None,
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
fn plans_append_requires_progress_text_without_mutating_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Append input".into(),
            body: Some("Initial body".into()),
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap();
    let plan_path = plan_body_path(&ctx, plan_id).unwrap();
    let body_before = fs::read_to_string(&plan_path).unwrap();
    let state_before = state_summary(&ctx).unwrap();
    let empty_body_file = temp.path().join("empty-progress.md");
    fs::write(&empty_body_file, "\n  \n").unwrap();

    for request in [
        PlanAppendRequest {
            plan_id: plan_id.into(),
            body: None,
            body_file: None,
        },
        PlanAppendRequest {
            plan_id: plan_id.into(),
            body: Some(String::new()),
            body_file: None,
        },
        PlanAppendRequest {
            plan_id: plan_id.into(),
            body: Some(" \n\t ".into()),
            body_file: None,
        },
        PlanAppendRequest {
            plan_id: plan_id.into(),
            body: None,
            body_file: Some(empty_body_file),
        },
    ] {
        let error = plans_append(&ctx, request).unwrap_err().to_string();
        assert!(error.contains("Progress text"));
    }
    assert_eq!(fs::read_to_string(plan_path).unwrap(), body_before);
    assert_eq!(state_summary(&ctx).unwrap(), state_before);
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
            base: None,
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
