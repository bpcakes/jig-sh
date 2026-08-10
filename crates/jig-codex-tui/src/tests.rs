use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jig_tui::format_percent;
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

use crate::{
    Home, HomeUpdate,
    model::{App, ExitState, Focus, Inspection, Projection},
    render,
    runtime::{Action, handle_key},
};

const PROJECTION_TOLERANCE: f64 = 1e-9;

#[test]
fn percent_formatting_never_rounds_a_partial_quota_up_to_complete() {
    assert_eq!(format_percent(0.04), "0.1%");
    assert_eq!(format_percent(99.98), "99.9%");
    assert_eq!(format_percent(100.0), "100%");
    assert_eq!(format_percent(42.26), "42.3%");
}

fn homes() -> Vec<Home> {
    vec![
        Home {
            path: PathBuf::from("/tmp/.codex"),
            name: "codex".into(),
            current: true,
        },
        Home {
            path: PathBuf::from("/tmp/.codex-work"),
            name: "codex-work".into(),
            current: false,
        },
    ]
}

fn app(homes: Vec<Home>) -> App {
    App::new(homes, Vec::new())
}

fn ready_update(index: usize) -> HomeUpdate {
    HomeUpdate {
        index,
        details: json!({
            "account": {
                "type": "chatgpt",
                "email": "person@example.com",
                "plan_type": "pro"
            },
            "status": "authenticated",
            "rate_limits": [{
                "id": "codex",
                "name": null,
                "plan_type": "pro",
                "primary": {
                    "used_percent": 25,
                    "duration_minutes": 10080,
                    "resets_at": null
                },
                "secondary": null,
                "reached": null
            }],
            "inspection_error": null,
            "usage_error": null
        }),
    }
}

fn projected_update(
    index: usize,
    used_percent: f64,
    duration_minutes: u64,
    elapsed_fraction: f64,
    now: u64,
) -> HomeUpdate {
    let duration_seconds = duration_minutes * 60;
    let remaining_seconds = (duration_seconds as f64 * (1.0 - elapsed_fraction)).round() as u64;
    let mut update = ready_update(index);
    update.details["rate_limits"][0]["primary"]["used_percent"] = json!(used_percent);
    update.details["rate_limits"][0]["primary"]["duration_minutes"] = json!(duration_minutes);
    update.details["rate_limits"][0]["primary"]["resets_at"] = json!(now + remaining_seconds);
    update
}

#[test]
fn starts_with_visible_loading_rows_and_current_selection() {
    let app = app(homes());
    assert_eq!(app.selected, Some(0));
    assert!(matches!(app.rows[0].inspection, Inspection::Loading));
    assert_eq!(app.rows[0].usage(), "loading…");
}

#[test]
fn update_is_indexed_and_single_codex_window_is_weekly() {
    let mut app = app(homes());
    app.apply_update(ready_update(1));

    assert_eq!(app.completed, 1);
    assert_eq!(app.rows[1].account(), "person@example.com");
    assert_eq!(app.rows[1].usage(), "weekly 75% left");
    assert!(matches!(app.rows[0].inspection, Inspection::Loading));
}

#[test]
fn duplicate_codex_window_durations_receive_the_same_role() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["primary"] =
        json!({ "used_percent": 10, "duration_minutes": 300 });
    update.details["rate_limits"][0]["secondary"] =
        json!({ "used_percent": 20, "duration_minutes": 300 });

    app.apply_update(update);

    assert_eq!(app.rows[0].usage(), "5h 90% left, 5h 80% left");
}

#[test]
fn unrecognized_codex_window_durations_remain_distinguishable() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["primary"] =
        json!({ "used_percent": 10, "duration_minutes": 120 });
    update.details["rate_limits"][0]["secondary"] =
        json!({ "used_percent": 20, "duration_minutes": 240 });

    app.apply_update(update);

    assert_eq!(app.rows[0].usage(), "2h 90% left, 4h 80% left");
}

#[test]
fn projection_compares_usage_with_elapsed_window_time() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::Remaining { percent, .. }
            if (percent - 50.0).abs() < PROJECTION_TOLERANCE
    ));
    assert_eq!(
        app.rows[0].projection().label(),
        "weekly: ~50% left at reset"
    );
}

