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
