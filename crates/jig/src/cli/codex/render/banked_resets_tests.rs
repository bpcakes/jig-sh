use serde_json::{Value, json};

use super::format_codex_homes_summary;

#[test]
fn usage_summary_distinguishes_zero_banked_resets_from_unknown() {
    for (count, expected) in [
        (json!(3), "3"),
        (json!(0), "0"),
        (Value::Null, "unknown"),
        (json!(-1), "unknown"),
    ] {
        let report = json!({
            "usage_included": true,
            "homes": [{
                "name": "example", "account": {"type": "chatgpt"},
                "banked_resets": count, "usage_error": "no usage windows"
            }]
        });
        let summary = format_codex_homes_summary(&report);
        assert!(
            summary.contains(&format!("Banked resets: {expected}")),
            "{summary}"
        );
        assert!(summary.contains("usage error: no usage windows"));
    }
}

#[test]
fn banked_resets_require_usage_and_an_observed_account() {
    for (usage, account, error) in [
        (false, json!({"type": "chatgpt"}), Value::Null),
        (true, Value::Null, Value::Null),
        (true, json!({"type": "chatgpt"}), json!("inspection failed")),
    ] {
        let report = json!({
            "usage_included": usage,
            "homes": [{"account": account, "banked_resets": 3, "inspection_error": error}]
        });
        assert!(!format_codex_homes_summary(&report).contains("Banked resets"));
    }
}