#[test]
fn projection_reports_when_quota_runs_out_before_reset() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 60.0, 10_080, 0.5, NOW), NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::ExhaustsEarly { seconds, .. } if seconds == 100_800
    ));
    assert_eq!(
        app.rows[0].projection().label(),
        "weekly: runs out ~1.2d early"
    );
}

#[test]
fn projection_collects_data_during_the_first_tenth_of_a_window() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 2.0, 10_080, 0.05, NOW), NOW);

    assert_eq!(
        app.rows[0].projection(),
        Projection::Collecting {
            role: "weekly",
            remaining_percent: 98.0,
        }
    );
    assert_eq!(app.best_projection_index(), None);
}

#[test]
fn zero_usage_is_rankable_during_the_projection_warmup() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 0.0, 10_080, 0.05, NOW), NOW);
    app.apply_update_at(projected_update(1, 20.0, 10_080, 0.5, NOW), NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::Remaining {
            percent: 100.0,
            partial: false,
            ..
        }
    ));
    assert_eq!(app.best_projection_index(), Some(0));
}

#[test]
fn fallback_usage_bucket_is_projected_and_used_for_account_recommendation() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut update = projected_update(0, 25.0, 10_080, 0.5, NOW);
    update.details["rate_limits"][0]["id"] = json!("other");

    app.apply_update_at(update, NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::Remaining { percent, .. }
            if (percent - 50.0).abs() < PROJECTION_TOLERANCE
    ));
    assert_eq!(app.best_projection_index(), Some(0));
}

#[test]
fn valid_projection_survives_an_unavailable_sibling_but_remains_unranked() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut update = projected_update(1, 10.0, 10_080, 0.5, NOW);
    update.details["rate_limits"][0]["secondary"] = json!({
        "used_percent": 5.0,
        "duration_minutes": 300,
        "resets_at": null
    });
    app.apply_update_at(update, NOW);

    assert!(matches!(
        app.rows[1].projection(),
        Projection::Remaining {
            role: "weekly",
            percent,
            partial: true,
            ..
        } if (percent - 80.0).abs() < PROJECTION_TOLERANCE
    ));
    assert_eq!(
        app.rows[1].projection().label(),
        "weekly: ~80% left · partial"
    );
    assert_eq!(app.best_projection_index(), None);

    app.selected = Some(1);
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();
    let rendered = terminal.backend().to_string();
    assert!(!rendered.contains("Recommendation:"), "{rendered}");
}

#[test]
fn valid_warning_survives_a_collecting_sibling_but_remains_unranked() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut update = projected_update(1, 60.0, 10_080, 0.5, NOW);
    update.details["rate_limits"][0]["secondary"] = json!({
        "used_percent": 1.0,
        "duration_minutes": 300,
        "resets_at": NOW + 17_100
    });
    app.apply_update_at(update, NOW);

    assert!(matches!(
        app.rows[1].projection(),
        Projection::ExhaustsEarly {
            role: "weekly",
            partial: true,
            ..
        }
    ));
    assert!(
        app.rows[1]
            .projection()
            .label()
            .contains("runs out ~1.2d early · partial")
    );
    assert_eq!(app.best_projection_index(), None);
}

#[test]
fn exhausted_quota_is_explicit_instead_of_a_future_exhaustion() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 100.0, 10_080, 0.5, NOW), NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::Exhausted {
            role: "weekly",
            partial: false,
            ..
        }
    ));
    assert_eq!(
        app.rows[0].projection().label(),
        "weekly: exhausted until reset"
    );
    assert_eq!(app.best_projection_index(), None);
}

#[test]
fn overreported_quota_is_clamped_to_zero_remaining_and_exhausted() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["primary"]["used_percent"] = json!(100.5);
    app.apply_update_at(update, NOW);

    assert_eq!(app.rows[0].usage(), "weekly 0% left");
    assert!(matches!(
        app.rows[0].projection(),
        Projection::Exhausted {
            role: "weekly",
            partial: false,
        }
    ));
}

