use super::*;

#[test]
fn work_timeline_and_health_render_typed_parity_fields() {
    let mut app = app_with_local(Tab::Work);
    let work = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &work,
        &[
            "ExampleProject",
            "plan_example",
            "Example plan",
            "plan_closed",
            "completed",
            "2 sessions / 1 open / 1 decisions",
            "8.0s",
            "Gates: pass",
            "Runtime: 0.3.0",
            "contract 8",
            "/example/source",
        ],
    );

    app.select_tab(Tab::Timeline);
    let timeline = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &timeline,
        &[
            "receipt_failed",
            "Tool: jig.test",
            "Exit: 1",
            "1 file changed",
        ],
    );

    app.select_tab(Tab::Health);
    app.move_selection(2);
    let health = normalized(&render_text(&app, 120, 36));
    assert!(health.contains("Recent failures"));
    assert!(health.contains("Check health"));
    assert!(health.contains("Loop collection"));
    assert!(health.contains("limit 1000 workflows"));
}

#[test]
fn repository_open_plans_failures_and_tool_times_keep_exact_semantics() {
    let mut snapshot = scenarios::recorder_snapshot();
    let mut second_plan = snapshot.open_plans[0].clone();
    second_plan.plan_id = "plan_second".to_string();
    second_plan.title = "Second open plan".to_string();
    snapshot.open_plans.push(second_plan);

    let mut newest = snapshot.failures[0].clone();
    newest.id = "failure-newest".to_string();
    newest.ended_at_ms = Some(scenarios::OBSERVED_AT_MS);
    newest.tool_name = "jig.clippy".to_string();
    newest.exit_status = 7;
    newest.plan_id = None;
    let mut oldest = newest.clone();
    oldest.id = "failure-oldest".to_string();
    oldest.ended_at_ms = Some(scenarios::OBSERVED_AT_MS - 10_000);
    oldest.plan_id = Some("plan_second".to_string());
    snapshot.failures = vec![oldest, newest];

    let mut app = App::new(Tab::Work);
    app.recorder.data = Some(snapshot.into());
    let local = app.recorder.data.as_ref().unwrap();
    assert_eq!(local.repo.name, "ExampleProject");
    assert_eq!(local.repo.default_branch, "main");
    assert_eq!(local.harness.runtime_version, "0.3.0");
    assert_eq!(local.harness.contract_version, 8);
    assert_ne!(local.tools[0].last_ended_at, "—");
    assert_eq!(local.failures[0].id, "failure-newest");
    assert_eq!(local.failures[0].tool, "jig.clippy");
    assert_eq!(local.failures[0].exit_status, 7);
    assert!(local.failures[0].display_plan_id.is_none());
    assert_eq!(local.failures[1].id, "failure-oldest");
    assert_eq!(
        local.failures[1].display_plan_id.as_deref(),
        Some("plan_second")
    );
    assert_ne!(local.failures[0].ended_at, local.failures[1].ended_at);

    for (index, expected) in [(0, "plan_example"), (1, "plan_second")] {
        app.work_index = index;
        assert_eq!(app.selected_work().unwrap().plan_id, expected);
    }
}

#[test]
fn local_views_remain_reachable_at_compact_and_micro_sizes() {
    for tab in [Tab::Work, Tab::Timeline, Tab::Health] {
        let app = app_with_local(tab);
        let compact = render_text(&app, 60, 15);
        assert!(compact.contains("Enter"), "{tab:?}:\n{compact}");
        let micro = render_text(&app, 39, 11);
        assert!(micro.contains("Jig"), "{tab:?}:\n{micro}");
        assert!(
            micro.contains("Selected")
                || micro.contains("receipt")
                || micro.contains("Recent failures")
        );
    }
}

