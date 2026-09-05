use crate::{
    dashboard::{
        BoundedRows, BoundedText, CollectionDomain, DecisionTimelineRow, LimitId, LoopStateError,
        PlanSnapshotResult, PlanTimelineRow, RecorderEpochId, ScheduledOccurrence,
        SessionTimelineRow, SnapshotError, SnapshotErrorCode, StatusRefresh, TimelineRow,
        scenarios,
    },
    terminal::model::{App, BaseDetail, PlanSection, Tab, TimelineFilter},
};

use super::{normalized, render_text};

fn app_with_local(tab: Tab) -> App {
    let mut app = App::new(tab);
    app.recorder.data = Some(scenarios::recorder_snapshot().into());
    app
}

fn assert_contains_all(rendered: &str, expected: &[&str]) {
    for value in expected {
        assert!(
            rendered.contains(value),
            "missing {value:?} from:\n{rendered}"
        );
    }
}

#[test]
fn work_timeline_and_health_render_typed_parity_fields() {
    let mut app = app_with_local(Tab::Work);
    let work = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &work,
        &[
            "plan_example",
            "Example plan",
            "plan_closed",
            "completed",
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
    assert!(gates.contains("'two words'"));
    assert!(gates.contains("'$(unsafe)'"));
    assert!(gates.contains("'a'\"'\"'b'"));
}

#[test]
fn plan_detail_leaf_navigation_preserves_parent_state() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(scenarios::plan_snapshot())),
    );
    for _ in 0..3 {
        app.cycle_detail_section(false);
    }
    assert_eq!(app.detail.section, PlanSection::Decisions);
    app.open_detail_leaf_or_close();
    assert!(app.detail.leaf.is_some());
    assert!(render_text(&app, 80, 24).contains("A is deterministic"));
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
            "src/example.rs",
            "1 file changed",
            "200ms",
        ],
    );
}

#[test]
fn standalone_timeline_and_loop_attention_details_are_reachable() {
    let mut recorder = scenarios::recorder_snapshot();
    recorder.timeline.insert(
        0,
        TimelineRow::Decision(DecisionTimelineRow {
            stable_identity: "decision:standalone".to_string(),
            timestamp_ms: Some(scenarios::OBSERVED_AT_MS),
            id: "decision-standalone".to_string(),
            plan_id: None,
            title: "Standalone choice".to_string(),
            selected_option: "safe".to_string(),
            rationale: BoundedText::for_limit(
                "bounded reason",
                Some(14),
                LimitId::TimelineDecisionRationaleChars,
            )
            .unwrap(),
        }),
    );
    let mut app = App::new(Tab::Timeline);
    app.recorder.data = Some(recorder.into());
    assert!(app.open_selected_detail());
    assert!(render_text(&app, 80, 24).contains("bounded reason"));
    app.close_detail();

    app.select_tab(Tab::Health);
    app.move_selection(6);
    assert!(app.open_selected_detail());
    let attention = normalized(&render_text(&app, 120, 36));
    assert!(attention.contains("Loop attention"));
    assert!(attention.contains("Recovery argv: scripts/jig loop clear-attempt"));
}

#[test]
fn producer_limits_and_partial_errors_are_visible_without_erasing_data() {
    let mut recorder = scenarios::partial_recorder_snapshot();
    recorder.limits.history.omitted = Some(3);
    recorder.limits.timeline.omitted = None;
    let mut app = App::new(Tab::Work);
    app.recorder.data = Some(recorder.into());
    let work = normalized(&render_text(&app, 120, 36));
    assert!(work.contains("3 omitted"));
    assert!(work.contains("example loop data is unavailable"));

    app.select_tab(Tab::Timeline);
    let timeline = normalized(&render_text(&app, 120, 36));
    assert!(timeline.contains("omitted count unknown"));
    assert!(timeline.contains("receipt_failed"));
}

#[test]
fn recorder_refresh_reconciles_selection_without_discarding_bounded_closed_detail() {
    let mut app = app_with_local(Tab::Work);
    app.move_selection(1);
    assert_eq!(app.selected_work().unwrap().plan_id, "plan_closed");
    assert!(app.open_selected_detail());
    let (basis, requested) = app.take_plan_request().unwrap();
    let mut detail = scenarios::plan_snapshot();
    detail.plan.plan_id = requested.clone();
    detail.plan.title = "Closed example".to_string();
    detail.plan.state = "closed".to_string();
    app.accept_plan_result(
        basis,
        &requested,
        PlanSnapshotResult::Found(Box::new(detail)),
    );

    let mut recorder = scenarios::recorder_snapshot();
    recorder.history.insert(0, recorder.history[0].clone());
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder,
    });
    assert_eq!(app.selected_work().unwrap().plan_id, "plan_closed");
    assert!(app.take_plan_request().is_none());

    let mut recorder = scenarios::recorder_snapshot();
    recorder.history.clear();
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder,
    });
    assert!(app.detail_is_open());
    assert_eq!(app.detail.plan().unwrap().raw_plan_id, "plan_closed");

    assert!(app.refresh_plan_detail());
    let (basis, requested) = app.take_plan_request().unwrap();
    app.accept_plan_result(basis, &requested, PlanSnapshotResult::NotFound);
    assert!(!app.detail_is_open());
    assert!(app.detail.notice.as_deref().unwrap().contains("no longer"));
}

