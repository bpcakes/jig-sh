use super::*;

#[test]
fn missing_duration_preserves_known_usage_with_or_without_reset() {
    let now = chrono::Utc::now().timestamp();
    for omit_duration in [false, true] {
        for reset in [None, Some(Value::Null), Some(json!(now + 60))] {
            for used in [0.0, 40.0, 100.0] {
                let mut details = usage(used, used);
                for key in ["primary", "secondary"] {
                    let window = details["rate_limits"][0][key].as_object_mut().unwrap();
                    window.insert("duration_minutes".into(), Value::Null);
                    if omit_duration {
                        window.remove("duration_minutes");
                    }
                    window.remove("resets_at");
                    if let Some(reset) = &reset {
                        window.insert("resets_at".into(), reset.clone());
                    }
                }
                let result = assessment::assess(&details, "example", now);
                assert_eq!(result.remaining_percent, Some(100.0 - used));
                assert_eq!(
                    result.reason,
                    if used < 100.0 {
                        Reason::Eligible
                    } else {
                        Reason::Exhausted
                    }
                );
                let serialized = serde_json::to_value(result).unwrap();
                assert!(serialized["limiting_window"]["duration_minutes"].is_null());
                assert_eq!(
                    serialized["limiting_window"]["resets_at"],
                    reset.clone().unwrap_or(Value::Null)
                );
            }
        }
    }
}

#[test]
fn absent_duration_does_not_disable_reset_or_utilization_validation() {
    let now = chrono::Utc::now().timestamp();
    for (field, value, reason) in [
        ("resets_at", json!(now), Reason::StaleUsage),
        ("resets_at", json!(now - 1), Reason::StaleUsage),
        ("resets_at", json!("invalid"), Reason::InvalidUsage),
        ("used_percent", Value::Null, Reason::InvalidUsage),
    ] {
        let mut details = usage(0.0, 0.0);
        details["rate_limits"][0]["primary"]["duration_minutes"] = Value::Null;
        details["rate_limits"][0]["primary"][field] = value;
        assert_eq!(assessment::assess(&details, "example", now).reason, reason);
    }
}

#[test]
fn malformed_durations_are_not_treated_as_absent_even_without_reset() {
    let now = chrono::Utc::now().timestamp();
    for duration in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("300"),
        json!(false),
        json!({}),
        json!(i64::MAX),
        json!(u64::MAX),
    ] {
        let mut details = usage(0.0, 0.0);
        details["rate_limits"][0]["primary"]["resets_at"] = Value::Null;
        details["rate_limits"][0]["primary"]["duration_minutes"] = duration;
        assert_eq!(
            assessment::assess(&details, "example", now).reason,
            Reason::InvalidUsage
        );
    }
}
