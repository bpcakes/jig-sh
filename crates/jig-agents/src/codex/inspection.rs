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
        "banked_resets": null,
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
    let banked_resets = if include_usage && !account.is_null() {
        response
            .rate_limits
            .as_ref()
            .and_then(|usage| usage["rateLimitResetCredits"]["availableCount"].as_u64())
    } else {
        None
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
        "banked_resets": banked_resets,
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
    let Some(window) = window.filter(|window| !window.is_null()) else {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inspect_resets(usage: JsonValue, include_usage: bool, logged_in: bool) -> JsonValue {
        inspected_home_json(
            AppServerAccountResponse {
                account: json!({"account": logged_in.then(|| json!({"type": "chatgpt"}))}),
                rate_limits: Some(usage),
                usage_error: None,
            },
            include_usage,
        )
    }

    #[test]
    fn banked_resets_use_available_count_not_credit_detail_length() {
        for count in [0, 7, u64::MAX] {
            let result = inspect_resets(
                json!({"rateLimitResetCredits": {
                    "availableCount": count,
                    "credits": [{"id": "example-reset-credit"}]
                }}),
                true,
                true,
            );
            assert_eq!(result["banked_resets"], count);
            // Missing windows do not discard a known count.
            assert!(result["usage_error"].is_string());
            assert!(!result.to_string().contains("example-reset-credit"));
        }
    }

    #[test]
    fn unavailable_banked_resets_remain_unknown() {
        for usage in [
            json!({}),
            json!({"rateLimitResetCredits": null}),
            json!({"rateLimitResetCredits": {}}),
            json!({"rateLimitResetCredits": {"availableCount": null}}),
            json!({"rateLimitResetCredits": {"availableCount": -1}}),
            json!({"rateLimitResetCredits": {"availableCount": 1.5}}),
            json!({"rateLimitResetCredits": {"availableCount": "3"}}),
        ] {
            assert_eq!(
                inspect_resets(usage, true, true)["banked_resets"],
                JsonValue::Null
            );
        }
        for (include_usage, logged_in) in [(false, true), (true, false)] {
            let result = inspect_resets(
                json!({"rateLimitResetCredits": {"availableCount": 3}}),
                include_usage,
                logged_in,
            );
            assert!(result["banked_resets"].is_null());
        }
    }

    #[test]
    fn malformed_present_windows_stay_unknown_instead_of_disappearing() {
        for value in [json!("invalid"), json!(7), json!({"usedPercent":"10"})] {
            let normalized = normalized_window(Some(&value));
            assert!(normalized.is_object());
            assert!(normalized["used_percent"].as_f64().is_none());
        }
        assert!(normalized_window(None).is_null());
        assert!(normalized_window(Some(&JsonValue::Null)).is_null());
    }
}