#[test]
fn timeline_filters_cover_every_kind_and_preserve_raw_identity() {
    let mut snapshot = scenarios::recorder_snapshot();
    snapshot.timeline.extend([
        TimelineRow::Plan(PlanTimelineRow {
            stable_identity: "plan:event:1".to_string(),
            timestamp_ms: Some(scenarios::OBSERVED_AT_MS - 2_000),
            id: "plan-event".to_string(),
            event: "closed".to_string(),
            plan_id: "plan_closed".to_string(),
            title: Some("Closed example".to_string()),
            resolution: Some("completed".to_string()),
        }),
        TimelineRow::Session(SessionTimelineRow {
            stable_identity: "session:event:1".to_string(),
            timestamp_ms: Some(scenarios::OBSERVED_AT_MS - 3_000),
            id: "session-event".to_string(),
            event: "finished".to_string(),
            session_id: "session_example".to_string(),
            outcome: Some("success".to_string()),
        }),
        TimelineRow::Decision(DecisionTimelineRow {
            stable_identity: "decision:\u{1b}[31mraw".to_string(),
            timestamp_ms: Some(scenarios::OBSERVED_AT_MS - 4_000),
            id: "decision-1".to_string(),
            plan_id: None,
            title: "Choose safely".to_string(),
            selected_option: "A".to_string(),
            rationale: BoundedText::for_limit(
                "because",
                Some(7),
                LimitId::TimelineDecisionRationaleChars,
            )
            .unwrap(),
        }),
    ]);
    let mut app = App::new(Tab::Timeline);
    app.recorder.data = Some(snapshot.into());

    let expected = [
        (TimelineFilter::All, 4),
        (TimelineFilter::Receipts, 1),
        (TimelineFilter::Failures, 1),
        (TimelineFilter::Plans, 1),
        (TimelineFilter::Sessions, 1),
        (TimelineFilter::Decisions, 1),
    ];
    for (filter, count) in expected {
        assert_eq!(app.timeline_filter, filter);
        assert_eq!(app.timeline_rows().len(), count);
        app.cycle_timeline_filter(false);
    }
    assert_eq!(app.timeline_filter, TimelineFilter::All);

    app.cycle_timeline_filter(true);
    assert_eq!(app.timeline_filter, TimelineFilter::Decisions);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "decision:\u{1b}[31mraw"
    );
    let rendered = render_text(&app, 120, 36);
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn mixed_timeline_is_newest_first_and_every_plan_row_opens_its_raw_id() {
    let raw_plan = "plan $(raw) with spaces";
    let receipt = TimelineRow::Receipt(ReceiptTimelineRow {
        stable_identity: "receipt:newest".to_string(),
        timestamp_ms: Some(400),
        id: "receipt-newest".to_string(),
        tool_name: "jig.test".to_string(),
        invoked_command_key: Some("test".to_string()),
        plan_id: Some(raw_plan.to_string()),
        session_id: Some("session-one".to_string()),
        exit_status: 9,
        started_at_ms: Some(350),
        ended_at_ms: Some(400),
        duration_ms: Some(50),
        diff_summary: Some("2 files changed".to_string()),
        changed_path_count: Some(2),
        stderr_preview: None,
    });
    let plan = TimelineRow::Plan(PlanTimelineRow {
        stable_identity: "plan:second".to_string(),
        timestamp_ms: Some(300),
        id: "plan-event".to_string(),
        event: "closed".to_string(),
        plan_id: raw_plan.to_string(),
        title: Some("Raw plan".to_string()),
        resolution: Some("completed".to_string()),
    });
    let session = TimelineRow::Session(SessionTimelineRow {
        stable_identity: "session:third".to_string(),
        timestamp_ms: Some(200),
        id: "session-event".to_string(),
        event: "finished".to_string(),
        session_id: "session-one".to_string(),
        outcome: Some("success".to_string()),
    });
    let decision = TimelineRow::Decision(DecisionTimelineRow {
        stable_identity: "decision:oldest".to_string(),
        timestamp_ms: Some(100),
        id: "decision-one".to_string(),
        plan_id: Some(raw_plan.to_string()),
        title: "Choose implementation".to_string(),
        selected_option: "Option A".to_string(),
        rationale: BoundedText::for_limit(
            "Deterministic behavior",
            Some(22),
            LimitId::TimelineDecisionRationaleChars,
        )
        .unwrap(),
    });
    let mut snapshot = scenarios::recorder_snapshot();
    snapshot.timeline = vec![decision, session, plan, receipt];
    let mut app = App::new(Tab::Timeline);
    app.recorder.data = Some(snapshot.into());

    let local = app.recorder.data.as_ref().unwrap();
    assert_eq!(
        local
            .timeline
            .iter()
            .map(|row| row.identity.as_str())
            .collect::<Vec<_>>(),
        [
            "receipt:newest",
            "plan:second",
            "session:third",
            "decision:oldest"
        ]
    );
    assert_contains_all(
        &local.timeline[0].detail.lines.join(" "),
        &["Exit: 9", "Duration: 50ms", "Diff: 2 files changed"],
    );
    assert_contains_all(
        &local.timeline[1].detail.lines.join(" "),
        &["Event: closed", "Resolution: completed"],
    );
    assert_contains_all(
        &local.timeline[2].detail.lines.join(" "),
        &["Event: finished", "Outcome: success"],
    );
    assert_contains_all(
        &local.timeline[3].detail.lines.join(" "),
        &["Selected: Option A", "Deterministic behavior"],
    );

    for index in [0, 1, 3] {
        app.timeline_index = index;
        assert!(app.open_selected_detail());
        assert_eq!(app.take_plan_request().unwrap().1, raw_plan);
        app.close_detail();
    }
}

