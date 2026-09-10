use serde_json::json;

use super::*;

#[test]
fn subscription_windows_preserve_cli_labels_and_order() {
    for id in ["codex", "claude"] {
        for (primary, secondary, expected) in [
            (
                json!({"used_percent": 20, "duration_minutes": 10080}),
                json!({"used_percent": 10, "duration_minutes": 300}),
                "5h 90% left (5h), weekly 80% left (7d)",
            ),
            (
                json!({"used_percent": 10, "duration_minutes": 300}),
                json!({"used_percent": 20, "duration_minutes": 300}),
                "5h 90% left (5h), 5h 80% left (5h)",
            ),
            (
                json!({"used_percent": 0, "duration_minutes": 120}),
                json!(null),
                "100% left (2h)",
            ),
            (
                json!({"used_percent": 101}),
                json!(null),
                "0% left (window ?)",
            ),
        ] {
            assert_eq!(
                format_limits(&json!([{"id":id,"primary":primary,"secondary":secondary}])),
                format!("{id}: {expected}")
            );
        }
    }
    assert_eq!(
        format_limits(&json!([{"id":"other","primary":{"used_percent":0,"duration_minutes":300}}])),
        "other: 100% left (5h)"
    );
}

#[test]
fn invalid_usage_stays_unknown_and_elapsed_reset_is_omitted() {
    for used in [json!(null), json!(-1), json!("25")] {
        assert_eq!(
            format_window(&json!({"used_percent":used,"duration_minutes":0,"resets_at":0}))
                .as_deref(),
            Some("remaining unavailable (0m)")
        );
    }
    assert_eq!(
        format_window(&json!({})).as_deref(),
        Some("remaining unavailable (window ?)")
    );
}
