use std::ffi::OsString;
use std::path::Path;

use super::*;
use crate::agent_provider::{Choice, HomeInspection, Metadata, PreparedLaunch};

mod claude_usage;

struct ExampleProvider {
    samples: Vec<(usize, Value)>,
    wait_for_cancel: bool,
}

#[derive(Debug, PartialEq)]
enum ExampleHome {
    Native,
    Explicit,
}

impl AgentProvider for ExampleProvider {
    type Home = ExampleHome;
    const METADATA: Metadata = Metadata {
        command: "example",
        homes_command: "example homes",
        name: "Example",
        executable: "example",
        executable_env: "JIG_EXAMPLE_BIN",
        usage: true,
        inspect_on_list: false,
        subscription_bucket: Some("example"),
    };

    fn resolve(&self, _: &Path) -> Result<Self::Home> {
        panic!("selection must retain exact provider identity")
    }
    fn discover(&self, mode: InspectionMode) -> Result<Discovery<Self::Home>> {
        assert_eq!(mode, InspectionMode::Headless);
        Ok(Discovery {
            choices: [ExampleHome::Native, ExampleHome::Explicit]
                .into_iter()
                .enumerate()
                .map(|(index, selection)| Choice {
                    selection,
                    path: "/tmp/ExampleHome".into(),
                    name: format!("example-{index}"),
                    current: index == 0,
                    details: vec![],
                })
                .collect(),
            warnings: vec![],
            inspection: Some(Box::new(Self {
                samples: self.samples.clone(),
                wait_for_cancel: self.wait_for_cancel,
            })),
        })
    }
    fn revalidate(&self, home: &Self::Home) -> Result<Self::Home> {
        Ok(match home {
            ExampleHome::Native => ExampleHome::Native,
            ExampleHome::Explicit => ExampleHome::Explicit,
        })
    }
    fn prepare(&self, _: &Self::Home, _: &[OsString]) -> Result<PreparedLaunch> {
        panic!("selection must not launch")
    }
    fn homes_report(
        &self,
        _: bool,
        _: &(dyn Fn() -> bool + Sync),
        _: &mut dyn FnMut(usize, usize),
    ) -> Result<Value> {
        panic!("selection must not reconstruct homes from report JSON")
    }
}

