use chrono::DateTime;
use serde_json::{Value, json};

pub(super) fn limits(value: &Value) -> Result<Vec<Value>, String> {
    for key in [
        "five_hour",
        "seven_day",
        "seven_day_sonnet",
        "seven_day_opus",
        "seven_day_oauth_apps",
    ] {
        if !value[key].is_null() && window(&value[key], 1).is_null() {
            return Err("Claude returned a malformed subscription usage window".into());
        }
    }
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
    // Current Claude clients receive model-specific allowances (including
    // Fable) as weekly_scoped rows. The model ID may be null; neither that nor
    // is_active=false means the quota can be omitted from account selection.
    if !value["limits"].is_null() {
        let rows = value["limits"]
            .as_array()
            .ok_or("Claude returned malformed scoped usage limits")?;
        for (index, row) in rows.iter().enumerate() {
            let kind = row["kind"]
                .as_str()
                .ok_or("Claude returned a malformed usage row")?;
            if kind != "weekly_scoped" {
                continue;
            }
            let normalized = window(
                &json!({
                    "utilization": row["percent"],
                    "resets_at": row["resets_at"],
                }),
                10_080,
            );
            if normalized.is_null() {
                return Err("Claude returned a malformed scoped usage window".into());
            }
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
