use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

use crate::{
    Home, HomeUpdate,
    model::{App, ExitState, Focus, Inspection},
    render,
    runtime::{Action, handle_key},
};

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
    assert_eq!(app.rows[1].usage(), "weekly 25%/7d");
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

    assert_eq!(app.rows[0].usage(), "5h 10%/5h, 5h 20%/5h");
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
    assert!(loading.contains("Codex Home Picker"), "{loading}");
    assert!(loading.contains("loading"), "{loading}");
    assert!(loading.contains("You can launch now"), "{loading}");

    app.apply_update(ready_update(0));
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let ready = terminal.backend().to_string();
    assert!(ready.contains("person@example.com"), "{ready}");
    assert!(ready.contains("weekly 25%/7d"), "{ready}");
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
