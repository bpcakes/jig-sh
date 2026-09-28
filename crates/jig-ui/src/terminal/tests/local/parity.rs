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
fn repository_failures_and_tool_times_keep_exact_semantics() {
    let mut snapshot = scenarios::recorder_snapshot();
    let mut newest = snapshot.failures[0].clone();
    newest.id = "failure-newest".to_string();
    newest.ended_at_ms = Some(scenarios::OBSERVED_AT_MS);
    newest.tool_name = "jig.clippy".to_string();
    newest.exit_status = 7;
    let mut oldest = newest.clone();
    oldest.id = "failure-oldest".to_string();
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
    assert_ne!(local.tools[0].last_ended_at, "—");
    assert_eq!(local.failures[0].id, "failure-newest");
    assert_eq!(local.failures[0].tool, "jig.clippy");
    assert_eq!(local.failures[0].exit_status, 7);
    assert_eq!(local.failures[1].id, "failure-oldest");
    assert_ne!(local.failures[0].ended_at, local.failures[1].ended_at);

    for (index, expected) in [(0, "failure:failure-newest"), (1, "failure:failure-oldest")] {
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
    snapshot.timeline.push(receipt_row(
        "receipt:\u{1b}[31mraw",
        scenarios::OBSERVED_AT_MS - 4_000,
        0,
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
        "receipt:receipt_failed"
    );
    app.cycle_timeline_filter(true);
    app.move_selection(1);
    assert_eq!(
        app.selected_timeline().unwrap().identity,
        "receipt:\u{1b}[31mraw"
    );
    let rendered = render_text(&app, 120, 36);
    assert!(!rendered.contains('\u{1b}'));
}

#[test]
fn receipt_timeline_is_newest_first_and_rows_open_their_detail() {
    let mut newest = receipt_row("receipt:newest", 400, 9);
    let TimelineRow::Receipt(row) = &mut newest;
    row.diff_summary = Some("2 files changed".to_string());
    row.changed_path_count = Some(2);
    let mut snapshot = scenarios::recorder_snapshot();
    snapshot.timeline = vec![
        receipt_row("receipt:oldest", 100, 0),
        newest,
        receipt_row("receipt:middle", 300, 0),
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
        ["receipt:newest", "receipt:middle", "receipt:oldest"]
    );
    assert_eq!(local.timeline[0].primary, "jig.test exit 9");
    assert_eq!(local.timeline[1].primary, "jig.test pass");
    assert_contains_all(
        &local.timeline[0].detail.lines.join(" "),
        &[
            "Exit: 9",
            "Duration: 50ms",
            "Diff: 2 files changed",
            "Changed paths: 2",
            "Command key: test",
        ],
    );

    for (index, receipt) in [(0, "newest"), (1, "middle"), (2, "oldest")] {
        app.timeline_index = index;
        assert!(app.open_selected_detail());
        let rendered = normalized(&render_text(&app, 120, 36));
        assert_contains_all(
            &rendered,
            &[
                "Receipt event",
                &format!("Receipt: {receipt}"),
                "Esc closes",
            ],
        );
        app.close_detail();
    }
}
