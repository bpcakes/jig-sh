use super::*;
use crate::claude::usage::normalize_limits;

fn details(raw: Value) -> Value {
    json!({
        "account": {"type": "Claude"},
        "rate_limits": normalize_limits(&raw).unwrap(),
    })
}

#[test]
fn claude_scoped_usage_with_nullable_resets_is_scored_without_losing_exhaustion() {
    let now = chrono::Utc::now().timestamp();
    for used in [0.0, 20.0, 100.0] {
        let observed = details(json!({
            "five_hour":{"utilization":0},
            "seven_day":{"utilization":0,"resets_at":null},
            "limits":[{"kind":"weekly_scoped","percent":used,"resets_at":null,
                "is_active":false,"scope":{"model":{"id":null,"display_name":"Fable"}}}]
        }));
        let result = assessment::assess(&observed, "claude", now);
        assert_eq!(result.remaining_percent, Some(100.0 - used));
        assert_eq!(
            result.reason,
            if used < 100.0 {
                Reason::Eligible
            } else {
                Reason::Exhausted
            }
        );
        let window = result.limiting_window.unwrap();
        assert_eq!(window.resets_at, None);
        if used > 0.0 {
            assert_eq!(window.bucket_name.as_deref(), Some("Fable"));
        }
    }
}

#[test]
fn malformed_claude_quotas_remain_visible_but_cannot_be_selected() {
    for extra in [
        json!({"seven_day_sonnet":{"utilization":"invalid"}}),
        json!({"seven_day":{"utilization":null}}),
        json!({"seven_day_opus":42}),
        json!({"limits":{}}),
        json!({"limits":[null]}),
        json!({"limits":[{"percent":0}]}),
        json!({"limits":[{"kind":5,"percent":0}]}),
        json!({"limits":[{"kind":"weekly_scoped","percent":null}]}),
    ] {
        let mut raw = json!({"five_hour":{"utilization":25}});
        raw.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let observed = details(raw);
        assert_eq!(observed["rate_limits"][0]["primary"]["used_percent"], 25.0);
        let result = assessment::assess(&observed, "claude", chrono::Utc::now().timestamp());
        assert_eq!(result.reason, Reason::InvalidUsage, "{observed}");
        assert_eq!(result.remaining_percent, None);
    }
}

#[test]
fn malformed_claude_resets_are_distinct_from_absent_or_expired_resets() {
    let now = chrono::Utc::now().timestamp();
    for reset in [json!("invalid"), json!(now + 60), json!({}), json!(false)] {
        let observed = details(json!({"five_hour":{"utilization":25,"resets_at":reset}}));
        assert_eq!(observed["rate_limits"][0]["primary"]["used_percent"], 25.0);
        assert_eq!(
            assessment::assess(&observed, "claude", now).reason,
            Reason::InvalidUsage
        );
        assert!(!observed.to_string().contains("\"resets_at\":\"invalid\""));
    }
    for (seconds, expected) in [(60, Reason::Eligible), (-1, Reason::StaleUsage)] {
        let reset = chrono::DateTime::from_timestamp(now + seconds, 0)
            .unwrap()
            .to_rfc3339();
        let observed = details(json!({"five_hour":{"utilization":25,"resets_at":reset}}));
        assert_eq!(
            assessment::assess(&observed, "claude", now).reason,
            expected
        );
    }
}
