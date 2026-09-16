//! Pure fleet-forecast tests.
//!
//! These build rows directly so the engine can be exercised without the picker, the
//! terminal, or inspection transport. Analytical expectations are stated in the test so an
//! arithmetic bug is distinguishable from a deliberate allocation-policy choice.

use serde_json::{Value, json};

use super::*;
use crate::Home;

const ORIGIN: u64 = 2_000_000_000;
const FIVE_HOUR: u64 = 300;
const WEEKLY: u64 = 10_080;
const TOLERANCE: u64 = 2;

/// One reported quota window. `resets_at` is expressed as an offset after `ORIGIN` so reset
/// alignment is explicit in each test.
fn window(used_percent: f64, duration_minutes: u64, resets_in: u64) -> Value {
    json!({
        "used_percent": used_percent,
        "duration_minutes": duration_minutes,
        "resets_at": ORIGIN + resets_in
    })
}

/// A window whose elapsed fraction is chosen instead of its reset offset.
fn paced(used_percent: f64, duration_minutes: u64, elapsed_fraction: f64) -> Value {
    let duration = duration_minutes * 60;
    let remaining = (duration as f64 * (1.0 - elapsed_fraction)).round() as u64;
    window(used_percent, duration_minutes, remaining)
}

fn account(email: &str, plan: &str, windows: Vec<Value>) -> Value {
    let mut iterator = windows.into_iter();
    json!({
        "account": { "type": "chatgpt", "email": email, "plan_type": plan },
        "status": "authenticated",
        "rate_limits": [{
            "id": "codex",
            "name": null,
            "plan_type": plan,
            "primary": iterator.next().unwrap_or(Value::Null),
            "secondary": iterator.next().unwrap_or(Value::Null),
            "reached": null
        }],
        "inspection_error": null,
        "usage_error": null
    })
}

fn row_at(name: &str, observed_at: u64, details: Value) -> HomeRow {
    let mut row = HomeRow::new(Home {
        path: format!("/tmp/.codex-{name}").into(),
        name: name.to_owned(),
        current: false,
    });
    row.set_inspection(Inspection::Ready(Details::from_value(
        details,
        observed_at,
        &[CODEX_SUBSCRIPTION_BUCKET.to_owned()],
    )));
    row
}

fn row(name: &str, details: Value) -> HomeRow {
    row_at(name, ORIGIN, details)
}

fn loading_row(name: &str) -> HomeRow {
    HomeRow::new(Home {
        path: format!("/tmp/.codex-{name}").into(),
        name: name.to_owned(),
        current: false,
    })
}

fn assert_near(actual: u64, expected: u64) {
    assert!(
        actual.abs_diff(expected) <= TOLERANCE,
        "expected ~{expected}, got {actual}"
    );
}

#[test]
fn a_single_window_account_keeps_its_reported_schema() {
    let rows = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]),
    )];

    let forecast = forecast(&rows);

    assert_eq!(forecast.windows, vec![WindowRole::Weekly]);
    assert!(
        forecast
            .assess_at(ORIGIN)
            .detail_lines_at(ORIGIN)
            .contains(&("Quota windows".to_owned(), "weekly".to_owned()))
    );
}

#[test]
fn duplicate_window_durations_are_unsupported_rather_than_merged() {
    let rows = [row(
        "solo",
        account(
            "a@example.com",
            "pro",
            vec![paced(10.0, FIVE_HOUR, 0.5), paced(20.0, FIVE_HOUR, 0.4)],
        ),
    )];

    let forecast = forecast(&rows);

    assert_eq!(
        forecast.coverage.exclusions,
        vec![(FleetExclusion::DuplicateWindowDurations, 1)]
    );
    assert_eq!(
        forecast.outcome,
        FleetOutcome::Unsupported(FleetUnsupported::NoEligibleAccount)
    );
}