#[test]
fn plan_detail_preserves_sections_errors_and_inert_argv() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    assert_eq!(plan_id, "plan_example");
    assert_eq!(
        basis,
        crate::dashboard::PlanBasis::RecorderEpoch(crate::dashboard::RecorderEpochId::FIRST)
    );

    let mut snapshot = scenarios::plan_snapshot();
    let gates = snapshot.gates.as_mut().unwrap();
    let mut gate = gates.gates.items()[0].clone();
    gate.remediation.as_mut().unwrap().argv = vec![
        "scripts/jig".to_string(),
        "check".to_string(),
        "two words".to_string(),
        "$(unsafe)".to_string(),
        "a'b".to_string(),
    ];
    gates.gates = BoundedRows::for_limit(vec![gate], Some(1), LimitId::GateRows).unwrap();
    snapshot.errors.push(SnapshotError::new(
        CollectionDomain::Body,
        SnapshotErrorCode::BodyReadFailed,
        Some("plan_example".to_string()),
        "body read was partial",
    ));
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(snapshot)),
    );

    assert!(matches!(app.detail.base, Some(BaseDetail::Plan(_))));
    let summary = render_text(&app, 120, 36);
    assert!(summary.contains("basis epoch 1"));
    assert!(summary.contains("h/l horizontal"));
    assert!(!summary.contains("Enter opens"));
    app.cycle_detail_section(false);
    assert_eq!(app.detail.section, PlanSection::Body);
    let body = render_text(&app, 120, 36);
    assert!(body.contains("# Example plan"));
    assert!(body.contains("body read was partial"));
    app.cycle_detail_section(false);
    let gates = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &gates,
        &[
            "Overall: pass",
            "test [pass] jig.test · required true",
            "Freshness: fresh",
            "diff 1 file changed",
            "changed src/example.rs",
            "matched src/example.rs",
            "'two words'",
            "'$(unsafe)'",
            "'a'\"'\"'b'",
        ],
    );
}

