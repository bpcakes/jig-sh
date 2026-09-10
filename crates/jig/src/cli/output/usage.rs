use jig_codex_tui::usage::{
    WindowRole, format_duration, is_subscription_bucket, remaining_percent, valid_used_percent,
};
use jig_tui::{format_countdown, format_percent, sanitize_text};

use super::value_str;

pub(super) fn format_limits(value: &serde_json::Value) -> String {
    let Some(buckets) = value.as_array() else {
        return "usage unavailable".into();
    };
    if buckets.is_empty() {
        return "usage unavailable".into();
    }
    buckets
        .iter()
        .map(|bucket| {
            let is_subscription = value_str(bucket, "id").is_some_and(is_subscription_bucket);
            let label = sanitize_text(
                value_str(bucket, "name")
                    .or_else(|| value_str(bucket, "id"))
                    .unwrap_or("limit"),
            );
            let mut windows = [&bucket["primary"], &bucket["secondary"]]
                .into_iter()
                .filter(|window| window.is_object())
                .collect::<Vec<_>>();
            windows.sort_by_key(|window| {
                window
                    .get("duration_minutes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX)
            });
            match windows.as_slice() {
                [window] if is_subscription => {
                    format!("{label}: {}", format_window_with_duration_role(window))
                }
                [window] => format!(
                    "{label}: {}",
                    format_window(window).expect("window was checked above")
                ),
                [first, second] if is_subscription => format!(
                    "{label}: {}, {}",
                    format_window_with_duration_role(first),
                    format_window_with_duration_role(second)
                ),
                [first, second] => format!(
                    "{label}: {}, {}",
                    format_window(first).expect("window was checked above"),
                    format_window(second).expect("window was checked above")
                ),
                [] => format!("{label}: unavailable"),
                _ => unreachable!("a normalized rate-limit bucket has at most two windows"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_window_with_duration_role(window: &serde_json::Value) -> String {
    let rendered = format_window(window).expect("window was checked above");
    match WindowRole::for_subscription(window["duration_minutes"].as_u64()) {
        Some(role @ (WindowRole::FiveHour | WindowRole::Weekly)) => format!("{role} {rendered}"),
        _ => rendered,
    }
}

pub(super) fn format_window(window: &serde_json::Value) -> Option<String> {
    let object = window.as_object()?;
    let remaining = valid_used_percent(
        object
            .get("used_percent")
            .and_then(serde_json::Value::as_f64),
    )
    .map(remaining_percent)
    .map(|remaining| format!("{} left", format_percent(remaining)))
    .unwrap_or_else(|| "remaining unavailable".into());
    let duration = object
        .get("duration_minutes")
        .and_then(serde_json::Value::as_u64)
        .map(format_duration)
        .unwrap_or_else(|| "window ?".into());
    let reset = object
        .get("resets_at")
        .and_then(serde_json::Value::as_i64)
        .and_then(format_reset)
        .map(|reset| format!(", resets in {reset}"))
        .unwrap_or_default();
    Some(format!("{remaining} ({duration}{reset})"))
}

fn format_reset(timestamp: i64) -> Option<String> {
    let timestamp = u64::try_from(timestamp).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    format_reset_from(timestamp, now)
}

pub(super) fn format_reset_from(timestamp: u64, now: u64) -> Option<String> {
    let remaining = timestamp
        .checked_sub(now)
        .filter(|remaining| *remaining > 0)?;
    Some(format_countdown(remaining))
}

#[cfg(test)]
mod tests;
