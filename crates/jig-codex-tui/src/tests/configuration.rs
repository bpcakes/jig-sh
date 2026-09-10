use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use crate::{
    ConfigurationHome, Home,
    model::{App, ExitState},
    render,
    runtime::{Action, handle_key},
};

fn configuration_app() -> App {
    App::configuration(
        "Claude Home Picker",
        [true, false]
            .into_iter()
            .map(|native| ConfigurationHome {
                home: Home {
                    path: PathBuf::from("/tmp/.claude"),
                    name: if native {
                        "claude [default config]"
                    } else {
                        "claude"
                    }
                    .into(),
                    current: !native,
                },
                details: vec![(
                    "CLAUDE_CONFIG_DIR".into(),
                    if native { "unset" } else { "/tmp/.claude" }.into(),
                )],
            })
            .collect(),
        vec!["Example discovery warning".into()],
    )
}

#[test]
fn configuration_modes_keep_distinct_indices_through_navigation_and_search() {
    let mut app = configuration_app();
    assert_eq!(app.selected, Some(1));
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        Action::Redraw
    );
    assert_eq!(app.selected, Some(0));
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Select
    );
    assert_eq!(app.selected_path(), Some(PathBuf::from("/tmp/.claude")));
    handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    for character in "default".chars() {
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.visible_indices(), vec![0]);
    assert_eq!(app.selected, Some(0));
    handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
        ),
        Action::Cancel
    );
}

#[test]
fn configuration_view_reuses_layout_and_controls_without_account_inspection() {
    for (width, height) in [(120, 30), (80, 30)] {
        let app = configuration_app();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render::draw(frame, &app)).unwrap();
        let screen = terminal.backend().to_string();
        for expected in [
            "Claude Home Picker",
            "Home / Path",
            "Selected home",
            "CLAUDE_CONFIG_DIR",
            "Example discovery warning",
            "/ search",
            "Enter launch",
        ] {
            assert!(screen.contains(expected), "missing {expected}: {screen}");
        }
        for absent in ["Codex", "Account", "Projection", "Inspecting", "loading"] {
            assert!(!screen.contains(absent), "unexpected {absent}: {screen}");
        }
    }
}

#[test]
fn configuration_small_terminal_uses_its_own_title_and_cancellation() {
    let mut app = configuration_app();
    let mut terminal = Terminal::new(TestBackend::new(45, 11)).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Claude Home Picker"), "{screen}");
    assert!(screen.contains("Terminal too small"), "{screen}");
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Cancel
    );
}

#[test]
fn configuration_details_and_title_are_sanitized() {
    let mut app = App::configuration(
        "Claude\x1b[2J",
        vec![ConfigurationHome {
            home: Home {
                path: PathBuf::from("/tmp/.claude"),
                name: "claude".into(),
                current: true,
            },
            details: vec![("Config\x1b[2J".into(), "value\u{202e}".into())],
        }],
        Vec::new(),
    );
    assert!(!app.title().contains('\x1b'));
    let details = app.rows[0].configuration_details.as_ref().unwrap();
    assert!(!details[0].0.contains('\x1b'));
    assert!(!details[0].1.contains('\u{202e}'));
    app.begin_exit(ExitState::Launching);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| render::draw(frame, &app)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Launching selected home"), "{screen}");
    assert!(!screen.contains("inspection"), "{screen}");
}

#[test]
fn inspected_configurations_show_subscription_limits_and_preserve_mode_identity() {
    let entries = [true, false]
        .into_iter()
        .map(|native| ConfigurationHome {
            home: Home {
                path: "/tmp/.claude".into(),
                name: if native {
                    "claude [default config]"
                } else {
                    "claude"
                }
                .into(),
                current: native,
            },
            details: vec![(
                "CLAUDE_CONFIG_DIR".into(),
                if native { "unset" } else { "/tmp/.claude" }.into(),
            )],
        })
        .collect();
    let mut app = App::inspected_configuration("Claude Home Picker", entries, Vec::new());
    assert!(!app.inspection_finished);
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Select
    );
    let now = 1_800_000_000;
    app.apply_update_at(crate::HomeUpdate { index: 0, details: serde_json::json!({
        "account":{"type":"Claude", "plan_type":"max"}, "status":"authenticated",
        "rate_limits":[{"id":"claude","name":"Claude","primary":{"used_percent":25,"duration_minutes":300,"resets_at":now+9000},"secondary":{"used_percent":40,"duration_minutes":10080,"resets_at":now+300000}}]
    }) }, now);
    assert_eq!(app.selected, Some(0));
    assert_eq!(app.rows[1].account(), "loading…");
    assert!(
        app.rows[0].usage().contains("5h 75% left"),
        "{}",
        app.rows[0].usage()
    );
    assert!(app.rows[0].usage().contains("weekly 60% left"));
    for width in [80, 120, 200] {
        let mut terminal = Terminal::new(TestBackend::new(width, 50)).unwrap();
        terminal
            .draw(|frame| render::draw_at(frame, &app, now))
            .unwrap();
        let screen = terminal.backend().to_string();
        for expected in [
            "Claude Home Picker",
            "CLAUDE_CONFIG_DIR",
            "75%",
            "60%",
            "max",
            "Inspecting",
        ] {
            assert!(screen.contains(expected), "missing {expected}: {screen}");
        }
        assert!(!screen.contains("Codex"));
    }
    app.apply_update_at(crate::HomeUpdate { index: 1, details: serde_json::json!({
        "account":null,"status":"unknown","rate_limits":[],"inspection_error":"Keychain access denied"
    }) }, now);
    app.move_selection(1);
    assert_eq!(app.selected, Some(1));
    assert!(app.rows[1].usage().contains("Keychain access denied"));
    assert_eq!(
        handle_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Select
    );
}

#[test]
fn provider_metadata_drives_primary_quota_and_recommendations_without_known_names() {
    let now = 1_800_000_000;
    let make_entries = || {
        vec![crate::ConfigurationHome {
            home: crate::Home {
                path: "/tmp/ExampleAgent".into(),
                name: "Example Agent".into(),
                current: true,
            },
            details: Vec::new(),
        }]
    };
    for (primary, expected_usage, recommended) in [
        (Some("example-subscription"), "5h 90% left", true),
        (None, "Other 5h 0% left", false),
    ] {
        let mut app = App::provider(
            "Example Agent Picker",
            make_entries(),
            vec![],
            true,
            primary,
        );
        app.apply_update_at(crate::HomeUpdate { index: 0, details: serde_json::json!({
            "account":{"type":"example"}, "rate_limits":[
                {"id":"other","name":"Other","primary":{"used_percent":100,"duration_minutes":300,"resets_at":now+9000}},
                {"id":"example-subscription","primary":{"used_percent":10,"duration_minutes":300,"resets_at":now+9000}}
            ]
        }) }, now);
        assert_eq!(app.rows[0].usage(), expected_usage);
        assert_eq!(
            app.rows[0]
                .usage_snapshot_assessment_at(now)
                .recommendation()
                .is_some(),
            recommended
        );
    }
    let app = App::provider("Example Agent Picker", make_entries(), vec![], false, None);
    assert!(app.static_configuration);
    assert!(app.inspection_finished);
    assert_eq!(app.selected, Some(0));
}
