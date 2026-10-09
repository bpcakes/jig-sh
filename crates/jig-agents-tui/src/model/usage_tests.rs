use serde_json::json;

use super::*;

#[test]
fn subscription_windows_are_ordered_by_duration_and_named_by_role() {
    use WindowRole::{DurationMinutes, FiveHour, Weekly, Window};
    for id in ["codex", "claude"] {
        for (primary, secondary, expected) in [
            (
                json!({"used_percent":20,"duration_minutes":10080}),
                json!({"used_percent":10,"duration_minutes":300}),
                vec![(FiveHour, 10.0), (Weekly, 20.0)],
            ),
            (
                json!({"used_percent":10,"duration_minutes":300}),
                json!({"used_percent":20,"duration_minutes":300}),
                vec![(FiveHour, 10.0), (FiveHour, 20.0)],
            ),
            (
                json!({"used_percent":0,"duration_minutes":120}),
                json!(null),
                vec![(DurationMinutes(120), 0.0)],
            ),
            (
                json!({"used_percent":101}),
                json!(null),
                vec![(Window, 101.0)],
            ),
        ] {
            let bucket = RateLimitBucket::from_value(
                &json!({"id":id,"primary":primary,"secondary":secondary}),
                &["codex".into(), "claude".into()],
            )
            .unwrap();
            let windows = (0..bucket.windows.len())
                .map(|index| {
                    (
                        bucket.window_role(index),
                        bucket.windows[index].used_percent.unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(windows, expected);
        }
    }
    let bucket = RateLimitBucket::from_value(
        &json!({"id":"other","primary":{"used_percent":0,"duration_minutes":300}}),
        &["codex".into(), "claude".into()],
    )
    .unwrap();
    assert_eq!(bucket.label(), "other");
    assert_eq!(bucket.window_role(0), WindowRole::Window);
}

#[test]
fn invalid_usage_stays_unknown_and_tui_keeps_reset_status() {
    for used_percent in [
        None,
        Some(-1.0),
        Some(f64::NAN),
        Some(f64::INFINITY),
        Some(f64::NEG_INFINITY),
    ] {
        let window = RateLimitWindow {
            used_percent,
            duration_minutes: Some(0),
            resets_at: Some(0),
        };
        assert_eq!(window.usage_amounts(), "usage unavailable");
        assert_eq!(window.usage_detail(), "usage unavailable · 0m window");
        assert_eq!(window.reset_label_at(100), "reset due");
        assert!(matches!(
            window.projection_at(100),
            WindowProjection::Unavailable
        ));
    }
    let window = RateLimitWindow {
        used_percent: Some(125.0),
        duration_minutes: None,
        resets_at: None,
    };
    assert_eq!(window.usage_amounts(), "125% used · 0% left");
    assert_eq!(window.usage_detail(), "125% used · 0% left · ? window");
    assert_eq!(window.reset_label_at(100), "reset unknown");
}
