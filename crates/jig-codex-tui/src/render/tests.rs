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
            App::new(homes, Vec::new())
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
    App::new(
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