#[test]
fn sub_minute_overrun_is_not_rounded_up_to_one_minute() {
    let projection = Projection::ExhaustsEarly {
        role: "weekly",
        seconds: 10,
        score: -0.1,
        partial: false,
    };

    assert_eq!(projection.label(), "weekly: runs out <1m early");
    assert_eq!(projection.outcome_label(), "runs out <1m early");
}

#[test]
fn exhausted_window_dominates_other_complete_window_projections() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut update = projected_update(0, 60.0, 10_080, 0.5, NOW);
    update.details["rate_limits"][0]["secondary"] = json!({
        "used_percent": 100.0,
        "duration_minutes": 300,
        "resets_at": NOW + 9_000
    });
    app.apply_update_at(update, NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::Exhausted {
            role: "5h",
            partial: false,
        }
    ));
    assert_eq!(app.best_projection_index(), None);
}

#[test]
fn best_projection_uses_burn_pace_not_raw_usage() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 30.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(projected_update(1, 20.0, 10_080, 0.25, NOW), NOW);

    assert_eq!(app.best_projection_index(), Some(0));
    assert_eq!(app.rows[0].display_name, "codex");
    assert_eq!(app.rows[1].display_name, "codex-work");
}

#[test]
fn projection_is_fixed_to_the_usage_observation_time() {
    const OBSERVED_AT: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(
        projected_update(0, 30.0, 300, 0.2, OBSERVED_AT),
        OBSERVED_AT,
    );

    let projection = app.rows[0].projection();
    assert!(matches!(projection, Projection::ExhaustsEarly { .. }));

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render::draw_at(frame, &app, OBSERVED_AT + 3_600))
        .unwrap();

    assert_eq!(app.rows[0].projection(), projection);
    assert_eq!(app.best_projection_index(), Some(0));
}

#[test]
fn generic_usage_fallback_keeps_bucket_window_context_and_ranking() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["id"] = json!("spark");
    update.details["rate_limits"][0]["name"] = json!("Spark");
    update.details["rate_limits"][0]["primary"]["duration_minutes"] = json!(1_440);
    update.details["rate_limits"][0]["primary"]["resets_at"] = json!(NOW + 43_200);
    app.apply_update_at(update, NOW);

    assert_eq!(app.rows[0].usage(), "Spark 1d 75% left");
    assert!(matches!(
        app.rows[0].projection(),
        Projection::Remaining {
            role: "window",
            percent,
            partial: false,
        } if (percent - 50.0).abs() < PROJECTION_TOLERANCE
    ));
    assert_eq!(app.best_projection_index(), Some(0));

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render::draw_at(frame, &app, NOW))
        .unwrap();
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Spark usage"), "{rendered}");
    assert!(
        rendered.contains("At current pace: ~50% left at reset"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("no rankable Codex projection"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Recommendation: best projected headroom at"),
        "{rendered}"
    );
}

#[test]
fn filtered_list_recommends_the_best_visible_account() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 10.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(projected_update(1, 30.0, 10_080, 0.5, NOW), NOW);
    assert_eq!(app.best_projection_index(), Some(0));

    for character in "work".chars() {
        app.push_filter(character);
    }

    assert_eq!(app.visible_indices(), vec![1]);
    assert_eq!(app.best_projection_index(), Some(1));
}

#[test]
fn best_projection_uses_the_tightest_returned_codex_window() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 30.0, 10_080, 0.5, NOW), NOW);
    let mut second = projected_update(1, 10.0, 10_080, 0.5, NOW);
    second.details["rate_limits"][0]["secondary"] = json!({
        "used_percent": 60.0,
        "duration_minutes": 300,
        "resets_at": NOW + 9_000
    });
    app.apply_update_at(second, NOW);

    assert!(matches!(
        app.rows[1].projection(),
        Projection::ExhaustsEarly { role: "5h", .. }
    ));
    assert_eq!(app.best_projection_index(), Some(0));
}

#[test]
fn update_arriving_after_worker_completion_repairs_progress() {
    let mut app = app(homes());
    app.finish_inspection(None);
    assert!(matches!(app.rows[0].inspection, Inspection::Unavailable));

    app.apply_update(ready_update(0));

    assert_eq!(app.completed, 1);
    assert!(matches!(app.rows[0].inspection, Inspection::Ready(_)));
}

