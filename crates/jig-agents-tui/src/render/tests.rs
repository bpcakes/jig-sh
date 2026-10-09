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
                .draw(|frame| {
                    draw_list(
                        frame,
                        frame.area(),
                        &View {
                            app: &app,
                            theme: Theme::default(),
                            now: 0,
                            best: None,
                        },
                    )
                })
                .unwrap();
            let screen = terminal.backend().to_string();
            let selected_line = screen.lines().find(|line| line.contains('›')).unwrap();
            assert!(selected_line.contains("keep-18"), "{screen}");
            // Twelve rows less the borders and the column header.
            assert!(app.list_offset_for_viewport(9) > 0);
        }

        app.move_selection(-1);
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| {
                draw_list(
                    frame,
                    frame.area(),
                    &View {
                        app: &app,
                        theme: Theme::default(),
                        now: 0,
                        best: None,
                    },
                )
            })
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

fn side_by_side(layout: &layout::PickerLayout) -> bool {
    matches!((layout.list, layout.details), (Some(list), Some(details)) if details.x > list.x)
}

#[test]
fn side_by_side_panes_keep_readable_widths() {
    for rows in [4, 12] {
        let app = example_app(rows);
        for height in [MIN_HEIGHT, 30, 101] {
            for width in MIN_WIDTH..=260 {
                let layout = layout::picker_layout(Rect::new(0, 0, width, height), &app);
                let size = format!("{rows} homes at {width}x{height}");
                if side_by_side(&layout) {
                    let (list, details) = (layout.list.unwrap(), layout.details.unwrap());
                    assert!(list.width >= 64, "{size}: {list:?}");
                    assert!((56..=72).contains(&details.width), "{size}: {details:?}");
                } else if let Some(list) = layout.list {
                    assert_eq!(list.width, width, "{size}");
                }
            }
        }
    }
}

#[test]
fn panes_stack_whenever_the_details_keep_a_comfortable_height() {
    // A tall, narrow multiplexer pane: wide enough to split, but stacking
    // shows everything at full width.
    for (homes, width, height) in [(4, 109, 101), (4, 200, 60), (4, 100, 40), (12, 130, 60)] {
        let layout = layout::picker_layout(Rect::new(0, 0, width, height), &example_app(homes));
        assert!(!side_by_side(&layout), "{homes} homes at {width}x{height}");
        let details = layout.details.unwrap();
        assert_eq!(details.width, width);
        assert!(
            details.height >= 20,
            "{homes} homes at {width}x{height}: {details:?}"
        );
    }
    // Too short to stack comfortably, and wide enough for both panes.
    for (homes, width, height) in [(4, 130, 26), (12, 160, 30), (4, 200, 22)] {
        let layout = layout::picker_layout(Rect::new(0, 0, width, height), &example_app(homes));
        assert!(side_by_side(&layout), "{homes} homes at {width}x{height}");
    }
    // Too short to stack comfortably, but too narrow to split: squeeze the stack.
    let layout = layout::picker_layout(Rect::new(0, 0, 110, 22), &example_app(4));
    assert!(!side_by_side(&layout));
    assert!(layout.details.unwrap().height < 20);
}

#[test]
fn searching_never_changes_the_arrangement() {
    for (width, height) in [(130, 30), (130, 31), (130, 32), (109, 101), (80, 24)] {
        let mut app = example_app(12);
        let before = side_by_side(&layout::picker_layout(Rect::new(0, 0, width, height), &app));
        app.searching = true;
        for character in "example-1".chars() {
            app.push_filter(character);
            let during = layout::picker_layout(Rect::new(0, 0, width, height), &app);
            assert_eq!(side_by_side(&during), before, "{width}x{height}");
        }
    }
}

#[test]
fn stacked_list_fits_its_rows_and_leaves_the_rest_to_details() {
    let area = Rect::new(0, 0, 80, 24);
    let layout = layout::picker_layout(area, &example_app(2));
    let (list, details) = (layout.list.unwrap(), layout.details.unwrap());
    assert_eq!(list.height, 3 + 2 * 2);
    assert_eq!(details.y, list.bottom());
    // One header row and one footer row leave 22 for the panes.
    assert_eq!(details.height, 22 - list.height);

    let layout = layout::picker_layout(area, &example_app(30));
    let (list, details) = (layout.list.unwrap(), layout.details.unwrap());
    assert_eq!(list.height, 13, "a long list keeps to its share");
    assert_eq!(details.height, 9);
}

