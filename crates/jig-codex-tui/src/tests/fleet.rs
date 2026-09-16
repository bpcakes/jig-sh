//! Application and rendering behavior of the Codex fleet forecast.

use super::*;
use crate::{ConfigurationHome, model::FleetOutcome};

/// Two accounts burning 90% of a weekly window each: a modeled gap arrives well inside the
/// horizon, so the summary has something to say at every terminal size.
fn pressured_app() -> (App, u64) {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut second = projected_update(1, 90.0, 10_080, 0.5, NOW);
    second.details["account"]["email"] = json!("work@example.com");
    app.apply_update_at(projected_update(0, 90.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(second, NOW);
    (app, NOW)
}

#[test]
fn searching_and_selection_never_change_the_forecast_account_pool() {
    let (mut app, now) = pressured_app();
    let unfiltered = app.fleet_assessment_at(now).expect("Codex fleet forecast");
    assert_eq!(unfiltered.coverage().included(), 2);

    for character in "work".chars() {
        app.push_filter(character);
    }
    assert_eq!(app.visible_indices(), vec![1]);
    app.move_to_edge(true);

    let filtered = app.fleet_assessment_at(now).expect("Codex fleet forecast");
    assert_eq!(filtered, unfiltered);
    assert_eq!(filtered.coverage().homes(), 2);
}

#[test]
fn the_forecast_survives_terminal_widths_down_to_the_supported_minimum() {
    let (app, now) = pressured_app();

    for (width, height) in [(46, 12), (60, 20), (80, 24), (120, 30), (168, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render::draw_at(frame, &app, now))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(
            rendered.contains("All accounts 2/2"),
            "{width}x{height}: {rendered}"
        );
        assert!(
            rendered.contains("Inspection complete") || rendered.contains("Inspecting"),
            "{width}x{height}: {rendered}"
        );
    }
}

#[test]
fn the_detail_pane_explains_the_forecast_assumptions_for_any_selected_row() {
    const NOW: u64 = 2_000_000_000;
    let now = NOW;
    // The second row is still loading, so the pane must explain the pool even when the
    // selected row has no usage of its own yet.
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 90.0, 10_080, 0.5, NOW), NOW);

    for selected in [0, 1] {
        app.selected = Some(selected);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| render::draw_at(frame, &app, now))
            .unwrap();

        let rendered = terminal.backend().to_string();
        for expected in [
            "Codex fleet forecast",
            "gap risk",
            "window-average pace",
            "periodic approximation",
            "earliest reset used first",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected}: {rendered}"
            );
        }
    }
}

#[test]
fn a_partial_cohort_is_never_presented_as_an_all_account_result() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 20.0, 10_080, 0.5, NOW), NOW);
    app.finish_inspection(None);

    let assessment = app.fleet_assessment_at(NOW).expect("Codex fleet forecast");
    assert!(!assessment.coverage().is_complete());
    let summary = assessment.summary_label_at(NOW);
    assert!(summary.starts_with("Fleet partial 1/2:"), "{summary}");
    assert!(!summary.contains("All accounts"), "{summary}");
    assert!(assessment.detail_lines_at(NOW).contains(&(
        "Accounts".to_owned(),
        "1 of 2 discovered homes · 1 inspection unavailable".to_owned()
    )));

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Fleet partial 1/2"), "{rendered}");
    assert!(!rendered.contains("All accounts"), "{rendered}");
}

#[test]
fn the_summary_ages_without_a_new_inspection_result() {
    let (app, now) = pressured_app();
    let fresh = app.fleet_assessment_at(now).expect("Codex fleet forecast");
    let later = app
        .fleet_assessment_at(now + 600)
        .expect("Codex fleet forecast");
    let stale = app
        .fleet_assessment_at(now + 900)
        .expect("Codex fleet forecast");

    // The modeled timeline is fixed to the observed samples; only the countdown moves.
    assert_eq!(later.outcome(), fresh.outcome());
    assert_eq!(stale.outcome(), fresh.outcome());
    assert_ne!(
        fresh.outcome_label_at(now + 3_600),
        fresh.outcome_label_at(now)
    );
    assert!(!later.is_stale());
    assert!(stale.is_stale());

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| render::draw_at(frame, &app, now + 900))
        .unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("fleet forecast stale"), "{rendered}");
    assert!(rendered.contains("reopen to refresh"), "{rendered}");
}

#[test]
fn repeated_draws_reuse_one_forecast_per_inspection_generation() {
    let (mut app, now) = pressured_app();
    let first = app.fleet_assessment_at(now).expect("Codex fleet forecast");
    assert_eq!(app.fleet_assessment_at(now).as_ref(), Some(&first));

    let mut relieved = projected_update(1, 10.0, 10_080, 0.5, now);
    relieved.details["account"]["email"] = json!("work@example.com");
    app.apply_update_at(relieved, now);

    let updated = app.fleet_assessment_at(now).expect("Codex fleet forecast");
    assert_ne!(updated.outcome(), first.outcome());
    assert_eq!(
        updated.outcome(),
        &FleetOutcome::NoGap {
            burn_observed: true
        }
    );
}

#[test]
fn other_providers_and_configuration_views_gain_no_forecast() {
    const NOW: u64 = 2_000_000_000;
    let entries = || {
        vec![ConfigurationHome {
            home: Home {
                path: "/tmp/ExampleHome".into(),
                name: "ExampleHome".into(),
                current: true,
            },
            details: Vec::new(),
        }]
    };
    let cases = [
        App::provider(
            "Claude Home Picker",
            entries(),
            Vec::new(),
            true,
            Some("claude"),
        ),
        App::provider("Example Agent Picker", entries(), Vec::new(), true, None),
        App::configuration("Example Picker", entries(), Vec::new()),
    ];

    for mut app in cases {
        app.apply_update_at(projected_update(0, 90.0, 10_080, 0.5, NOW), NOW);
        assert!(app.fleet_assessment_at(NOW).is_none());

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| render::draw_at(frame, &app, NOW))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(!rendered.contains("fleet"), "{rendered}");
        assert!(!rendered.contains("Fleet"), "{rendered}");
    }
}

#[test]
fn the_codex_picker_reports_a_healthy_pool_without_claiming_certainty() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut second = projected_update(1, 5.0, 10_080, 0.5, NOW);
    second.details["account"]["email"] = json!("work@example.com");
    app.apply_update_at(projected_update(0, 5.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(second, NOW);

    let assessment = app.fleet_assessment_at(NOW).expect("Codex fleet forecast");

    assert_eq!(
        assessment.summary_label_at(NOW),
        "All accounts 2/2: no gap projected through 7d"
    );
    assert!(assessment.detail_lines_at(NOW).contains(&(
        "Horizon".to_owned(),
        "7d from the newest usage sample".to_owned()
    )));
}