impl HomeInspection for ExampleProvider {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        for (index, value) in &self.samples {
            if cancelled() {
                break;
            }
            emit(*index, value.clone())?;
        }
        while self.wait_for_cancel && !cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

fn usage(short: f64, weekly: f64) -> Value {
    let now = chrono::Utc::now().timestamp();
    json!({
        "account": {"type": "example"},
        "rate_limits": [{
            "id": "example",
            "primary": {"used_percent": short, "duration_minutes": 300, "resets_at": now + 9000},
            "secondary": {"used_percent": weekly, "duration_minutes": 10080, "resets_at": now + 300000},
        }],
    })
}

#[test]
fn chooses_the_best_limiting_window_and_preserves_same_path_modes() {
    let provider = ExampleProvider {
        samples: vec![(1, usage(45.0, 40.0)), (0, usage(10.0, 90.0))],
        wait_for_cancel: false,
    };
    let selected = select(&provider, &|| false).unwrap();
    assert_eq!(selected.home, Some(ExampleHome::Explicit));
    assert_eq!(selected.report["selected_index"], 1);
    assert_eq!(
        selected.report["candidates"][1]["assessment"]["remaining_percent"],
        55.0
    );
    assert_eq!(selected.report["partial"], false);
}

#[test]
fn ties_use_discovery_order_instead_of_inspection_completion_order() {
    let provider = ExampleProvider {
        samples: vec![(1, usage(20.0, 20.0)), (0, usage(20.0, 20.0))],
        wait_for_cancel: false,
    };
    assert_eq!(
        select(&provider, &|| false).unwrap().home,
        Some(ExampleHome::Native)
    );
}

#[test]
fn unknown_exhausted_or_invalid_usage_never_becomes_free_capacity() {
    let now = chrono::Utc::now().timestamp();
    for (field, replacement, expected) in [
        ("used_percent", json!(null), Reason::InvalidUsage),
        ("used_percent", json!(-1), Reason::InvalidUsage),
        ("used_percent", json!(100), Reason::Exhausted),
        ("used_percent", json!(105), Reason::Exhausted),
        ("resets_at", json!(now), Reason::StaleUsage),
        ("resets_at", json!("invalid"), Reason::InvalidUsage),
        ("resets_at", json!(now + 700000), Reason::InvalidUsage),
        ("duration_minutes", json!(0), Reason::InvalidUsage),
    ] {
        let mut details = usage(0.0, 0.0);
        details["rate_limits"][0]["secondary"][field] = replacement;
        let assessment = assessment::assess(&details, "example", now);
        assert_eq!(assessment.reason, expected, "{details}");
        assert_eq!(
            assessment.remaining_percent,
            (expected == Reason::Exhausted).then_some(0.0)
        );
    }
}

#[test]
fn missing_reset_times_keep_known_usage_and_nullable_limiting_window() {
    let now = chrono::Utc::now().timestamp();
    for used in [0.0, 40.0, 100.0] {
        for omit in [false, true] {
            let mut details = usage(used, used);
            for key in ["primary", "secondary"] {
                let window = &mut details["rate_limits"][0][key];
                window["resets_at"] = Value::Null;
                if omit {
                    window.as_object_mut().unwrap().remove("resets_at");
                }
            }
            let result = assessment::assess(&details, "example", now);
            assert_eq!(result.remaining_percent, Some(100.0 - used));
            assert_eq!(
                result.reason,
                if used < 100.0 {
                    Reason::Eligible
                } else {
                    Reason::Exhausted
                }
            );
            assert!(
                serde_json::to_value(result).unwrap()["limiting_window"]["resets_at"].is_null()
            );
        }
    }
}

#[test]
fn model_specific_allowances_and_server_reached_state_can_block_selection() {
    let mut details = usage(1.0, 1.0);
    let mut scoped = details["rate_limits"][0].clone();
    scoped["id"] = json!("weekly_scoped_0");
    scoped["name"] = json!("Fable");
    scoped["primary"]["used_percent"] = json!(100);
    details["rate_limits"].as_array_mut().unwrap().push(scoped);
    let result = assessment::assess(&details, "example", chrono::Utc::now().timestamp());
    assert_eq!(result.reason, Reason::Exhausted);
    assert_eq!(
        result.limiting_window.unwrap().bucket_name.as_deref(),
        Some("Fable")
    );
    details["rate_limits"].as_array_mut().unwrap().pop();
    details["rate_limits"][0]["reached"] = json!("credits");
    assert_eq!(
        assessment::assess(&details, "example", chrono::Utc::now().timestamp()).reason,
        Reason::Exhausted
    );
}

#[test]
fn absent_optional_windows_are_distinct_from_unavailable_usage() {
    let now = chrono::Utc::now().timestamp();
    let mut details = usage(20.0, 20.0);
    details["rate_limits"][0]["secondary"] = Value::Null;
    assert_eq!(
        assessment::assess(&details, "example", now).remaining_percent,
        Some(80.0)
    );
    for (field, replacement, reason) in [
        ("account", Value::Null, Reason::SignedOut),
        (
            "usage_error",
            json!("unavailable"),
            Reason::UsageUnavailable,
        ),
        (
            "inspection_error",
            json!("unavailable"),
            Reason::InspectionError,
        ),
        ("rate_limits", json!([]), Reason::UsageUnavailable),
    ] {
        let mut unavailable = details.clone();
        unavailable[field] = replacement;
        assert_eq!(
            assessment::assess(&unavailable, "example", now).reason,
            reason
        );
    }
    details["rate_limits"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"unknown"}));
    assert_eq!(
        assessment::assess(&details, "example", now).reason,
        Reason::UsageUnavailable
    );
}

#[test]
fn timeout_keeps_only_completed_eligible_samples_and_reports_partial_coverage() {
    let provider = ExampleProvider {
        samples: vec![(1, usage(10.0, 20.0))],
        wait_for_cancel: true,
    };
    let selected = select_with_timeout(&provider, &|| false, Duration::from_millis(20)).unwrap();
    assert_eq!(selected.home, Some(ExampleHome::Explicit));
    assert_eq!(selected.report["timed_out"], true);
    assert_eq!(selected.report["partial"], true);
    assert_eq!(
        selected.report["candidates"][0]["assessment"]["reason"],
        "inspection_unavailable"
    );
    let selected = select_with_timeout(&provider, &|| false, Duration::ZERO).unwrap();
    assert_eq!(selected.home, None);
    assert_eq!(selected.report["reason"], "no_eligible_home");
}

#[test]
fn cancellation_and_invalid_inspection_indices_fail_instead_of_launching() {
    let provider = ExampleProvider {
        samples: vec![(20, usage(0.0, 0.0))],
        wait_for_cancel: false,
    };
    assert!(
        select(&provider, &|| true)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(
        select(&provider, &|| false)
            .unwrap_err()
            .to_string()
            .contains("unknown home index")
    );
}
