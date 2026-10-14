use serde_json::Value;

use super::{Details, RateLimitBucket, optional_text, sanitize_value, text_at};

impl Details {
    pub(super) fn from_value(
        mut value: Value,
        observed_at: u64,
        subscription_buckets: &[String],
    ) -> Self {
        sanitize_value(&mut value);
        let account = value.get("account").filter(|account| account.is_object());
        let inferred_status = if account.is_some() {
            "authenticated"
        } else if value.get("account").is_some_and(Value::is_null) {
            "not logged in"
        } else {
            "unknown"
        };
        Self {
            account_type: text_at(account, "type"),
            email: text_at(account, "email"),
            plan: text_at(account, "plan_type"),
            status: value
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| !status.is_empty())
                .unwrap_or(inferred_status)
                .to_owned(),
            buckets: value
                .get("rate_limits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|value| RateLimitBucket::from_value(value, subscription_buckets))
                .collect(),
            inspection_error: optional_text(&value, "inspection_error"),
            usage_error: optional_text(&value, "usage_error"),
            banked_resets: value.get("banked_resets").map(|count| {
                count
                    .as_u64()
                    .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
            }),
            observed_at,
        }
    }
}