#[test]
fn plan_detail_leaf_navigation_preserves_parent_state() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut snapshot = scenarios::plan_snapshot();
    snapshot.receipts[0].stderr_preview =
        BoundedText::for_limit("example stderr", Some(14), LimitId::ReceiptStderrChars).unwrap();
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(snapshot)),
    );
    for _ in 0..3 {
        app.cycle_detail_section(false);
    }
    assert_eq!(app.detail.section, PlanSection::Decisions);
    app.open_detail_leaf_or_close();
    assert!(app.detail.leaf.is_some());
    let decision = normalized(&render_text(&app, 80, 24));
    assert_contains_all(
        &decision,
        &["Selected: A", "Alternatives: A, B", "A is deterministic"],
    );
    app.move_detail_selection(3);
    assert_eq!(app.detail.leaf_scroll, 3);
    app.close_detail();
    assert!(app.detail.leaf.is_none());
    assert_eq!(app.detail.section, PlanSection::Decisions);
    assert_eq!(app.detail.section_scroll[PlanSection::Decisions.index()], 0);

    app.cycle_detail_section(false);
    assert_eq!(app.detail.section, PlanSection::Receipts);
    app.open_detail_leaf_or_close();
    let receipt = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &receipt,
        &[
            "example output",
            "example stderr",
            "src/example.rs",
            "1 file changed",
            "200ms",
        ],
    );
}

#[test]
fn bounded_plan_body_and_fifty_receipts_remain_reachable() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut snapshot = scenarios::plan_snapshot();
    snapshot.body = Some(
        BoundedText::for_limit(
            "b".repeat(LimitId::PlanBodyChars.ceiling()),
            Some(LimitId::PlanBodyChars.ceiling() + 7),
            LimitId::PlanBodyChars,
        )
        .unwrap(),
    );
    let receipt = snapshot.receipts[0].clone();
    snapshot.receipts = (0..50)
        .map(|index| {
            let mut receipt = receipt.clone();
            receipt.id = format!("receipt-{index:02}");
            receipt
        })
        .collect();
    snapshot.limits.plan_receipts.omitted = Some(3);
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(snapshot)),
    );

    app.cycle_detail_section(false);
    assert_eq!(app.detail.section, PlanSection::Body);
    let body = app.detail.plan().unwrap().body.as_ref().unwrap();
    assert_eq!(
        body.lines[0].chars().count(),
        LimitId::PlanBodyChars.ceiling()
    );
    assert_eq!(
        body.lines.last().unwrap(),
        "limit 20000 characters; 7 omitted"
    );

    for _ in 0..3 {
        app.cycle_detail_section(false);
    }
    assert_eq!(app.detail.section, PlanSection::Receipts);
    assert_eq!(app.detail.plan().unwrap().receipts.len(), 50);
    assert_eq!(app.detail.plan().unwrap().receipts_limit.omitted, Some(3));
    app.move_detail_selection(49);
    assert_eq!(app.detail.receipt_index, 49);
    app.open_detail_leaf_or_close();
    assert!(
        app.detail
            .leaf
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|line| line == "Receipt: receipt-49")
    );
}

#[test]
fn receipt_output_and_paths_keep_independent_bounds() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut snapshot = scenarios::plan_snapshot();
    snapshot.receipts[0].stdout_preview =
        BoundedText::for_limit("o".repeat(1_000), Some(1_003), LimitId::ReceiptStdoutChars)
            .unwrap();
    snapshot.receipts[0].stderr_preview =
        BoundedText::for_limit("e".repeat(1_000), Some(1_007), LimitId::ReceiptStderrChars)
            .unwrap();
    snapshot.receipts[0].changed_paths = BoundedRows::for_limit(
        (0..20)
            .map(|index| format!("src/file-{index}.rs"))
            .collect(),
        Some(23),
        LimitId::ReceiptChangedPaths,
    )
    .unwrap();
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(snapshot)),
    );
    for _ in 0..4 {
        app.cycle_detail_section(false);
    }
    app.open_detail_leaf_or_close();

    let receipt = app.detail.leaf.as_ref().unwrap().lines.join(" ");
    assert_contains_all(
        &receipt,
        &[
            "Stdout:",
            "limit 1000 characters; 3 omitted",
            "Stderr:",
            "limit 1000 characters; 7 omitted",
            "src/file-19.rs",
            "limit 20 paths; 3 omitted",
        ],
    );
}
