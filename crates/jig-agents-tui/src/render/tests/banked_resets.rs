use serde_json::{Value, json};

use super::{example_app, render};
use crate::HomeUpdate;

#[test]
fn account_details_show_banked_resets_in_each_layout() {
    for width in [50, 90, 160] {
        for (count, expected) in [(json!(7), "7"), (json!(0), "0"), (Value::Null, "unknown")] {
            let mut app = example_app(1);
            app.apply_update_at(
                HomeUpdate {
                    index: 0,
                    details: json!({"account": {"type": "chatgpt"}, "banked_resets": count}),
                },
                100,
            );
            let screen = render(&app, width, 40, 100).backend().to_string();
            let line = screen
                .lines()
                .find(|line| line.contains("Banked resets"))
                .unwrap_or_else(|| panic!("{screen}"));
            assert!(line.contains(expected), "{screen}");
        }
    }
}

#[test]
fn banked_resets_follow_the_selected_account_and_its_latest_inspection() {
    let mut app = example_app(2);
    app.apply_update_at(
        HomeUpdate {
            index: 0,
            details: json!({"account": {"type": "chatgpt"}, "banked_resets": 7}),
        },
        100,
    );
    app.move_selection(1);
    for (details, expected) in [
        (json!({"banked_resets": 3}), Some("3")),
        (json!({"banked_resets": 0}), Some("0")),
        (json!({"banked_resets": null}), Some("unknown")),
        // Providers without this field do not get a banked-resets row.
        (json!({"account": {"type": "oauth"}}), None),
    ] {
        app.apply_update_at(HomeUpdate { index: 1, details }, 101);
        let screen = render(&app, 90, 40, 101).backend().to_string();
        let line = screen.lines().find(|line| line.contains("Banked resets"));
        match expected {
            Some(count) => assert!(line.is_some_and(|line| line.contains(count)), "{screen}"),
            None => assert!(line.is_none(), "{screen}"),
        }
    }
    app.move_selection(-1);
    let screen = render(&app, 90, 40, 101).backend().to_string();
    assert!(
        screen
            .lines()
            .any(|line| line.contains("Banked resets") && line.contains('7')),
        "{screen}"
    );
}
