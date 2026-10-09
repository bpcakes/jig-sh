use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{ConfigurationHome, Home};

#[test]
fn filtered_selection_stays_visible_across_table_layout_changes() {
    for configuration in [false, true] {
        let homes: Vec<_> = (0..20)
            .map(|index| Home {
                path: format!("/tmp/ExampleHome-{index:02}").into(),
                name: format!(
                    "{}-{index:02}",
                    if index % 2 == 0 { "keep" } else { "omit" }
                ),
                current: index == 0,
            })
            .collect();
        let mut app = if configuration {
            App::configuration(
                "Example Picker",
                homes
                    .into_iter()
                    .map(|home| ConfigurationHome {
                        home,
                        details: Vec::new(),
                    })
                    .collect(),
                Vec::new(),
            )
        } else {
            App::codex(homes, Vec::new())
        };
        for character in "keep".chars() {
            app.push_filter(character);
        }
        app.move_to_edge(true);
        assert_eq!(app.selected, Some(18));
        assert_eq!(app.visible_indices().len(), 10);

        for width in [120, 80, 50, 120] {
            let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
            terminal
                .draw(|frame| draw_list(frame, frame.area(), &app, 0, None))
                .unwrap();
            let screen = terminal.backend().to_string();
            let selected_line = screen.lines().find(|line| line.contains('›')).unwrap();
            assert!(selected_line.contains("keep-18"), "{screen}");
            assert!(app.list_offset_for_viewport(12) > 0);
        }

        app.move_selection(-1);
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| draw_list(frame, frame.area(), &app, 0, None))
            .unwrap();
        let screen = terminal.backend().to_string();
        let selected_line = screen.lines().find(|line| line.contains('›')).unwrap();
        assert!(selected_line.contains("keep-16"), "{screen}");
        assert!(screen.contains("keep-18"), "{screen}");
    }
}

fn example_app(count: usize) -> App {
    App::codex(
        (0..count)
            .map(|index| Home {
                path: format!("/tmp/ExampleHome-{index}").into(),
                name: format!("example-{index}"),
                current: index == 0,
            })
            .collect(),
        Vec::new(),
    )
}

#[test]
fn wider_terminals_never_yield_a_poorer_list_or_cramped_details() {
    let app = example_app(4);
    let mut previous = ListStyle::Compact;
    for width in MIN_WIDTH..=260 {
        let layout = layout::picker_layout(Rect::new(0, 0, width, 30), &app);
        let list = layout.list.unwrap();
        let details = layout.details.unwrap();
        let style = ListStyle::for_width(list.width);
        assert!(style >= previous, "{width}: {style:?} after {previous:?}");
        previous = style;
        if details.x > list.x {
            assert_eq!(list.y, details.y, "{width}");
            assert!((44..=64).contains(&details.width), "{width}: {details:?}");
            assert!(style >= ListStyle::TwoLine, "{width}: {style:?}");
        } else {
            assert_eq!(list.width, width, "{width}");
            assert_eq!(details.width, width, "{width}");
        }
    }
    assert_eq!(previous, ListStyle::Full);
}

#[test]
fn stacked_list_fits_its_rows_and_leaves_the_rest_to_details() {
    let area = Rect::new(0, 0, 80, 24);
    let layout = layout::picker_layout(area, &example_app(2));
    let (list, details) = (layout.list.unwrap(), layout.details.unwrap());
    assert_eq!(list.height, 3 + 2 * 2);
    assert_eq!(details.y, list.bottom());
    assert_eq!(details.height, 20 - list.height);

    let layout = layout::picker_layout(area, &example_app(30));
    let (list, details) = (layout.list.unwrap(), layout.details.unwrap());
    assert_eq!(list.height, 12, "a long list keeps to its share");
    assert_eq!(details.height, 8);
}

#[test]
fn short_narrow_terminals_show_only_the_focused_pane() {
    let mut app = example_app(4);
    let area = Rect::new(0, 0, 80, 14);
    let layout = layout::picker_layout(area, &app);
    assert_eq!(layout.list.map(|list| list.height), Some(10));
    assert!(layout.details.is_none());

    app.toggle_focus();
    let layout = layout::picker_layout(area, &app);
    assert!(layout.list.is_none());
    assert_eq!(layout.details.map(|details| details.height), Some(10));

    let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
    terminal.draw(|frame| draw_at(frame, &app, 0)).unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Selected home  [focused]"), "{screen}");
    assert!(!screen.contains("Homes"), "{screen}");
}

/// Four inspected Claude homes with five-hour and weekly windows.
fn claude_app(now: u64) -> App {
    let homes = ["claude", "claude-work", "claude-personal", "claude-team"]
        .iter()
        .enumerate()
        .map(|(index, name)| ConfigurationHome {
            home: Home {
                path: format!("/tmp/ExampleHome/.{name}").into(),
                name: (*name).into(),
                current: index == 0,
            },
            details: vec![(
                "CLAUDE_CONFIG_DIR".into(),
                format!("/tmp/ExampleHome/.{name}"),
            )],
        })
        .collect();
    let mut app = App::provider(
        "Claude Home Picker",
        homes,
        Vec::new(),
        true,
        Some("claude"),
    );
    for (index, (five_hour, weekly)) in [(42, 18), (12, 61), (80, 30), (5, 9)]
        .into_iter()
        .enumerate()
    {
        app.apply_update_at(
            crate::HomeUpdate {
                index,
                details: serde_json::json!({
                    "account": {"type": "claude.ai", "email": format!("person{index}@example.com"), "plan_type": "max"},
                    "status": "authenticated",
                    "rate_limits": [{
                        "id": "claude",
                        "primary": {"used_percent": five_hour, "duration_minutes": 300, "resets_at": now + 7_200},
                        "secondary": {"used_percent": weekly, "duration_minutes": 10_080, "resets_at": now + 300_000}
                    }]
                }),
            },
            now,
        );
    }
    app.finish_inspection(None);
    app
}

