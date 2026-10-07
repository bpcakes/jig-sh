use chrono::DateTime;
use serde_json::{Value, json};

pub(super) fn limits(value: &Value) -> Result<Vec<Value>, String> {
    let mut buckets = Vec::new();
    let primary = window(&value["five_hour"], 300);
    let secondary = window(&value["seven_day"], 10_080);
    if !primary.is_null() || !secondary.is_null() {
        buckets.push(
            json!({"id":"claude", "name":"Claude", "primary":primary, "secondary":secondary}),
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
    if buckets.is_empty() {
        return Err("Claude returned no supported subscription usage windows".into());
    }
    Ok(buckets)
}

fn window(value: &Value, duration: u64) -> Value {
    let Some(used) = value["utilization"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
    else {
        return Value::Null;
    };
    let resets_at = value["resets_at"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.timestamp());
    json!({"used_percent":used, "duration_minutes":duration, "resets_at":resets_at})
}
