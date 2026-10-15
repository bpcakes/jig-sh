//! The full global explanation is reachable without changing home or search state.

use super::*;

const NOW: u64 = 2_000_000_000;

fn partial_app() -> App {
    let mut app = app(homes());
    let mut update = projected_update(0, 90.0, 300, 0.5, NOW);
    update.details["rate_limits"][0]["secondary"] = json!({
        "used_percent": 20,
        "duration_minutes": 10_080,
        "resets_at": NOW + 302_400
    });
    // Codex reports banked resets, so the explanation says the scenario excludes them.
    update.details["banked_resets"] = json!(2);
    app.apply_update_at(update, NOW);
    app.finish_inspection(None);
    app
}

fn press(app: &mut App, code: KeyCode) -> Action {
    handle_key(app, KeyEvent::new(code, KeyModifiers::NONE))
}

fn page(terminal: &mut Terminal<TestBackend>, app: &App, monochrome: bool) -> String {
    terminal
        .draw(|frame| {
            if monochrome {
                render::draw_themed(
                    frame,
                    app,
                    render::Theme::from_env(|key| (key == "NO_COLOR").then(|| "1".to_owned())),
                );
            } else {
                render::draw_at(frame, app, NOW);
            }
        })
        .unwrap();
    if monochrome {
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|cell| cell.fg == ratatui::style::Color::Reset
                    && cell.bg == ratatui::style::Color::Reset)
        );
    }
    terminal
        .backend()
        .to_string()
        .replace(['"', '│'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_fleet_details_readable(width: u16, height: u16, monochrome: bool) {
    let mut app = partial_app();
    // No visible home and an active search cannot prevent reading fleet details.
    app.searching = true;
    for character in "missing".chars() {
        app.push_filter(character);
    }
    assert!(app.selected.is_none());
    let expected = app.fleet_assessment_at(NOW);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let overview = page(&mut terminal, &app, monochrome);
    assert!(overview.contains("?/F1 details"), "{overview}");
    assert_eq!(press(&mut app, KeyCode::F(1)), Action::Redraw);
    let mut text = String::new();
    for _ in 0..100 {
        text.push_str(&page(&mut terminal, &app, monochrome));
        text.push(' ');
        let before = app.fleet_scroll;
        assert_eq!(press(&mut app, KeyCode::Down), Action::Redraw);
        if app.fleet_scroll == before {
            break;
        }
    }
    for expected in [
        "fleet forecast",
        "Fleet partial 1/2",
        "inspection unavailable",
        "Limiting windows",
        "work transferable",
        "earliest reset",
        "window-average pace",
        "equal quotas",
        "periodic approximation",
        "Banked resets",
        "not modeled",
        "Horizon",
        "7d",
    ] {
        assert!(
            text.contains(expected),
            "{width}x{height}: missing {expected}: {text}"
        );
    }
    assert_eq!(app.fleet_assessment_at(NOW), expected);
    assert_eq!(app.filter, "missing");
    assert!(app.searching);
    assert!(app.selected.is_none());
    let end = app.fleet_scroll;
    assert_eq!(press(&mut app, KeyCode::Home), Action::Redraw);
    assert_eq!(app.fleet_scroll, 0);
    press(&mut app, KeyCode::End);
    assert_eq!(app.fleet_scroll, end);
    if height == 12 {
        assert!(app.fleet_scroll > 0);
    }
    assert!(page(&mut terminal, &app, monochrome).contains("Horizon"));
}

#[test]
fn complete_fleet_qualifications_are_readable_at_supported_sizes_and_without_color() {
    for (width, height) in [(46, 12), (80, 20), (120, 30)] {
        for monochrome in [false, true] {
            assert_fleet_details_readable(width, height, monochrome);
        }
    }
}

#[test]
fn fleet_shortcuts_and_refresh_preserve_the_search_and_home_selection() {
    let mut app = partial_app();
    app.searching = true;
    assert_eq!(press(&mut app, KeyCode::Char('?')), Action::Redraw);
    assert!(!app.fleet_details_open);
    let selected = app.selected;
    let filter = app.filter.clone();
    assert_eq!(press(&mut app, KeyCode::F(1)), Action::Redraw);
    assert!(app.fleet_details_open);
    assert_eq!(press(&mut app, KeyCode::Char('r')), Action::Refresh);
    assert_eq!(press(&mut app, KeyCode::Esc), Action::Redraw);
    assert!(!app.fleet_details_open);
    assert_eq!(app.selected, selected);
    assert_eq!(app.filter, filter);
    assert!(app.searching);
    app.searching = false;
    assert_eq!(press(&mut app, KeyCode::Char('?')), Action::Redraw);
    assert!(app.fleet_details_open);
    assert_eq!(press(&mut app, KeyCode::Char('?')), Action::Redraw);
    assert!(!app.fleet_details_open);
}

#[test]
fn fleet_details_controls_never_launch_a_home_or_change_its_focus() {
    for key in [
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Esc,
        KeyCode::Char('q'),
    ] {
        let mut app = partial_app();
        app.focus = Focus::Details;
        press(&mut app, KeyCode::F(1));
        assert_eq!(press(&mut app, key), Action::Redraw);
        assert!(!app.fleet_details_open);
        assert_eq!(app.focus, Focus::Details);
        assert_eq!(app.selected, Some(0));
        assert!(app.exit_state.is_none());
    }
    let mut app = partial_app();
    press(&mut app, KeyCode::F(1));
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ),
        Action::Cancel
    );
}

#[test]
fn other_pickers_keep_their_existing_shortcuts_and_search_text() {
    let mut app = App::new("Example Picker", homes(), Vec::new());
    assert_eq!(press(&mut app, KeyCode::F(1)), Action::Ignore);
    assert_eq!(press(&mut app, KeyCode::Char('?')), Action::Ignore);
    assert!(!app.fleet_details_open);
    app.searching = true;
    assert_eq!(press(&mut app, KeyCode::Char('?')), Action::Redraw);
    assert_eq!(app.filter, "?");
}
