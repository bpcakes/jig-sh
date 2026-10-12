use super::*;
use crate::terminal::model::TimelineFilter;

#[test]
fn status_timeline_and_health_render_typed_parity_fields() {
    let recorder = scenarios::recorder_snapshot();
    let mut app = App::new(Tab::Status);
    accept_recorder(&mut app, recorder);
    let status = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &status,
        &[
            "ExampleProject",
            "default main",
            "Runtime: 0.3.0 · contract 8",
            "/example/source",
            "Recorder epoch 1",
        ],
    );

    app.select_tab(Tab::Timeline);
    let timeline = normalized(&render_text(&app, 120, 36));
    assert_contains_all(
        &timeline,
        &[
            "run_failed",
            "Target: api:test",
            "Conclusion: failure (exit 1)",
            "Findings: 2",
            "example failure",
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
fn repository_failures_and_target_times_keep_exact_semantics() {
    let mut snapshot = scenarios::recorder_snapshot();
    let mut newest = snapshot.failures[0].clone();
    newest.run_id = "run_newest".to_string();
    newest.ended_at_ms = Some(scenarios::OBSERVED_AT_MS);
    newest.target = "api:clippy".to_string();
    newest.conclusion = "timed_out".to_string();
    newest.exit_code = Some(7);
    let mut oldest = newest.clone();
    oldest.run_id = "run_oldest".to_string();
    oldest.ended_at_ms = Some(scenarios::OBSERVED_AT_MS - 10_000);
    snapshot.failures = vec![oldest, newest];

    let mut app = App::new(Tab::Health);
    app.recorder.data = Some(snapshot.into());
    let local = app.recorder.data.as_ref().unwrap();
    assert_eq!(local.repo.name, "ExampleProject");
    assert_eq!(local.repo.default_branch, "main");
    assert_eq!(local.repo.source_path.as_deref(), Some("/example/source"));
    assert_eq!(local.harness.runtime_version, "0.3.0");
    assert_eq!(local.harness.contract_version, 8);
    assert_ne!(local.targets[0].last_ended_at, "—");
    assert_eq!(local.failures[0].run_id, "run_newest");
    assert_eq!(local.failures[0].target, "api:clippy");
    assert_eq!(local.failures[0].outcome, "timed_out (exit 7)");
    assert_eq!(local.failures[1].run_id, "run_oldest");
    assert_ne!(local.failures[0].ended_at, local.failures[1].ended_at);

    for (index, expected) in [
        (0, "failure:run_newest:api:clippy"),
        (1, "failure:run_oldest:api:clippy"),
    ] {
        app.health_index = index;
        assert_eq!(app.selected_health().unwrap().identity, expected);
    }
}

#[test]
fn local_views_remain_reachable_at_compact_and_micro_sizes() {
    for tab in [Tab::Timeline, Tab::Health] {
        let app = app_with_local(tab);
        let compact = render_text(&app, 60, 15);
        assert!(compact.contains("Enter"), "{tab:?}:\n{compact}");
        let micro = render_text(&app, 39, 11);
        assert!(micro.contains("Jig"), "{tab:?}:\n{micro}");
        assert!(
            micro.contains("Selected") || micro.contains("Recent failures"),
            "{tab:?}:\n{micro}"
        );
    }
}

#[test]
fn timeline_filters_select_failures_and_preserve_raw_identity() {
    let mut snapshot = scenarios::recorder_snapshot();
    snapshot.timeline.push(result_row(
        "\u{1b}[31mraw",
        scenarios::OBSERVED_AT_MS - 4_000,
        "success",
    ));
    let mut app = App::new(Tab::Timeline);
    app.recorder.data = Some(snapshot.into());

    for (filter, count) in [(TimelineFilter::All, 2), (TimelineFilter::Failures, 1)] {
        assert_eq!(app.timeline_filter, filter);
        assert_eq!(app.timeline_rows().len(), count);
        app.cycle_timeline_filter(false);
    }
    assert_eq!(app.timeline_filter, TimelineFilter::All);

    app.cycle_timeline_filter(true);
    assert_eq!(app.timeline_filter, TimelineFilter::Failures);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "run_failed:api:test"
    );
    app.cycle_timeline_filter(true);
    app.move_selection(1);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "\u{1b}[31mraw:api:test"
    );
    let rendered = render_text(&app, 120, 36);
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn timeline_failure_filter_excludes_cancelled_and_skipped_targets() {
    let mut recorder = scenarios::recorder_snapshot();
    recorder.timeline = vec![
        result_row("run_cancelled", 600, "cancelled"),
        result_row("run_skipped", 500, "skipped"),
        result_row("run_timed_out", 400, "timed_out"),
        result_row("run_blocked", 300, "blocked"),
        result_row("run_failed", 200, "failure"),
        result_row("run_success", 100, "success"),
    ];
    for row in &mut recorder.timeline {
        if matches!(row.conclusion.as_deref(), Some("cancelled" | "skipped")) {
            row.started_at_ms = None;
            row.duration_ms = None;
            row.exit_code = None;
        }
    }
    let mut app = App::new(Tab::Timeline);
    accept_recorder(&mut app, recorder);
    assert_eq!(app.timeline_rows().len(), 6);

    app.cycle_timeline_filter(false);
    assert_eq!(app.timeline_filter, TimelineFilter::Failures);
    assert_eq!(
        app.timeline_rows()
            .iter()
            .map(|row| row.identity.as_str())
            .collect::<Vec<_>>(),
        [
            "run_timed_out:api:test",
            "run_blocked:api:test",
            "run_failed:api:test",
        ]
    );

    app.cycle_timeline_filter(false);
    assert_eq!(app.timeline_filter, TimelineFilter::All);
    assert_eq!(app.timeline_rows().len(), 6);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "run_cancelled:api:test"
    );
}

#[test]
fn target_result_timeline_is_newest_first_and_rows_open_their_detail() {
    let mut newest = result_row("run_newest", 400, "failure");
    newest.exit_code = Some(9);
    newest.finding_count = Some(2);
    let mut snapshot = scenarios::recorder_snapshot();
    snapshot.timeline = vec![
        result_row("run_oldest", 100, "success"),
        newest,
        result_row("run_middle", 300, "success"),
    ];
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
            "run_newest:api:test",
            "run_middle:api:test",
            "run_oldest:api:test"
        ]
    );
    assert_eq!(local.timeline[0].primary, "api:test failure (exit 9)");
    assert_eq!(local.timeline[1].primary, "api:test success");
    assert_contains_all(
        &local.timeline[0].detail.lines.join(" "),
        &[
            "Status: completed",
            "Exit: 9",
            "Duration: 50ms",
            "Findings: 2",
        ],
    );

    for (index, run) in [(0, "run_newest"), (1, "run_middle"), (2, "run_oldest")] {
        app.timeline_index = index;
        assert!(app.open_selected_detail());
        let rendered = normalized(&render_text(&app, 120, 36));
        assert_contains_all(
            &rendered,
            &["Target result", &format!("Run: {run}"), "Esc closes"],
        );
        app.close_detail();
    }
}
