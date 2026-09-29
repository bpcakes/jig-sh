use crate::{
    dashboard::{
        BoundedRows, BoundedText, CollectionDomain, LimitId, LoopStateError, RecorderEpochId,
        RecorderRefresh, RecorderSnapshot, Remediation, ScheduledOccurrence, SnapshotError,
        SnapshotErrorCode, TimelineRow, scenarios,
    },
    terminal::model::{App, Tab},
};

use super::{normalized, render_text, status_local};

mod parity;

fn app_with_local(tab: Tab) -> App {
    let mut app = App::new(tab);
    app.recorder.data = Some(scenarios::recorder_snapshot().into());
    app
}

fn accept_recorder(app: &mut App, recorder: RecorderSnapshot) {
    app.accept_recorder_refresh(RecorderRefresh {
        status_local: status_local(scenarios::status_snapshot(), &recorder),
        recorder,
    });
}

fn assert_contains_all(rendered: &str, expected: &[&str]) {
    for value in expected {
        assert!(
            rendered.contains(value),
            "missing {value:?} from:\n{rendered}"
        );
    }
}

/// A target result row whose identity is `<run>:<target>`.
fn result_row(run_id: &str, timestamp_ms: u64, conclusion: &str) -> TimelineRow {
    TimelineRow {
        stable_identity: format!("{run_id}:api:test"),
        timestamp_ms: Some(timestamp_ms),
        run_id: run_id.to_string(),
        target: "api:test".to_string(),
        status: "completed".to_string(),
        conclusion: Some(conclusion.to_string()),
        exit_code: Some(i64::from(conclusion != "success")),
        started_at_ms: Some(timestamp_ms.saturating_sub(50)),
        ended_at_ms: Some(timestamp_ms),
        duration_ms: Some(50),
        finding_count: None,
        output_tail: None,
    }
}

fn output_failure(recorder: &mut RecorderSnapshot, output: &str) {
    recorder.failures[0].output_tail = BoundedText::for_limit(
        output,
        Some(output.chars().count()),
        LimitId::FailureOutputChars,
    )
    .unwrap();
}

#[test]
fn failure_output_is_bounded_and_scrollable() {
    let mut recorder = scenarios::recorder_snapshot();
    let mut older = recorder.failures[0].clone();
    older.run_id = "run_older".to_string();
    older.ended_at_ms = Some(scenarios::OBSERVED_AT_MS - 5_000);
    recorder.failures.push(older);
    let output = format!("first\n{}\nlast", "x".repeat(389));
    recorder.failures[0].output_tail =
        BoundedText::for_limit(output, Some(425), LimitId::FailureOutputChars).unwrap();
    let mut app = App::new(Tab::Health);
    app.recorder.data = Some(recorder.into());

    let failures = &app.recorder.data.as_ref().unwrap().failures;
    assert_eq!(failures[0].run_id, "run_failed");
    assert_eq!(failures[1].run_id, "run_older");
    let failure = app.selected_health().unwrap();
    assert_eq!(failure.identity, "failure:run_failed:api:test");
    assert_contains_all(
        &failure.detail.lines.join(" "),
        &[
            "Run: run_failed",
            "Target: api:test",
            "Conclusion: failure (exit 1)",
            "Output tail:",
            "first",
            "last",
            "limit 400 characters; 25 omitted",
        ],
    );
    assert!(app.open_selected_detail());
    let rendered = normalized(&render_text(&app, 80, 16));
    assert_contains_all(&rendered, &["Failure detail", "Output tail:", "first"]);
    assert!(app.detail.scroll_limit() > 0);
    app.move_detail_to_edge(true);
    let end = normalized(&render_text(&app, 80, 16));
    assert!(end.contains("limit 400 characters; 25 omitted"));
}

#[test]
fn target_health_renders_all_aggregates() {
    let app = app_with_local(Tab::Health);
    let item = app
        .recorder
        .data
        .as_ref()
        .unwrap()
        .health
        .iter()
        .find(|item| item.section == "Check health")
        .unwrap();
    assert_eq!(item.identity, "target:api:test");
    assert_contains_all(
        &item.detail.lines.join(" "),
        &[
            "Target: api:test",
            "Last conclusion: failure",
            "Last run:",
            "Runs: 3",
            "Failures: 1",
            "Average duration: 250ms",
        ],
    );
}