fn render(app: &App, width: u16, height: u16, now: u64) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw_at(frame, app, now)).unwrap();
    terminal
}

#[test]
fn every_list_style_shows_whole_usage_and_names_the_projected_window() {
    const NOW: u64 = 1_800_000_000;
    let app = claude_app(NOW);
    for width in [50, 80, 104, 130, 170] {
        let screen = render(&app, width, 40, NOW).backend().to_string();
        if width >= 60 {
            for usage in [
                "5h 58% · weekly 82% left",
                "5h 88% · weekly 39% left",
                "5h 20% · weekly 70% left",
                "5h 95% · weekly 91% left",
            ] {
                assert!(screen.contains(usage), "{width}: {screen}");
            }
        }
        for projection in [
            "5h: ~30% left at reset",
            "weekly: runs out ~1.2d early",
            "5h: runs out ~1.2h early",
        ] {
            assert!(screen.contains(projection), "{width}: {screen}");
        }
        for account in ["person0@example.com", "person3@example.com"] {
            assert!(screen.contains(account), "{width}: {screen}");
        }
    }
}

#[test]
fn details_lead_with_account_and_usage_before_the_home_identity() {
    const NOW: u64 = 1_800_000_000;
    let screen = render(&claude_app(NOW), 130, 30, NOW).backend().to_string();
    let position = |text: &str| {
        screen
            .find(text)
            .unwrap_or_else(|| panic!("missing {text}: {screen}"))
    };
    assert!(position("Account: person0") < position("claude usage"));
    assert!(position("claude usage") < position("Path: /tmp/ExampleHome/.claude"));
    // A role that names its duration does not repeat it.
    assert!(
        screen.contains("5h: 42% used · 58% left · resets in 2h"),
        "{screen}"
    );
    assert!(!screen.contains("5h window"), "{screen}");
}

#[test]
fn wrapped_details_hang_under_their_value() {
    let lines = details::wrap(
        details::key_value(
            "CLAUDE_CONFIG_DIR",
            "/tmp/ExampleHome/a-long-configuration-directory",
        ),
        30,
    );
    let rows = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            "CLAUDE_CONFIG_DIR: /tmp/Exampl",
            "               eHome/a-long-co",
            "               nfiguration-dir",
            "               ectory",
        ]
    );

    let rows = details::wrap(
        details::key_value("Usage sample", "just now · reopen to refresh"),
        30,
    )
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            "Usage sample: just now ·",
            "              reopen to",
            "              refresh"
        ]
    );

    let rows = details::wrap(details::key_value("Name", "界界界界界界"), 9)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rows, ["Name: 界", "    界界", "    界界", "    界"]);

    // One cell short of a double-width character moves it to the next row.
    let rows = details::wrap(details::key_value("Name", "a界界界"), 8)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rows, ["Name: a", "    界界", "    界"]);
    assert!(
        rows.iter()
            .all(|row| unicode_width::UnicodeWidthStr::width(row.as_str()) <= 8),
        "{rows:?}"
    );
}

#[test]
fn narrow_footers_keep_launch_and_cancel_longest() {
    let app = example_app(2);
    let footer = |width| {
        render(&app, width, 20, 0)
            .backend()
            .to_string()
            .lines()
            .rev()
            .nth(1)
            .unwrap()
            .to_owned()
    };
    let wide = footer(120);
    for hint in [
        "↑/↓ j/k move",
        "/ search",
        "Tab details",
        "Enter launch",
        "Esc/q cancel",
    ] {
        assert!(wide.contains(hint), "{wide}");
    }
    let narrow = footer(MIN_WIDTH);
    assert!(narrow.contains("Enter launch"), "{narrow}");
    assert!(narrow.contains("Esc/q cancel"), "{narrow}");
    assert!(!narrow.contains("Tab details"), "{narrow}");
}

#[test]
fn the_focused_pane_has_an_accent_border() {
    const NOW: u64 = 1_800_000_000;
    let mut app = claude_app(NOW);
    let border =
        |terminal: &Terminal<TestBackend>, x| terminal.backend().buffer().cell((x, 2)).unwrap().fg;
    // At 130 columns the list spans 0..78 and the details start at column 78.
    let terminal = render(&app, 130, 30, NOW);
    assert_eq!(border(&terminal, 0), ACCENT);
    assert_ne!(border(&terminal, 78), ACCENT);

    app.toggle_focus();
    let terminal = render(&app, 130, 30, NOW);
    assert_ne!(border(&terminal, 0), ACCENT);
    assert_eq!(border(&terminal, 78), ACCENT);
}

#[test]
fn column_fitting_trims_the_widest_column_first() {
    assert_eq!(list::fit_widths(40, [(10, 4), (12, 4)]), [10, 12]);
    assert_eq!(list::fit_widths(18, [(10, 4), (12, 4)]), [9, 9]);
    assert_eq!(list::fit_widths(5, [(10, 4), (12, 4)]), [4, 4]);
}