#[test]
fn detail_errors_keep_last_successful_plan_visible() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(scenarios::plan_snapshot())),
    );
    app.detail.request_plan(plan_id.clone());
    app.accept_plan_error(&plan_id, "detail collection failed".to_string());

    assert!(app.detail.plan().is_some());
    assert_eq!(
        app.detail.error.as_deref(),
        Some("detail collection failed")
    );
    assert!(app.detail_is_open());
}

#[test]
fn multiline_text_and_session_labels_cross_the_terminal_boundary_safely() {
    let mut recorder = scenarios::recorder_snapshot();
    recorder.current_session_id = Some("session\u{1b}[31m\u{202e}raw".to_string());
    let mut app = App::new(Tab::Work);
    app.recorder.data = Some(recorder.into());
    let work = render_text(&app, 120, 36);
    assert!(!work.contains('\u{1b}'));
    assert!(!work.contains('\u{202e}'));

    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut plan = scenarios::plan_snapshot();
    let body = "first\r\nsecond\tcolumn\nthird\u{1b}[31m";
    plan.body = Some(
        BoundedText::for_limit(body, Some(body.chars().count()), LimitId::PlanBodyChars).unwrap(),
    );
    app.accept_plan_result(basis, &plan_id, PlanSnapshotResult::Found(Box::new(plan)));
    app.cycle_detail_section(false);
    let rendered = render_text(&app, 120, 36);
    assert!(rendered.lines().any(|line| line.contains("first")));
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("second    column"))
    );
    assert!(rendered.lines().any(|line| line.contains("third�[31m")));
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn detail_scroll_edges_clamp_and_open_leaf_survives_epoch_refresh() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut plan = scenarios::plan_snapshot();
    let body = "one\ntwo\nthree\nfour";
    plan.body = Some(
        BoundedText::for_limit(body, Some(body.chars().count()), LimitId::PlanBodyChars).unwrap(),
    );
    app.accept_plan_result(basis, &plan_id, PlanSnapshotResult::Found(Box::new(plan)));
    app.cycle_detail_section(false);
    app.move_detail_to_edge(true);
    assert_eq!(app.detail.section_scroll[PlanSection::Body.index()], 5);
    app.move_detail_selection(10);
    assert_eq!(app.detail.section_scroll[PlanSection::Body.index()], 5);
    app.move_detail_to_edge(false);
    assert_eq!(app.detail.section_scroll[PlanSection::Body.index()], 0);

    for _ in 0..2 {
        app.cycle_detail_section(false);
    }
    app.open_detail_leaf_or_close();
    assert!(app.detail.leaf.is_some());
    let mut recorder = scenarios::recorder_snapshot();
    recorder.epoch_id = RecorderEpochId::new(2).unwrap();
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder,
    });
    assert!(app.detail.leaf.is_some());
    let (basis, requested) = app.take_plan_request().unwrap();
    assert_eq!(
        basis,
        crate::dashboard::PlanBasis::RecorderEpoch(RecorderEpochId::new(2).unwrap())
    );
    let mut refreshed = scenarios::plan_snapshot();
    refreshed.basis_epoch = RecorderEpochId::new(2).unwrap();
    app.accept_plan_result(
        basis,
        &requested,
        PlanSnapshotResult::Found(Box::new(refreshed)),
    );
    assert!(app.detail.leaf.is_some());
}