#[test]
fn short_narrow_terminals_show_only_the_focused_pane() {
    let mut app = example_app(4);
    let area = Rect::new(0, 0, 80, 12);
    let layout = layout::picker_layout(area, &app);
    assert_eq!(layout.list.map(|list| list.height), Some(10));
    assert!(layout.details.is_none());

    app.toggle_focus();
    let layout = layout::picker_layout(area, &app);
    assert!(layout.list.is_none());
    assert_eq!(layout.details.map(|details| details.height), Some(10));

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal.draw(|frame| draw_at(frame, &app, 0)).unwrap();
    let screen = terminal.backend().to_string();
    // Only the details show, titled with the home; the footer names the mode.
    assert!(screen.contains("╭ example-0 "), "{screen}");
    assert!(!screen.contains("╭ homes"), "{screen}");
    assert!(screen.contains("Tab  homes"), "{screen}");
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
        let list = screen.split("╰").next().unwrap();
        if width >= 60 {
            // Every window's used quota sits beside its meter.
            for used in ["42%", "18%", "12%", "61%", "80%", "30%", "5%", "9%"] {
                assert!(list.contains(used), "{width}: {screen}");
            }
        }
        // The projection names its window.
        for projection in ["~30% left", "out ~1.2d early", "out ~1.2h early"] {
            assert!(list.contains(projection), "{width}: {screen}");
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
    assert!(position("claude usage") < position("Account  "));
    assert!(position("Account  ") < position("/tmp/ExampleHome/.claude "));
    // A role that names its duration does not repeat it.
    assert!(screen.contains("42% used · resets in 2h"), "{screen}");
    assert!(!screen.contains("5h window"), "{screen}");
}

#[test]
fn wrapped_details_hang_under_their_value() {
    let lines = wrap::wrap(
        labeled(
            "CLAUDE_CONFIG_DIR",
            "/tmp/ExampleHome/a-long-configuration-directory",
        ),
        30,
    );
    let rows = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            "CLAUDE_CONFIG_DIR  /tmp/Exampl",
            "               eHome/a-long-co",
            "               nfiguration-dir",
            "               ectory",
        ]
    );

    let rows = wrap::wrap(labeled("Usage sample", "just now · reopen to refresh"), 30)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            "Usage sample  just now ·",
            "              reopen to",
            "              refresh"
        ]
    );

    let rows = wrap::wrap(labeled("Name", "界界界界界界"), 9)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rows, ["Name  界", "    界界", "    界界", "    界"]);

    // One cell short of a double-width character moves it to the next row.
    let rows = wrap::wrap(labeled("Name", "a界界界"), 8)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rows, ["Name  a", "    界界", "    界"]);
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
            .last()
            .unwrap()
            .to_owned()
    };
    // Each hint is a ` key ` chip followed by its action.
    let wide = footer(120);
    for hint in [
        " ↑↓  move",
        " /  search",
        " Tab  details",
        " Enter  launch",
        " Esc/q  cancel",
    ] {
        assert!(wide.contains(hint), "{wide}");
    }
    let narrow = footer(MIN_WIDTH);
    assert!(narrow.contains("Enter  launch"), "{narrow}");
    assert!(narrow.contains("Esc/q  cancel"), "{narrow}");
    assert!(!narrow.contains("Tab  details"), "{narrow}");
}

#[test]
fn the_focused_pane_has_an_accent_border() {
    const NOW: u64 = 1_800_000_000;
    let mut app = claude_app(NOW);
    let layout = layout::picker_layout(Rect::new(0, 0, 130, 26), &app);
    assert!(side_by_side(&layout));
    let (top, details_x) = (layout.list.unwrap().y, layout.details.unwrap().x);
    let border = |terminal: &Terminal<TestBackend>, x| {
        terminal.backend().buffer().cell((x, top)).unwrap().fg
    };
    let terminal = render(&app, 130, 26, NOW);
    assert_eq!(border(&terminal, 0), Theme::default().accent());
    assert_ne!(border(&terminal, details_x), Theme::default().accent());

    app.toggle_focus();
    let terminal = render(&app, 130, 26, NOW);
    assert_ne!(border(&terminal, 0), Theme::default().accent());
    assert_eq!(border(&terminal, details_x), Theme::default().accent());
}

fn labeled(label: &str, value: &str) -> wrap::Detail {
    use unicode_width::UnicodeWidthStr;
    wrap::Detail::labeled(
        label,
        value,
        label.width(),
        ratatui::style::Style::default(),
    )
}

#[test]
fn no_color_keeps_every_glyph_and_status_without_any_color() {
    use ratatui::style::Color;
    const NOW: u64 = 1_800_000_000;
    let app = claude_app(NOW);
    let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
    terminal
        .draw(|frame| draw_with(frame, &app, Theme::new(theme::ColorDepth::Monochrome), NOW))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer
            .content
            .iter()
            .all(|cell| matches!(cell.fg, Color::Reset) && matches!(cell.bg, Color::Reset)),
        "{}",
        terminal.backend()
    );
    let screen = terminal.backend().to_string();
    for text in ["›", "◆", "●", "━", "█", "│", "out ~1.2d early", "42% used"] {
        assert!(screen.contains(text), "missing {text}: {screen}");
    }
}
