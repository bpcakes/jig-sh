use chrono::DateTime;
use serde_json::{Value, json};

use super::SUBSCRIPTION_BUCKET;

pub(crate) fn limits(value: &Value) -> Result<Vec<Value>, String> {
    let mut buckets = Vec::new();
    let primary = subscription_window(&value["five_hour"], 300);
    let secondary = subscription_window(&value["seven_day"], 10_080);
    if !primary.is_null() || !secondary.is_null() {
        buckets.push(
            json!({"id":SUBSCRIPTION_BUCKET, "name":"Claude", "primary":primary, "secondary":secondary}),
        );
    }
    for (key, name) in [
        ("seven_day_sonnet", "Sonnet"),
        ("seven_day_opus", "Opus"),
        ("seven_day_oauth_apps", "OAuth apps"),
    ] {
        let window = window(&value[key], 10_080);
        if !window.is_null() {
            buckets.push(json!({"id":key, "name":name, "primary":window, "secondary":null}));
        }
    }
    // Current Claude clients receive model-specific allowances (including
    // Fable) as weekly_scoped rows. The model ID may be null; neither that nor
    // is_active=false means the quota can be omitted from account selection.
    if !value["limits"].is_null() {
        // Preserve an unknown window for malformed containers or rows so display
        // keeps the valid quotas, while selection cannot mistake them for absent.
        let unknown_rows = [Value::Null];
        let rows = value["limits"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&unknown_rows);
        for (index, row) in rows.iter().enumerate() {
            let kind = row["kind"].as_str();
            if kind.is_some_and(|kind| kind != "weekly_scoped") {
                continue;
            }
            let normalized = window(
                &json!({
                    "utilization": if kind.is_some() { &row["percent"] } else { &Value::Null },
                    "resets_at": row["resets_at"],
                }),
                10_080,
            );
            let name = row["scope"]["model"]["display_name"]
                .as_str()
                .or_else(|| row["scope"]["surface"]["display_name"].as_str())
                .unwrap_or("Scoped weekly");
            buckets.push(json!({
                "id": format!("weekly_scoped_{index}"),
                "name": name,
                "primary": normalized,
                "secondary": null,
            }));
        }
    }
    if buckets.is_empty() {
        return Err("Claude returned no supported subscription usage windows".into());
    }
    Ok(buckets)
}

/// A subscription window, marked `started: false` when the endpoint reports zero utilization
/// with an explicitly null reset: the shape observed for an account that has not used the
/// window since it last ended. The endpoint documents neither field, so only that exact shape
/// is marked, and an omitted or unreadable reset stays unknown.
fn subscription_window(value: &Value, duration: u64) -> Value {
    let mut normalized = window(value, duration);
    if value["utilization"].as_f64() == Some(0.0) && value.get("resets_at") == Some(&Value::Null) {
        normalized["started"] = json!(false);
    }
    normalized
}

fn window(value: &Value, duration: u64) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    let used = value["utilization"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0);
    let resets_at = value["resets_at"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.timestamp());
    let mut normalized =
        json!({"used_percent":used, "duration_minutes":duration, "resets_at":resets_at});
    if used.is_none() || (!value["resets_at"].is_null() && resets_at.is_none()) {
        normalized["invalid"] = json!(true);
    }
    normalized
}

pub(super) fn is_partial(limits: &Value) -> bool {
    limits.as_array().is_some_and(|buckets| {
        buckets.iter().any(|bucket| {
            ["primary", "secondary"]
                .iter()
                .any(|key| bucket[key]["invalid"] == true)
        })
    })
}