#[test]
fn manual_occurrences_and_loop_error_selection_survive_unrelated_insertions() {
    let target = LoopStateError {
        kind: "read".to_string(),
        workflow_id: Some("workflow-target".to_string()),
        error: "target error".to_string(),
    };
    let mut recorder = scenarios::recorder_snapshot();
    let loops = recorder.loops.as_mut().unwrap();
    loops.scheduled_occurrences = BoundedRows::for_limit(
        vec![ScheduledOccurrence {
            occurrence_id: "manual-run".to_string(),
            workflow_id: "workflow-example".to_string(),
            scheduled_at_ms: 0,
            owner: "worker-example".to_string(),
            claim_expires_at_ms: 0,
            started_at_ms: scenarios::OBSERVED_AT_MS,
            uses_shared_checkout: Some(true),
            finished_at_ms: None,
            acknowledged_at_ms: None,
            status: "running".to_string(),
            worker_receipt_id: None,
            worktree: None,
            error: None,
        }],
        Some(1),
        LimitId::LoopScheduledOccurrences,
    )
    .unwrap();
    loops.state_error_count = 1;
    loops.state_errors = vec![target.clone()];
    let mut app = App::new(Tab::Health);
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder,
    });
    app.health_index = app
        .recorder
        .data
        .as_ref()
        .unwrap()
        .health
        .iter()
        .position(|item| item.primary.contains("read"))
        .unwrap();
    let identity = app.selected_health().unwrap().identity.clone();
    let local = app.recorder.data.as_ref().unwrap();
    let manual = local
        .health
        .iter()
        .find(|item| item.identity.contains("manual-run"))
        .unwrap();
    assert!(manual.secondary.starts_with("Manual ("));
    assert!(!manual.detail.lines.join(" ").contains("1970"));

    let mut recorder = scenarios::recorder_snapshot();
    let loops = recorder.loops.as_mut().unwrap();
    loops.state_error_count = 3;
    loops.state_errors = vec![
        LoopStateError {
            kind: "unrelated".to_string(),
            workflow_id: None,
            error: "another error".to_string(),
        },
        target.clone(),
        target,
    ];
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder,
    });
    assert_eq!(app.selected_health().unwrap().identity, identity);
    let error_ids = app
        .recorder
        .data
        .as_ref()
        .unwrap()
        .health
        .iter()
        .filter(|item| item.section == "Loop errors")
        .map(|item| &item.identity)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(error_ids.len(), 3);
}

#[test]
fn recorder_errors_render_their_sanitized_subject() {
    let mut recorder = scenarios::recorder_snapshot();
    recorder.errors.push(SnapshotError::new(
        CollectionDomain::Gates,
        SnapshotErrorCode::GateObservationFailed,
        Some("plan\u{1b}[31m-target".to_string()),
        "gate failed",
    ));
    let mut app = App::new(Tab::Work);
    app.recorder.data = Some(recorder.into());
    let rendered = render_text(&app, 120, 36);
    assert!(rendered.contains("plan�[31m-target"));
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn local_header_reports_default_branch_age_and_detached_state_on_every_local_tab() {
    let mut app = app_with_local(Tab::Work);
    for tab in [Tab::Work, Tab::Timeline, Tab::Health] {
        app.select_tab(tab);
        let rendered = render_text(&app, 120, 36);
        assert!(rendered.contains("default main"));
        assert!(rendered.contains("observed"));
    }
    let local = app.recorder.data.as_mut().unwrap();
    local.repo.branch = None;
    local.repo.detached = true;
    assert!(render_text(&app, 120, 36).contains("detached@"));
}

#[test]
fn compact_local_views_are_single_selection_following_lists() {
    let mut app = app_with_local(Tab::Work);
    let prototype = app.recorder.data.as_ref().unwrap().work[0].clone();
    for index in 0..12 {
        let mut plan = prototype.clone();
        plan.plan_id = format!("plan_{index:02}");
        plan.display_plan_id = plan.plan_id.clone();
        app.recorder.data.as_mut().unwrap().work.push(plan);
    }
    let last = app.recorder.data.as_ref().unwrap().work.len() - 1;
    for index in [0, last / 2, last] {
        app.work_index = index;
        let selected = app.recorder.data.as_ref().unwrap().work[index]
            .display_plan_id
            .clone();
        let rendered = render_text(&app, 40, 12);
        assert!(
            rendered.contains(&selected),
            "missing selected row {selected}"
        );
        assert!(!rendered.contains("Plan preview"));
    }

    let timeline = app.recorder.data.as_ref().unwrap().timeline[0].clone();
    let health = app.recorder.data.as_ref().unwrap().health[0].clone();
    for index in 0..12 {
        let mut row = timeline.clone();
        row.identity = format!("timeline-{index:02}");
        row.display_identity = row.identity.clone();
        row.primary = format!("TIMELINE_MARKER_{index:02}");
        app.recorder.data.as_mut().unwrap().timeline.push(row);
        let mut row = health.clone();
        row.identity = format!("health-{index:02}");
        row.primary = format!("HEALTH_MARKER_{index:02}");
        app.recorder.data.as_mut().unwrap().health.push(row);
    }
    for (tab, last, marker) in [
        (
            Tab::Timeline,
            app.timeline_rows().len() - 1,
            "TIMELINE_MARKER_11",
        ),
        (
            Tab::Health,
            app.recorder.data.as_ref().unwrap().health.len() - 1,
            "HEALTH_MARKER_11",
        ),
    ] {
        app.select_tab(tab);
        app.move_selection(isize::try_from(last).unwrap());
        assert!(render_text(&app, 40, 12).contains(marker));
    }
}

#[test]
fn plan_collection_errors_are_visible_in_empty_decision_and_receipt_sections() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut snapshot = scenarios::plan_snapshot();
    snapshot.decisions.clear();
    snapshot.receipts.clear();
    snapshot.errors.extend([
        SnapshotError::new(
            CollectionDomain::Decisions,
            SnapshotErrorCode::StreamReadFailed,
            Some(plan_id.clone()),
            "decisions unavailable",
        ),
        SnapshotError::new(
            CollectionDomain::Receipts,
            SnapshotErrorCode::RecordDecodeFailed,
            Some(plan_id.clone()),
            "receipts unavailable",
        ),
    ]);
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(snapshot)),
    );
    for _ in 0..3 {
        app.cycle_detail_section(false);
    }
    let decisions = render_text(&app, 120, 36);
    assert!(decisions.contains("decisions unavailable"));
    assert!(decisions.contains("Enter opens"));
    assert!(!decisions.contains("h/l horizontal"));
    app.cycle_detail_section(false);
    assert!(render_text(&app, 120, 36).contains("receipts unavailable"));
}

