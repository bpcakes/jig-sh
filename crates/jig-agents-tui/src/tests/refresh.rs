//! Inspection progress, and refreshing usage without reopening the picker.

use super::*;

const NOW: u64 = 2_000_000_000;

/// Two homes whose first inspection round has finished.
fn inspected() -> App {
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);
    app.apply_update_at(projected_update(1, 60.0, 10_080, 0.5, NOW), NOW);
    app.finish_inspection(None);
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn screen(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render::draw_at(frame, app, NOW))
        .unwrap();
    terminal.backend().to_string()
}

#[test]
fn refresh_is_offered_only_after_inspection_finishes() {
    let mut app = app(homes());
    assert!(!app.can_refresh());
    assert_eq!(
        handle_key(&mut app, key(KeyCode::Char('r'))),
        Action::Ignore
    );
    assert!(!screen(&app, 120, 30).contains("r  refresh"));

    let mut app = inspected();
    assert!(app.can_refresh());
    assert!(screen(&app, 120, 30).contains("r  refresh"));
    assert_eq!(
        handle_key(&mut app, key(KeyCode::Char('r'))),
        Action::Refresh
    );

    // While searching, r is part of the search text.
    handle_key(&mut app, key(KeyCode::Char('/')));
    assert_eq!(
        handle_key(&mut app, key(KeyCode::Char('r'))),
        Action::Redraw
    );
    assert_eq!(app.filter, "r");

    app.begin_exit(ExitState::Launching);
    assert!(!app.can_refresh());
}

#[test]
fn refresh_keeps_each_previous_sample_until_its_new_one_arrives() {
    let mut app = inspected();
    let before = app.rows[1].primary_usage();
    app.begin_refresh();

    assert!(!app.inspection_finished);
    assert!(!app.can_refresh());
    assert_eq!(app.completed, 0);
    assert!(app.rows.iter().all(|row| row.refreshing));
    assert_eq!(app.rows[1].primary_usage(), before);
    let refreshing = screen(&app, 120, 30);
    assert!(
        refreshing.contains("Refreshing accounts and usage  0/2"),
        "{refreshing}"
    );
    assert!(refreshing.contains("refreshing…"), "{refreshing}");
    assert!(refreshing.contains("⠋ "), "{refreshing}");

    let mut update = projected_update(1, 80.0, 10_080, 0.5, NOW + 60);
    update.details["account"]["email"] = json!("work@example.com");
    app.apply_update_at(update.clone(), NOW + 60);
    app.apply_update_at(update, NOW + 60);
    assert_eq!(app.completed, 1, "a repeated update counts once");
    assert!(!app.rows[1].refreshing);
    assert!(app.rows[0].refreshing);
    assert_eq!(
        app.rows[1].primary_usage().unwrap().1,
        [(WindowRole::Weekly, Some(80.0))]
    );

    // A home the refresh did not reach keeps its previous sample.
    app.finish_inspection(Some("inspection worker stopped".into()));
    assert!(!app.rows[0].refreshing);
    assert!(matches!(app.rows[0].inspection(), Inspection::Ready(_)));
    assert!(app.can_refresh());
}

#[test]
fn refresh_retries_homes_whose_inspection_stopped() {
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 25.0, 10_080, 0.5, NOW), NOW);
    app.finish_inspection(Some("inspection worker stopped".into()));
    assert!(matches!(app.rows[1].inspection(), Inspection::Unavailable));
    assert!(app.inspection_error.is_some());

    app.begin_refresh();
    assert!(matches!(app.rows[1].inspection(), Inspection::Loading));
    assert!(!app.rows[1].refreshing);
    assert_eq!(app.inspection_error, None, "the new round starts clean");
}

#[test]
fn static_configurations_never_refresh() {
    let mut app = App::configuration(
        "Example Picker",
        homes()
            .into_iter()
            .map(|home| crate::ConfigurationHome {
                home,
                details: Vec::new(),
            })
            .collect(),
        Vec::new(),
    );
    assert!(!app.can_refresh());
    assert_eq!(
        handle_key(&mut app, key(KeyCode::Char('r'))),
        Action::Ignore
    );
    assert!(!screen(&app, 120, 30).contains("refresh"));
}

#[test]
fn update_arriving_after_worker_completion_repairs_progress() {
    let mut app = app(homes());
    app.finish_inspection(None);
    assert!(matches!(app.rows[0].inspection(), Inspection::Unavailable));

    app.apply_update(ready_update(0));

    assert_eq!(app.completed, 1);
    assert!(matches!(app.rows[0].inspection(), Inspection::Ready(_)));
}