#[test]
fn search_filters_details_and_enter_selects_exact_path_while_loading() {
    let mut app = app(homes());
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
        ),
        Action::Redraw
    );
    for character in "work".chars() {
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.visible_indices(), vec![1]);
    assert_eq!(app.selected_path(), Some(PathBuf::from("/tmp/.codex-work")));
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Select
    );
}

#[test]
fn search_supports_fuzzy_subsequence_matching() {
    let mut app = app(homes());
    for character in "cdxwk".chars() {
        app.push_filter(character);
    }
    assert_eq!(app.visible_indices(), vec![1]);
}

#[test]
fn search_prioritizes_home_names_over_a_matching_common_path() {
    let homes = vec![
        Home {
            path: PathBuf::from("/Users/workman/.codex"),
            name: "codex".into(),
            current: true,
        },
        Home {
            path: PathBuf::from("/Users/workman/.codex-work"),
            name: "codex-work".into(),
            current: false,
        },
    ];
    let mut app = app(homes);

    for character in "work".chars() {
        app.push_filter(character);
    }

    assert_eq!(app.visible_indices(), vec![1, 0]);
    assert_eq!(
        app.selected_path(),
        Some(PathBuf::from("/Users/workman/.codex-work"))
    );
}

#[test]
fn escape_leaves_search_before_it_cancels() {
    let mut app = app(homes());
    app.searching = true;
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Redraw
    );
    assert!(!app.searching);
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Cancel
    );
}

#[test]
fn control_u_clears_the_filter_while_search_remains_active() {
    let mut app = app(homes());
    app.searching = true;
    for character in "work".chars() {
        app.push_filter(character);
    }

    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
        ),
        Action::Redraw
    );
    assert!(app.filter.is_empty());
    assert!(app.searching);
}

#[test]
fn search_mode_keeps_list_paging_keys_active() {
    let homes = (0..25)
        .map(|index| Home {
            path: PathBuf::from(format!("/tmp/.codex-{index}")),
            name: format!("codex-{index}"),
            current: index == 0,
        })
        .collect();
    let mut app = app(homes);
    app.searching = true;

    for (key, expected) in [
        (KeyCode::End, 24),
        (KeyCode::PageUp, 14),
        (KeyCode::Home, 0),
        (KeyCode::PageDown, 10),
    ] {
        assert_eq!(
            handle_key(&mut app, KeyEvent::new(key, KeyModifiers::NONE)),
            Action::Redraw
        );
        assert_eq!(app.selected, Some(expected));
        assert!(app.searching);
    }
}

#[test]
fn tab_focuses_the_detail_pane_and_navigation_scrolls_it() {
    let mut app = app(homes());
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Action::Redraw
    );
    assert_eq!(app.focus, Focus::Details);
    app.set_detail_scroll_limit(5);
    handle_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, 1);
    assert_eq!(app.selected, Some(0));
}

#[test]
fn end_then_up_moves_immediately_in_the_detail_pane() {
    let mut app = app(homes());
    app.apply_update(ready_update(0));
    app.focus = Focus::Details;
    let max_scroll = 12;
    app.set_detail_scroll_limit(max_scroll);

    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, max_scroll);
    handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, max_scroll - 1);
    handle_key(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert!(app.detail_scroll < max_scroll - 1);
    handle_key(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(app.detail_scroll, 0);
}

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

#[test]
fn search_cursor_uses_terminal_cell_width() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.searching = true;
    app.push_filter('界');

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((10, 28));

    app.clear_filter();
    app.push_filter('e');
    app.push_filter('\u{301}');
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((9, 28));

    app.clear_filter();
    app.push_filter('\u{201c}');
    app.push_filter('\u{fe01}');
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    terminal.backend_mut().assert_cursor_position((9, 28));
}

#[test]
fn wrapped_detail_content_scrolls_to_its_final_rendered_row() {
    let backend = TestBackend::new(70, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["inspection_error"] = json!(format!(
        "{} TAIL-MARKER",
        "long inspection failure ".repeat(20)
    ));
    app.apply_update(update);
    app.finish_inspection(None);
    app.focus = Focus::Details;

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert!(app.detail_scroll > 0);
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("TAIL-MARKER"), "{rendered}");
}

