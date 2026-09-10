use serde_json::json;

use super::*;

#[test]
fn subscription_windows_preserve_tui_labels_and_order() {
    for id in ["codex", "claude"] {
        for (primary, secondary, expected) in [
            (
                json!({"used_percent":20,"duration_minutes":10080}),
                json!({"used_percent":10,"duration_minutes":300}),
                "5h 90% left, weekly 80% left",
            ),
            (
                json!({"used_percent":10,"duration_minutes":300}),
                json!({"used_percent":20,"duration_minutes":300}),
                "5h 90% left, 5h 80% left",
            ),
            (
                json!({"used_percent":0,"duration_minutes":120}),
                json!(null),
                "2h 100% left",
            ),
            (json!({"used_percent":101}), json!(null), "? 0% left"),
        ] {
            let bucket = RateLimitBucket::from_value(
                &json!({"id":id,"primary":primary,"secondary":secondary}),
            )
            .unwrap();
            assert_eq!(bucket.summary(), expected);
        }
    }
    let bucket = RateLimitBucket::from_value(
        &json!({"id":"other","primary":{"used_percent":0,"duration_minutes":300}}),
    )
    .unwrap();
    assert_eq!(bucket.summary(), "other 5h 100% left");
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
        assert_eq!(window.remaining(), "remaining unavailable");
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
    assert_eq!(window.remaining(), "0% left");
    assert_eq!(window.usage_detail(), "125% used · 0% left · ? window");
    assert_eq!(window.reset_label_at(100), "reset unknown");
}