#[test]
fn different_window_layouts_are_not_pooled_by_displayed_role() {
    let rows = [
        row(
            "both",
            account(
                "a@example.com",
                "pro",
                vec![paced(20.0, FIVE_HOUR, 0.5), paced(20.0, WEEKLY, 0.5)],
            ),
        ),
        row(
            "weekly-only",
            account("b@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]),
        ),
    ];

    assert_eq!(
        forecast(&rows).outcome,
        FleetOutcome::Unsupported(FleetUnsupported::MixedWindowSchemas)
    );
}

#[test]
fn duplicate_identities_collapse_into_one_quota_pool() {
    let rows = [
        row_at(
            "primary",
            ORIGIN,
            account("a@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
        row_at(
            "copy",
            ORIGIN,
            account("a@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
    ];

    let forecast = forecast(&rows);

    assert_eq!(forecast.coverage.included, 1);
    assert_eq!(forecast.coverage.homes, 2);
    assert_eq!(
        forecast.coverage.exclusions,
        vec![(FleetExclusion::SharedQuotaPool, 1)]
    );
    // One pool, not two: 10% remaining at 90% per 302_400s lasts 33_600s.
    let FleetOutcome::GapRisk { gap_at, .. } = forecast.outcome else {
        panic!("expected gap risk, got {:?}", forecast.outcome);
    };
    assert_near(gap_at, ORIGIN + 33_600);
}

#[test]
fn an_unknown_account_identity_keeps_coverage_incomplete() {
    let mut details = account("a@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]);
    details["account"]["email"] = Value::Null;
    let rows = [row("solo", details)];

    assert_eq!(
        forecast(&rows).coverage.exclusions,
        vec![(FleetExclusion::UnknownIdentity, 1)]
    );
}

#[test]
fn mixed_capacity_classes_refuse_to_assume_equal_quotas() {
    let rows = [
        row(
            "pro",
            account("a@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
        row(
            "plus",
            account("b@example.com", "plus", vec![paced(10.0, WEEKLY, 0.5)]),
        ),
    ];

    let assessment = forecast(&rows).assess_at(ORIGIN);

    assert_eq!(
        assessment.outcome(),
        &FleetOutcome::Unsupported(FleetUnsupported::MixedCapacityClasses)
    );
    assert_eq!(
        assessment.summary_label_at(ORIGIN),
        "Fleet unavailable: mixed capacities need comparable quota weights"
    );
}

#[test]
fn an_unknown_capacity_class_is_excluded_instead_of_assumed_equal() {
    let mut details = account("a@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]);
    details["account"]["plan_type"] = Value::Null;
    details["rate_limits"][0]["plan_type"] = Value::Null;
    let rows = [row("solo", details)];

    assert_eq!(
        forecast(&rows).coverage.exclusions,
        vec![(FleetExclusion::UnknownCapacityClass, 1)]
    );
}

#[test]
fn unavailable_inputs_are_named_instead_of_producing_an_all_account_conclusion() {
    let signed_out = json!({
        "account": null,
        "status": "not logged in",
        "rate_limits": [],
        "inspection_error": null,
        "usage_error": null
    });
    let mut failed = account("b@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]);
    failed["inspection_error"] = json!("app-server unavailable");
    let mut unusable = account("c@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]);
    unusable["usage_error"] = json!("usage unavailable");
    let rows = [
        row(
            "healthy",
            account("a@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]),
        ),
        row("signed-out", signed_out),
        row("failed", failed),
        row("unusable", unusable),
        loading_row("loading"),
    ];

    let assessment = forecast(&rows).assess_at(ORIGIN);

    assert_eq!(assessment.coverage().included(), 1);
    assert_eq!(assessment.coverage().homes(), 5);
    assert!(!assessment.coverage().is_complete());
    assert_eq!(
        assessment.coverage().exclusion_label().as_deref(),
        Some("1 still loading, 1 inspection error, 1 usage error, 1 signed out")
    );
    assert!(
        assessment
            .summary_label_at(ORIGIN)
            .starts_with("Fleet partial 1/5: ")
    );
}

#[test]
fn inspection_that_has_not_finished_collects_instead_of_reporting_unsupported() {
    let rows = [loading_row("one"), loading_row("two")];

    let assessment = forecast(&rows).assess_at(ORIGIN);

    assert_eq!(assessment.outcome(), &FleetOutcome::Collecting);
    assert_eq!(
        assessment.summary_label_at(ORIGIN),
        "Fleet: collecting account usage"
    );
    assert_eq!(
        assessment.detail_lines_at(ORIGIN),
        vec![
            ("Forecast".to_owned(), "collecting account usage".to_owned()),
            (
                "Accounts".to_owned(),
                "0 of 2 discovered homes · 2 still loading".to_owned()
            ),
        ]
    );
}

#[test]
fn samples_with_different_observation_times_share_one_forecast_origin() {
    // The older sample is advanced by its own observed pace to the newest sample's time,
    // so its fixed usage reading is never reinterpreted as a slowing burn.
    let rows = [
        row_at(
            "older",
            ORIGIN - 600,
            account("a@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
        row_at(
            "newer",
            ORIGIN,
            account("b@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
    ];

    let forecast = forecast(&rows);

    assert_eq!(forecast.origin, ORIGIN);
    let FleetOutcome::GapRisk { gap_at, .. } = forecast.outcome else {
        panic!("expected gap risk, got {:?}", forecast.outcome);
    };
    // Pooled demand is 90/301_800 + 90/302_400 = 5.9583e-4 %/s. The older account already
    // spent 0.179% of its own quota reaching the origin, leaving a pooled 19.821% and a
    // runway of 33_266s rather than the 33_566s it would have had at its sample time.
    assert_near(gap_at, ORIGIN + 33_266);
}

#[test]
fn the_assessment_goes_stale_when_real_time_passes_the_sample_expiry() {
    let rows = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(60.0, WEEKLY, 0.5)]),
    )];
    let forecast = forecast(&rows);

    assert!(!forecast.assess_at(ORIGIN + 899).is_stale());
    let stale = forecast.assess_at(ORIGIN + 900);
    assert!(stale.is_stale());
    assert_eq!(
        stale.summary_label_at(ORIGIN + 900),
        "All accounts 1/1: fleet forecast stale · reopen to refresh"
    );
    // Staleness never rewrites the modeled timeline.
    assert_eq!(stale.outcome(), forecast.assess_at(ORIGIN).outcome());
}

#[test]
fn a_gap_is_labeled_as_scenario_risk_with_its_assumptions_visible() {
    let rows = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(60.0, WEEKLY, 0.5)]),
    )];

    let assessment = forecast(&rows).assess_at(ORIGIN);
    let summary = assessment.summary_label_at(ORIGIN);
    let details = assessment.detail_lines_at(ORIGIN);

    assert_eq!(
        summary,
        "All accounts 1/1: gap risk in ~2d; capacity returns in ~3d"
    );
    assert!(!summary.contains("unavoidable"), "{summary}");
    assert_eq!(
        details,
        vec![
            (
                "Forecast".to_owned(),
                "gap risk in ~2d; capacity returns in ~3d".to_owned()
            ),
            ("Accounts".to_owned(), "1 of 1 discovered homes".to_owned()),
            ("Quota windows".to_owned(), "weekly".to_owned()),
            ("Limiting windows".to_owned(), "weekly".to_owned()),
            (
                "Scenario".to_owned(),
                "work transferable between accounts · earliest reset used first".to_owned()
            ),
            (
                "Rate basis".to_owned(),
                "window-average pace per account window".to_owned()
            ),
            (
                "Capacity basis".to_owned(),
                "equal quotas assumed across plan pro".to_owned()
            ),
            (
                "Reset model".to_owned(),
                "periodic approximation from each reported window duration".to_owned()
            ),
            (
                "Horizon".to_owned(),
                "7d from the newest usage sample".to_owned()
            ),
        ]
    );
}

#[test]
fn a_modeled_instant_that_real_time_has_passed_reads_as_now() {
    // A 5h window pooled against a short gap and recovery, so both modeled instants fall
    // inside the sample's own 15-minute freshness window.
    let rows = [row(
        "solo",
        account("a@example.com", "pro", vec![window(99.5, FIVE_HOUR, 300)]),
    )];

    let assessment = forecast(&rows).assess_at(ORIGIN);
    assert!(!assessment.is_stale());

    assert_eq!(
        assessment.outcome_label_at(ORIGIN + 600),
        "gap risk now; capacity returns now"
    );
    assert!(
        assessment
            .outcome_label_at(ORIGIN)
            .starts_with("gap risk in ~"),
        "{}",
        assessment.outcome_label_at(ORIGIN)
    );
}

#[test]
fn a_generic_usage_bucket_is_not_treated_as_codex_subscription_quota() {
    let mut details = account("a@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]);
    details["rate_limits"][0]["id"] = json!("spark");
    let rows = [row("solo", details)];

    assert_eq!(
        forecast(&rows).coverage.exclusions,
        vec![(FleetExclusion::NoSubscriptionQuota, 1)]
    );
}

fn cohort(count: usize, used_percent: f64) -> Vec<HomeRow> {
    (0..count)
        .map(|index| {
            row(
                &format!("home-{index}"),
                account(
                    &format!("a{index}@example.com"),
                    "pro",
                    vec![
                        paced(used_percent, FIVE_HOUR, 0.5),
                        paced(used_percent, WEEKLY, 0.5),
                    ],
                ),
            )
        })
        .collect()
}

#[test]
fn a_realistic_cohort_forecasts_a_full_weekly_horizon_of_short_window_resets() {
    let rows = cohort(64, 10.0);

    let forecast = forecast(&rows);

    assert_eq!(forecast.coverage.included, 64);
    assert_eq!(
        forecast.outcome,
        FleetOutcome::NoGap {
            burn_observed: true
        }
    );
}

#[test]
fn an_oversized_cohort_states_its_budget_limit_instead_of_running_long() {
    let rows = cohort(1_024, 10.0);

    let forecast = forecast(&rows);

    assert_eq!(forecast.coverage.included, 1_024);
    assert_eq!(
        forecast.outcome,
        FleetOutcome::Unsupported(FleetUnsupported::ForecastBudget)
    );
}

mod timeline;
