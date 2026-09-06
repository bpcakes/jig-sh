use jig_ui::dashboard::{StatusExhaustedAttempt, StatusLoopAttempt};
use serde_json::json;

#[test]
fn status_attempt_names_preserve_the_exact_wire_fields_and_version_omissions() {
    for exhausted in [false, true] {
        for versioned in [false, true] {
            let mut expected = json!({
                "key": "attempt-example",
                "workflow_id": "example-workflow",
                "item_key": "example-item",
                "attempts": 3,
                "max_attempts": 3,
                "last_attempt_ms": 10,
                "next_eligible_ms": 20,
                "exhausted": exhausted,
                "last_status": "failed"
            });
            if versioned {
                expected["item_version"] = json!("version-one");
                expected["observed_item_version"] = json!("version-two");
            }
            let attempt: StatusLoopAttempt = serde_json::from_value(expected.clone()).unwrap();
            let compatibility: StatusExhaustedAttempt =
                serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(serde_json::to_value(attempt).unwrap(), expected);
            assert_eq!(serde_json::to_value(compatibility).unwrap(), expected);
        }
    }
}