#[test]
fn loop_health_renders_workflow_and_lease_fields() {
    let app = app_with_local(Tab::Health);
    let health = &app.recorder.data.as_ref().unwrap().health;
    let workflow = health
        .iter()
        .find(|item| item.section == "Loop workflows")
        .unwrap();
    assert_contains_all(
        &workflow.detail.lines.join(" "),
        &[
            "Workflow: workflow-example",
            "Kind: queue",
            "Enabled: true",
            "Configured: true",
        ],
    );
    let lease = health
        .iter()
        .find(|item| item.section == "Active leases")
        .unwrap();
    assert_contains_all(
        &lease.detail.lines.join(" "),
        &[
            "Key: item-example",
            "Owner: worker-example",
            "Acquired:",
            "Expires:",
        ],
    );
}

#[test]
fn exhausted_attempt_keeps_identity_and_inert_recovery_argv() {
    let mut snapshot = scenarios::recorder_snapshot();
    let attempts = &mut snapshot
        .loops
        .as_mut()
        .unwrap()
        .needs_attention
        .exhausted_attempts;
    let mut attempt = attempts.items()[0].clone();
    attempt.key = "workflow with space:$(unsafe) 'quote'".to_string();
    attempt.workflow_id = "workflow with space".to_string();
    attempt.item_key = "$(unsafe) 'quote'".to_string();
    attempt.remediation = Some(Remediation {
        argv: vec![
            "scripts/jig".to_string(),
            "loop".to_string(),
            "clear-attempt".to_string(),
            "--workflow".to_string(),
            "workflow with space".to_string(),
            "--item".to_string(),
            "$(unsafe) 'quote'".to_string(),
        ],
        display: "producer display must not be executed".to_string(),
    });
    *attempts =
        BoundedRows::for_limit(vec![attempt], Some(1), LimitId::LoopExhaustedAttempts).unwrap();
    let mut app = App::new(Tab::Health);
    app.recorder.data = Some(snapshot.into());
    let exhausted = app
        .recorder
        .data
        .as_ref()
        .unwrap()
        .health
        .iter()
        .find(|item| item.detail.title == "Loop attention")
        .unwrap();
    assert_eq!(
        exhausted.identity,
        "attention:workflow with space:$(unsafe) 'quote'"
    );
    assert_contains_all(
        &exhausted.detail.lines.join(" "),
        &[
            "Workflow: workflow with space",
            "Item: $(unsafe) 'quote'",
            "Exhausted: true",
            "Recovery argv: scripts/jig loop clear-attempt --workflow 'workflow with space' --item '$(unsafe) '\"'\"'quote'\"'\"''",
        ],
    );
}

#[test]
fn timeline_and_loop_attention_details_are_reachable() {
    let mut app = app_with_local(Tab::Timeline);
    assert!(app.open_selected_detail());
    let result = normalized(&render_text(&app, 80, 24));
    assert_contains_all(
        &result,
        &["Target result", "Run: run_failed", "Target: api:test"],
    );
    app.close_detail();
    assert!(!app.detail_is_open());

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
    recorder.limits.failures.omitted = Some(3);
    recorder.limits.timeline.omitted = None;
    let mut app = App::new(Tab::Health);
    app.recorder.data = Some(recorder.into());
    let health = normalized(&render_text(&app, 200, 36));
    assert!(health.contains("limit 10 failures; 3 omitted"), "{health}");
    assert!(
        health.contains("example loop data is unavailable"),
        "{health}"
    );
    assert!(health.contains("Recent failures"));

    app.select_tab(Tab::Timeline);
    let timeline = normalized(&render_text(&app, 120, 36));
    assert!(timeline.contains("omitted count unknown"));
    assert!(timeline.contains("run_failed"));
}

#[test]
fn recorder_refresh_keeps_timeline_selection_and_open_detail() {
    let mut recorder = scenarios::recorder_snapshot();
    recorder.timeline = vec![
        result_row("run_newer", scenarios::OBSERVED_AT_MS - 100, "success"),
        result_row("run_older", scenarios::OBSERVED_AT_MS - 200, "success"),
    ];
    let mut app = App::new(Tab::Timeline);
    accept_recorder(&mut app, recorder.clone());
    app.move_selection(1);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "run_older:api:test"
    );

    recorder.timeline.insert(
        0,
        result_row("run_newest", scenarios::OBSERVED_AT_MS, "success"),
    );
    accept_recorder(&mut app, recorder.clone());
    assert_eq!(app.timeline_index, 2);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "run_older:api:test"
    );

    assert!(app.open_selected_detail());
    recorder.epoch_id = RecorderEpochId::new(2).unwrap();
    recorder.timeline.clear();
    accept_recorder(&mut app, recorder);
    assert!(app.detail_is_open());
    assert!(
        app.detail
            .document
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|line| line == "Run: run_older")
    );
    assert!(app.selected_timeline().is_none());
}

