use super::*;

#[test]
fn rendering_shows_loading_then_selected_account_and_usage() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let loading = terminal.backend().to_string();
    let loading_text = loading
        .replace(['│', '"'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(loading.contains("Codex Home Picker"), "{loading}");
    assert!(loading.contains("loading"), "{loading}");
    assert!(loading_text.contains("You can launch now."), "{loading}");

    const NOW: u64 = 2_000_000_000;
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);
    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();
    let ready = terminal.backend().to_string();
    assert!(ready.contains("person@example.com"), "{ready}");
    assert!(ready.contains("weekly 75% left"), "{ready}");
    assert!(ready.contains("+*"), "{ready}");
    assert!(ready.contains("best projected headroom"), "{ready}");
    assert!(ready.contains("Usage sample"), "{ready}");
    assert!(ready.contains("just now"), "{ready}");
    assert!(
        ready.contains("At current pace: ~50% left at reset"),
        "{ready}"
    );
}

#[test]
fn rendering_marks_projection_as_an_aging_usage_snapshot() {
    const OBSERVED_AT: u64 = 2_000_000_000;
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.apply_update_at(
        projected_update(0, 25.0, 10_080, 0.5, OBSERVED_AT),
        OBSERVED_AT,
    );

    terminal
        .draw(|frame| render::draw_at(frame, &app, OBSERVED_AT + 3_600))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Usage sample"), "{rendered}");
    assert!(rendered.contains("1h ago"), "{rendered}");
    assert!(rendered.contains("reopen to refresh"), "{rendered}");
}

#[test]
fn stale_projection_is_labeled_and_no_longer_recommended_in_the_list() {
    const OBSERVED_AT: u64 = 2_000_000_000;
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.apply_update_at(
        projected_update(0, 25.0, 10_080, 0.5, OBSERVED_AT),
        OBSERVED_AT,
    );
    let stale_at = OBSERVED_AT + 15 * 60;

    assert_eq!(app.best_projection_index(), Some(0));
    assert_eq!(app.best_projection_index_at(stale_at), None);
    terminal
        .draw(|frame| render::draw_at(frame, &app, stale_at))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("stale"), "{rendered}");
    assert!(!rendered.contains("+*"), "{rendered}");
    assert!(
        rendered.contains("no rankable Codex projection"),
        "{rendered}"
    );
}

#[test]
fn over_pace_recommendation_is_labeled_as_the_least_projected_overrun() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 60.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(projected_update(1, 70.0, 10_080, 0.5, NOW), NOW);

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("+*"), "{rendered}");
    assert!(rendered.contains("least projected overrun"), "{rendered}");
    assert!(rendered.contains("runs out early"), "{rendered}");
    assert!(!rendered.contains("best projected headroom"), "{rendered}");
}

#[test]
fn common_width_lists_keep_non_selected_projection_outcomes_visible() {
    const NOW: u64 = 2_000_000_000;
    for width in [100, 120] {
        let backend = TestBackend::new(width, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app(homes());
        app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);
        app.apply_update_at(projected_update(1, 60.0, 10_080, 0.5, NOW), NOW);

        terminal
            .draw(|frame| render::draw_at(frame, &app, NOW))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(
            rendered.contains("person@example.com"),
            "{width}: {rendered}"
        );
        assert!(rendered.contains("weekly 40% left"), "{width}: {rendered}");
        assert!(
            rendered.contains("runs out ~1.2d early"),
            "{width}: {rendered}"
        );
    }
}

#[test]
fn common_width_lists_keep_each_non_selected_account_visible() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    let mut second = projected_update(1, 60.0, 10_080, 0.5, NOW);
    second.details["account"]["email"] = json!("work@example.com");
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(second, NOW);

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("work@example.com"), "{rendered}");
}

#[test]
fn compact_warmup_shows_current_remaining_instead_of_only_collecting() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 95.0, 10_080, 0.03, NOW), NOW);

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(
        rendered.contains("weekly: 5% left · collecting"),
        "{rendered}"
    );
    assert_eq!(app.best_projection_index(), None);
}

#[test]
fn full_table_breakpoint_keeps_long_projection_labels_visible() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(168, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    let mut exhausted = projected_update(1, 100.0, 10_080, 0.5, NOW);
    exhausted.details["account"]["email"] = json!("work@example.com");
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(exhausted, NOW);

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(
        rendered.contains("weekly: exhausted until reset"),
        "{rendered}"
    );
}

#[test]
fn compact_layout_keeps_the_projection_visible() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Projection"), "{rendered}");
    assert!(rendered.contains("person@example.com"), "{rendered}");
    assert!(
        rendered.contains("weekly: ~50% left at reset"),
        "{rendered}"
    );
}

#[test]
fn compact_layout_names_inspection_errors_and_signed_out_accounts() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    let mut failed = ready_update(0);
    failed.details["inspection_error"] = json!("app-server failed");
    app.apply_update(failed);
    app.apply_update(HomeUpdate {
        index: 1,
        details: json!({
            "account": null,
            "status": "not logged in",
            "rate_limits": [],
            "inspection_error": null,
            "usage_error": null
        }),
    });

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("inspection error"), "{rendered}");
    assert!(rendered.contains("signed out"), "{rendered}");
}

#[test]
fn compact_layout_names_usage_errors() {
    const NOW: u64 = 2_000_000_000;
    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    let mut failed = ready_update(0);
    failed.details["usage_error"] = json!("usage unavailable");
    app.apply_update(failed);

    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("usage error"), "{rendered}");
}
