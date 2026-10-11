use super::{ExampleProvider, select, usage};
use serde_json::{Value, json};

#[test]
fn banked_resets_break_headroom_ties_before_discovery_order() {
    for (first, second, expected) in [
        (json!(1), json!(3), 1),
        (json!(3), json!(1), 0),
        (json!(3), json!(3), 0),
        (json!(0), json!(u64::MAX), 1),
        (Value::Null, json!(1), 1),
        (json!(0), Value::Null, 0),
        (Value::Null, json!(0), 0),
        (Value::Null, Value::Null, 0),
        (json!(0), json!(-1), 0),
        (json!(0), json!(1.5), 0),
        (json!(0), json!("3"), 0),
    ] {
        let mut samples = [(0, first), (1, second)].map(|(index, count)| {
            let mut details = usage(20.0, 20.0);
            details["banked_resets"] = count;
            (index, details)
        });
        samples.reverse();
        let provider = ExampleProvider {
            samples: samples.to_vec(),
            wait_for_cancel: false,
        };
        let selected = select(&provider, &|| false).unwrap();
        assert_eq!(selected.report["selected_index"], expected, "{samples:?}");
        assert_eq!(selected.report["partial"], false);
        assert_eq!(
            selected.report["tie_breakers"],
            json!(["banked_resets", "discovery_order"])
        );
        for (index, sample) in samples {
            assert_eq!(
                selected.report["candidates"][index]["banked_resets"],
                json!(sample["banked_resets"].as_u64())
            );
        }
    }
}

#[test]
fn banked_resets_never_outweigh_current_headroom_or_restore_eligibility() {
    for used in [21.0, 100.0] {
        let mut reserves = usage(used, used);
        reserves["banked_resets"] = json!(u64::MAX);
        let provider = ExampleProvider {
            samples: vec![(1, reserves), (0, usage(20.0, 20.0))],
            wait_for_cancel: false,
        };
        let selected = select(&provider, &|| false).unwrap();
        assert_eq!(selected.report["selected_index"], 0);
        assert!(selected.report["candidates"][0]["banked_resets"].is_null());
        if used == 100.0 {
            assert_eq!(
                selected.report["candidates"][1]["assessment"]["reason"],
                "exhausted"
            );
        }
    }
}