#[test]
fn multiline_text_and_labels_cross_the_terminal_boundary_safely() {
    let mut recorder = scenarios::recorder_snapshot();
    output_failure(&mut recorder, "first\r\nsecond\tcolumn\nthird\u{1b}[31m");
    recorder.timeline[0].target = "api\u{1b}[31m\u{202e}:test".to_string();
    let mut app = App::new(Tab::Timeline);
    app.recorder.data = Some(recorder.into());
    let timeline = render_text(&app, 120, 36);
    assert!(!timeline.contains('\u{1b}'));
    assert!(!timeline.contains('\u{202e}'));

    app.select_tab(Tab::Health);
    assert!(app.open_selected_detail());
    let rendered = render_text(&app, 120, 36);
    assert!(rendered.lines().any(|line| line.contains("first")));
    assert!(rendered.lines().any(|line| line.contains("second column")));
    assert!(rendered.lines().any(|line| line.contains("third�[31m")));
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn detail_scroll_edges_clamp_and_document_survives_epoch_refresh() {
    let mut recorder = scenarios::recorder_snapshot();
    output_failure(&mut recorder, "one\ntwo\nthree\nfour");
    let mut app = App::new(Tab::Health);
    accept_recorder(&mut app, recorder);
    assert!(app.open_selected_detail());
    let limit = app.detail.scroll_limit();
    assert_eq!(limit, 10);
    app.move_detail_to_edge(true);
    assert_eq!(app.detail.scroll, limit);
    app.scroll_detail(10);
    assert_eq!(app.detail.scroll, limit);
    app.move_detail_to_edge(false);
    assert_eq!(app.detail.scroll, 0);
    app.scroll_detail(3);
    assert_eq!(app.detail.scroll, 3);

    let mut recorder = scenarios::recorder_snapshot();
    recorder.epoch_id = RecorderEpochId::new(2).unwrap();
    accept_recorder(&mut app, recorder);
    assert!(app.detail_is_open());
    assert_eq!(app.detail.scroll, 3);
    assert_eq!(app.detail.item_epoch, Some(RecorderEpochId::FIRST));
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
    accept_recorder(&mut app, recorder);
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
    accept_recorder(&mut app, recorder);
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
        CollectionDomain::Runs,
        SnapshotErrorCode::StreamReadFailed,
        Some("run\u{1b}[31m-target".to_string()),
        "run stream failed",
    ));
    let mut app = App::new(Tab::Timeline);
    app.recorder.data = Some(recorder.into());
    let rendered = render_text(&app, 120, 36);
    assert!(rendered.contains("run�[31m-target"));
    assert!(rendered.contains("run stream failed"));
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn local_header_reports_default_branch_age_and_detached_state_on_every_local_tab() {
    let mut app = app_with_local(Tab::Timeline);
    for tab in [Tab::Timeline, Tab::Health] {
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
    let mut app = app_with_local(Tab::Timeline);
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
        let rendered = render_text(&app, 40, 12);
        assert!(rendered.contains(marker), "{tab:?}:\n{rendered}");
        assert!(!rendered.contains("preview"), "{tab:?}:\n{rendered}");
    }
}

#[test]
fn long_detail_lines_are_reachable_horizontally_and_item_details_become_stale() {
    let mut recorder = scenarios::recorder_snapshot();
    output_failure(&mut recorder, &format!("{}TAIL_MARKER", "x".repeat(120)));
    let mut app = App::new(Tab::Health);
    accept_recorder(&mut app, recorder);
    assert!(app.open_selected_detail());
    assert!(!render_text(&app, 80, 24).contains("TAIL_MARKER"));
    app.scroll_detail_horizontal(120);
    assert!(render_text(&app, 80, 24).contains("TAIL_MARKER"));
    assert!(!render_text(&app, 80, 24).contains("stale"));

    let mut recorder = scenarios::recorder_snapshot();
    recorder.epoch_id = RecorderEpochId::new(2).unwrap();
    accept_recorder(&mut app, recorder);
    assert!(render_text(&app, 120, 36).contains("stale"));
}

#[test]
fn timeline_summaries_stay_single_line() {
    let mut recorder = scenarios::recorder_snapshot();
    recorder.timeline[0].run_id = "first line\nsecond line".to_string();
    let local: crate::terminal::model::LocalDashboard = recorder.into();
    assert!(
        local
            .timeline
            .iter()
            .all(|row| !row.primary.contains('\n') && !row.secondary.contains('\n'))
    );
}