#[test]
fn short_detail_content_does_not_overscroll() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.focus = Focus::Details;

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    handle_key(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));

    assert_eq!(app.detail_scroll, 0);
}

#[test]
fn worker_failure_uses_an_error_header_instead_of_success() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.finish_inspection(Some("inspection worker panicked".into()));

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Inspection stopped"), "{rendered}");
    assert!(!rendered.contains("Inspection complete"), "{rendered}");
}

#[test]
fn discovery_warnings_remain_visible_after_successful_inspection() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = App::new(
        homes(),
        vec!["Failed to inspect one Codex home candidate".into()],
    );
    app.apply_update(ready_update(0));
    app.apply_update(ready_update(1));
    app.finish_inspection(None);

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("discovery partial"), "{rendered}");
    assert!(rendered.contains("Discovery warning"), "{rendered}");
    assert!(rendered.contains("home candidate"), "{rendered}");
}

#[test]
fn unknown_update_diagnostic_survives_clean_worker_completion() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app(homes());
    app.apply_update(ready_update(0));
    app.apply_update(ready_update(1));
    app.apply_update(ready_update(99));
    app.finish_inspection(None);

    assert_eq!(
        app.inspection_error.as_deref(),
        Some("inspection returned unknown home index 99")
    );

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("Worker error"), "{rendered}");
    assert!(rendered.contains("index 99"), "{rendered}");
    assert!(rendered.contains("Inspection stopped"), "{rendered}");
    assert!(!rendered.contains("Inspection complete"), "{rendered}");
}

#[test]
fn unknown_update_diagnostics_accumulate_with_worker_failure() {
    let mut app = app(homes());

    app.apply_update(ready_update(98));
    app.apply_update(ready_update(99));
    app.apply_update(ready_update(98));
    app.finish_inspection(Some("inspection worker stopped".into()));
    app.apply_update(ready_update(99));

    assert_eq!(
        app.inspection_error.as_deref(),
        Some(
            "inspection returned unknown home index 98; inspection returned unknown home index 99; inspection worker stopped"
        )
    );
}

#[test]
fn exit_states_render_progress_during_worker_cleanup() {
    for (exit_state, expected) in [
        (ExitState::Launching, "Launching selected Codex home"),
        (
            ExitState::Cancelling,
            "Cancelling and cleaning up inspections",
        ),
    ] {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app(homes());
        app.begin_exit(exit_state);

        terminal.draw(|frame| render::draw(frame, &app)).unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains(expected), "{rendered}");
        assert!(rendered.contains("Please wait"), "{rendered}");
    }
}

#[test]
fn list_viewport_retains_context_when_navigation_reverses() {
    let backend = TestBackend::new(120, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    let homes = (0..20)
        .map(|index| Home {
            path: PathBuf::from(format!("/tmp/.codex-{index:02}")),
            name: format!("codex-{index:02}"),
            current: index == 0,
        })
        .collect();
    let mut app = app(homes);

    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    app.move_selection(12);
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let scrolled_offset = app.list_offset_for_viewport(14);
    assert!(scrolled_offset > 0);

    app.move_selection(-1);
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();

    assert_eq!(app.list_offset_for_viewport(14), scrolled_offset);
    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("codex-12"), "{rendered}");
}

#[test]
fn control_characters_are_sanitized_before_rendering() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["account"]["email"] = json!("unsafe\u{1b}[31m@example.com");
    app.apply_update(update);
    assert_eq!(app.rows[0].account(), "unsafe\u{fffd}[31m@example.com");
}

#[test]
fn bidi_controls_are_sanitized_while_script_joiners_are_preserved() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["account"]["email"] = json!("safe\u{202e}moc.elpmaxe\u{2069}\u{200c}\u{200d}");
    app.apply_update(update);

    assert_eq!(
        app.rows[0].account(),
        "safe\u{fffd}moc.elpmaxe\u{fffd}\u{200c}\u{200d}"
    );
}