#[test]
fn detail_failures_converge_without_automatic_retargeting() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    app.accept_plan_result(basis, &plan_id, PlanSnapshotResult::StaleRecorderEpoch);
    assert!(app.take_plan_request().is_none());
    assert!(app.detail.error.as_deref().unwrap().contains("stale"));

    assert!(app.refresh_plan_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut mismatch = scenarios::plan_snapshot();
    mismatch.basis_epoch = RecorderEpochId::new(2).unwrap();
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(mismatch)),
    );
    assert!(app.take_plan_request().is_none());
    assert!(
        app.detail
            .error
            .as_deref()
            .unwrap()
            .contains("different recorder epoch")
    );

    assert!(app.refresh_plan_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut mismatch = scenarios::plan_snapshot();
    mismatch.plan.plan_id = "different-plan".to_string();
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(mismatch)),
    );
    assert!(
        app.detail
            .error
            .as_deref()
            .unwrap()
            .contains("different plan ID")
    );

    assert!(app.refresh_plan_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut unsupported = scenarios::plan_snapshot();
    unsupported.schema_version += 1;
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(unsupported)),
    );
    assert!(
        app.detail
            .error
            .as_deref()
            .unwrap()
            .contains("unsupported plan snapshot")
    );
}

#[test]
fn long_detail_lines_are_reachable_horizontally_and_item_details_become_stale() {
    let mut app = app_with_local(Tab::Work);
    assert!(app.open_selected_detail());
    let (basis, plan_id) = app.take_plan_request().unwrap();
    let mut snapshot = scenarios::plan_snapshot();
    let long = format!("{}TAIL_MARKER", "x".repeat(120));
    snapshot.body = Some(
        BoundedText::for_limit(&long, Some(long.chars().count()), LimitId::PlanBodyChars).unwrap(),
    );
    app.accept_plan_result(
        basis,
        &plan_id,
        PlanSnapshotResult::Found(Box::new(snapshot)),
    );
    app.cycle_detail_section(false);
    assert!(!render_text(&app, 80, 24).contains("TAIL_MARKER"));
    app.scroll_detail_horizontal(120);
    assert!(render_text(&app, 80, 24).contains("TAIL_MARKER"));

    app.close_detail();
    app.select_tab(Tab::Health);
    assert!(app.open_selected_detail());
    let mut recorder = scenarios::recorder_snapshot();
    recorder.epoch_id = RecorderEpochId::new(2).unwrap();
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder,
    });
    assert!(render_text(&app, 120, 36).contains("stale"));
}

#[test]
fn work_projection_drops_nested_gate_payload_and_timeline_summaries_stay_single_line() {
    let mut recorder = scenarios::recorder_snapshot();
    let gates = recorder.open_plans[0].gates.as_mut().unwrap();
    let mut gate = gates.gates.items()[0].clone();
    gate.changed_paths = BoundedRows::for_limit(
        vec!["NESTED_GATE_SENTINEL".to_string()],
        Some(1),
        LimitId::GateChangedPaths,
    )
    .unwrap();
    gates.gates = BoundedRows::for_limit(vec![gate], Some(1), LimitId::GateRows).unwrap();
    if let Some(TimelineRow::Decision(decision)) = recorder
        .timeline
        .iter_mut()
        .find(|row| matches!(row, TimelineRow::Decision(_)))
    {
        decision.rationale = BoundedText::for_limit(
            "first line\nsecond line",
            Some(22),
            LimitId::TimelineDecisionRationaleChars,
        )
        .unwrap();
    }
    let local: crate::terminal::model::LocalDashboard = recorder.into();
    assert!(!format!("{:?}", local.work[0].gates).contains("NESTED_GATE_SENTINEL"));
    assert!(
        local
            .timeline
            .iter()
            .all(|row| !row.secondary.contains('\n'))
    );
}
