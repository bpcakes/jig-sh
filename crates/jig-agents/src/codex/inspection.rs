use std::ffi::OsStr;
use std::path::PathBuf;

use serde_json::{Map as JsonMap, Value as JsonValue, json};

use super::app_server::{AppServerAccountResponse, app_server_account};

pub(super) fn inspect_home(
    home: PathBuf,
    codex_bin: &OsStr,
    include_usage: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> JsonValue {
    if cancelled() {
        return inspection_failure("Codex app-server inspection was cancelled");
    }
    match app_server_account(&home, codex_bin, include_usage, cancelled) {
        Ok(response) => inspected_home_json(response, include_usage),
        Err(error) => inspection_failure(error),
    }
}

pub(super) fn inspection_failure(error: impl Into<String>) -> JsonValue {
    let error = error.into();
    json!({
        "account": null,
        "rate_limits": [],
        "status": "unknown",
        "inspection_error": error,
        "usage_error": null
    })
}

pub(super) fn inspected_home_json(
    response: AppServerAccountResponse,
    include_usage: bool,
) -> JsonValue {
    let account = match normalize_account(&response.account) {
        Ok(account) => account,
        Err(error) => return inspection_failure(error),
    };
    let rate_limits = if account.is_null() {
        Vec::new()
    } else {
        response
            .rate_limits
            .as_ref()
            .map(normalize_rate_limits)
            .unwrap_or_default()
    };
    let status = if account.is_null() {
        Some("not logged in".to_owned())
    } else {
        None
    };
    let mut usage_error = if account.is_null() {
        None
    } else {
        response.usage_error
    };
    if !account.is_null()
        && include_usage
        && usage_error.is_none()
        && !rate_limits_have_usage_data(&rate_limits)
    {
        usage_error = Some("account/rateLimits/read returned no usage data".into());
    }
    json!({
        "account": account,
        "rate_limits": rate_limits,
        "usage_included": include_usage,
        "status": status,
        "inspection_error": null,
        "usage_error": usage_error
    })
}

pub(super) fn normalize_account(result: &JsonValue) -> std::result::Result<JsonValue, String> {
    let Some(account) = result.get("account") else {
        return Err("account/read result did not include an account field".into());
    };
    if account.is_null() {
        return Ok(JsonValue::Null);
    }
    let Some(account) = account.as_object() else {
        return Err("account/read result included an invalid account field".into());
    };
    Ok(json!({
        "type": account.get("type").cloned().unwrap_or(JsonValue::Null),
        "email": account.get("email").cloned().unwrap_or(JsonValue::Null),
        "plan_type": account.get("planType").cloned().unwrap_or(JsonValue::Null)
    }))
}

pub(super) fn normalize_rate_limits(result: &JsonValue) -> Vec<JsonValue> {
    let mut buckets = result
        .get("rateLimitsByLimitId")
        .and_then(JsonValue::as_object)
        .filter(|buckets| !buckets.is_empty())
        .map(|buckets| {
            buckets
                .iter()
                .map(|(id, bucket)| normalized_bucket(id, bucket))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            result
                .get("rateLimits")
                .filter(|bucket| bucket.is_object())
                .map(|bucket| {
                    let id = bucket
                        .get("limitId")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("codex");
                    vec![normalized_bucket(id, bucket)]
                })
                .unwrap_or_default()
        });
    buckets.sort_by(|left, right| {
        left.get("id")
            .and_then(JsonValue::as_str)
            .cmp(&right.get("id").and_then(JsonValue::as_str))
    });
    buckets
}

pub(super) fn rate_limits_have_usage_data(buckets: &[JsonValue]) -> bool {
    buckets.iter().any(|bucket| {
        ["primary", "secondary"].into_iter().any(|window| {
            bucket
                .get(window)
                .and_then(|window| window.get("used_percent"))
                .and_then(JsonValue::as_f64)
                .is_some()
        })
    })
}

pub(super) fn normalized_bucket(id: &str, bucket: &JsonValue) -> JsonValue {
    json!({
        "id": id,
        "name": bucket.get("limitName").cloned().unwrap_or(JsonValue::Null),
        "plan_type": bucket.get("planType").cloned().unwrap_or(JsonValue::Null),
        "primary": normalized_window(bucket.get("primary")),
        "secondary": normalized_window(bucket.get("secondary")),
        "reached": bucket.get("rateLimitReachedType").cloned().unwrap_or(JsonValue::Null)
    })
}

pub(super) fn normalized_window(window: Option<&JsonValue>) -> JsonValue {
    let Some(window) = window.and_then(JsonValue::as_object) else {
        return JsonValue::Null;
    };
    let mut normalized = JsonMap::new();
    normalized.insert(
        "used_percent".into(),
        window
            .get("usedPercent")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    normalized.insert(
        "duration_minutes".into(),
        window
            .get("windowDurationMins")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    normalized.insert(
        "resets_at".into(),
        window.get("resetsAt").cloned().unwrap_or(JsonValue::Null),
    );
    JsonValue::Object(normalized)
}
