//! Modeled timeline and quota arithmetic: pooled demand, resets, and depletion.

use super::*;

#[test]
fn single_account_gap_matches_the_existing_linear_exhaustion_calculation() {
    // 60% used halfway through a weekly window burns the remaining 40% in
    // 302_400s * 40/60 = 201_600s, which is where the per-account projection also places
    // its "runs out 100_800s early" outcome.
    let rows = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(60.0, WEEKLY, 0.5)]),
    )];

    let forecast = forecast(&rows);

    let FleetOutcome::GapRisk {
        gap_at,
        recovers_at,
        limiting,
    } = forecast.outcome
    else {
        panic!("expected gap risk, got {:?}", forecast.outcome);
    };
    assert_near(gap_at, ORIGIN + 201_600);
    assert_near(
        recovers_at.expect("reset restores capacity"),
        ORIGIN + 302_400,
    );
    assert_eq!(limiting, vec![WindowRole::Weekly]);
    assert_eq!(forecast.horizon_at, ORIGIN + WEEKLY * 60);
}

#[test]
fn pooled_budget_and_demand_match_the_analytical_single_window_result() {
    // Two accounts, 10% remaining each, both burning 90% per 302_400s. Pooled budget 20%
    // against pooled demand 180%/302_400s exhausts after 20 * 302_400 / 180 = 33_600s.
    let rows = [
        row(
            "one",
            account("a@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
        row(
            "two",
            account("b@example.com", "pro", vec![paced(90.0, WEEKLY, 0.5)]),
        ),
    ];

    let forecast = forecast(&rows);

    let FleetOutcome::GapRisk { gap_at, .. } = forecast.outcome else {
        panic!("expected gap risk, got {:?}", forecast.outcome);
    };
    assert_near(gap_at, ORIGIN + 33_600);
}

#[test]
fn idle_spare_absorbs_demand_so_one_exhausted_row_is_not_a_fleet_gap() {
    let rows = [
        row(
            "busy",
            account("a@example.com", "pro", vec![paced(60.0, WEEKLY, 0.5)]),
        ),
        row(
            "spare",
            account("b@example.com", "pro", vec![paced(0.0, WEEKLY, 0.5)]),
        ),
    ];

    assert_eq!(
        forecast(&rows).outcome,
        FleetOutcome::NoGap {
            burn_observed: true
        }
    );
}

#[test]
fn an_early_reset_extends_runway_past_the_no_reset_estimate() {
    // Pooled remaining is 55% against 3.234e-4 %/s, so a model without resets would place
    // exhaustion around 170_000s. The account that resets after one hour replaces its
    // allowance first, which carries the pool through the whole weekly horizon.
    let rows = [
        row(
            "soon",
            account("a@example.com", "pro", vec![window(95.0, WEEKLY, 3_600)]),
        ),
        row(
            "later",
            account("b@example.com", "pro", vec![paced(50.0, WEEKLY, 0.5)]),
        ),
    ];

    assert_eq!(
        forecast(&rows).outcome,
        FleetOutcome::NoGap {
            burn_observed: true
        }
    );
}

#[test]
fn a_gap_before_a_later_refill_is_reported_even_though_capacity_returns() {
    let rows = [row(
        "solo",
        account(
            "a@example.com",
            "pro",
            vec![paced(60.0, FIVE_HOUR, 0.5), paced(1.0, WEEKLY, 0.5)],
        ),
    )];

    let forecast = forecast(&rows);

    let FleetOutcome::GapRisk {
        gap_at,
        recovers_at,
        limiting,
    } = forecast.outcome
    else {
        panic!("expected gap risk, got {:?}", forecast.outcome);
    };
    // 40% remaining at 120% per 5h window exhausts after 6_000s; the 5h reset at 9_000s
    // restores it while the weekly window still has headroom.
    assert_near(gap_at, ORIGIN + 6_000);
    assert_near(
        recovers_at.expect("the 5h reset restores capacity"),
        ORIGIN + 9_000,
    );
    assert_eq!(limiting, vec![WindowRole::FiveHour]);
}

#[test]
fn an_unused_allowance_is_replaced_at_reset_rather_than_accumulated() {
    // Exactly one full window per reset period: 50% used in half a window means the
    // replaced allowance is consumed precisely as each reset arrives, forever. Adding a
    // full allowance on top of unused quota would instead leave a growing surplus, and
    // spending more than one allowance per period would produce a gap.
    let rows = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(50.0, FIVE_HOUR, 0.5)]),
    )];

    assert_eq!(
        forecast(&rows).outcome,
        FleetOutcome::NoGap {
            burn_observed: true
        }
    );
}

#[test]
fn exhaustion_exactly_at_a_restoring_reset_is_not_an_outage() {
    let exact = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(50.0, FIVE_HOUR, 0.5)]),
    )];
    let over = [row(
        "solo",
        account("a@example.com", "pro", vec![paced(50.5, FIVE_HOUR, 0.5)]),
    )];

    assert_eq!(
        forecast(&exact).outcome,
        FleetOutcome::NoGap {
            burn_observed: true
        }
    );
    let FleetOutcome::GapRisk { gap_at, .. } = forecast(&over).outcome else {
        panic!("a slightly faster pace should run out before the reset");
    };
    // 49.5% remaining at 50.5% per 9_000s runs out 179s before the 9_000s reset.
    assert_near(gap_at, ORIGIN + 8_820);
}

#[test]
fn positive_per_dimension_sums_do_not_make_a_complementary_pair_usable() {
    // One account has no 5h quota, the other has no weekly quota. Every dimension has a
    // positive fleet-wide sum, yet no single account satisfies all of its own constraints.
    let rows = [
        row(
            "short",
            account(
                "a@example.com",
                "pro",
                vec![paced(100.0, FIVE_HOUR, 0.5), paced(10.0, WEEKLY, 0.5)],
            ),
        ),
        row(
            "long",
            account(
                "b@example.com",
                "pro",
                vec![paced(10.0, FIVE_HOUR, 0.5), paced(100.0, WEEKLY, 0.5)],
            ),
        ),
    ];

    let forecast = forecast(&rows);

    let FleetOutcome::BlockedNow { limiting, .. } = &forecast.outcome else {
        panic!("expected a blocked fleet, got {:?}", forecast.outcome);
    };
    assert_eq!(limiting, &vec![WindowRole::FiveHour, WindowRole::Weekly]);
}

#[test]
fn a_sibling_window_reset_does_not_declare_an_account_recovered() {
    let rows = [row(
        "solo",
        account(
            "a@example.com",
            "pro",
            vec![paced(100.0, FIVE_HOUR, 0.5), paced(100.0, WEEKLY, 0.5)],
        ),
    )];

    let forecast = forecast(&rows);

    let FleetOutcome::BlockedNow { recovers_at, .. } = forecast.outcome else {
        panic!("expected a blocked fleet, got {:?}", forecast.outcome);
    };
    // The 5h window returns after 9_000s but the exhausted weekly window still blocks the
    // account until its own reset at 302_400s.
    assert_near(
        recovers_at.expect("the weekly reset restores the account"),
        ORIGIN + 302_400,
    );
}

#[test]
fn simultaneous_resets_produce_the_same_result_in_either_row_order() {
    let first = row(
        "one",
        account("a@example.com", "pro", vec![paced(95.0, WEEKLY, 0.5)]),
    );
    let second = row(
        "two",
        account("b@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]),
    );

    assert_eq!(
        forecast(&[first.clone(), second.clone()]).outcome,
        forecast(&[second, first]).outcome
    );
}

#[test]
fn an_exhausted_account_keeps_contributing_its_demand_estimate() {
    // The exhausted account cannot serve work, but dropping its pace would leave the idle
    // spare with zero demand and an indefinite runway. Its estimated burn must still be
    // charged to whichever account serves the workload.
    let exhausted = row(
        "burned",
        account("a@example.com", "pro", vec![paced(100.0, FIVE_HOUR, 0.25)]),
    );
    let spare = row(
        "spare",
        account("b@example.com", "pro", vec![paced(0.0, FIVE_HOUR, 0.0)]),
    );

    assert_eq!(
        forecast(std::slice::from_ref(&spare)).outcome,
        FleetOutcome::NoGap {
            burn_observed: false
        }
    );
    let FleetOutcome::GapRisk { gap_at, .. } = forecast(&[exhausted, spare]).outcome else {
        panic!("retained demand should exhaust the spare account");
    };
    // 100% spare capacity at 400% per 5h window lasts 4_500s.
    assert_near(gap_at, ORIGIN + 4_500);
}

#[test]
fn all_zero_usage_reports_no_observed_burn_instead_of_sustainability() {
    let rows = [
        row(
            "one",
            account("a@example.com", "pro", vec![paced(0.0, WEEKLY, 0.5)]),
        ),
        row(
            "two",
            account("b@example.com", "pro", vec![paced(0.0, WEEKLY, 0.5)]),
        ),
    ];

    let assessment = forecast(&rows).assess_at(ORIGIN);

    assert_eq!(
        assessment.outcome(),
        &FleetOutcome::NoGap {
            burn_observed: false
        }
    );
    assert_eq!(
        assessment.summary_label_at(ORIGIN),
        "All accounts 2/2: no burn observed through 7d"
    );
}

#[test]
fn nonzero_usage_inside_the_warmup_window_collects_instead_of_reporting_zero_burn() {
    let rows = [
        row(
            "one",
            account("a@example.com", "pro", vec![paced(2.0, WEEKLY, 0.05)]),
        ),
        row(
            "two",
            account("b@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]),
        ),
    ];

    let assessment = forecast(&rows).assess_at(ORIGIN);

    assert_eq!(assessment.outcome(), &FleetOutcome::Collecting);
    assert_eq!(
        assessment.summary_label_at(ORIGIN),
        "All accounts 2/2: collecting pace evidence"
    );
}

#[test]
fn unusable_window_metadata_excludes_an_account_without_inventing_quota() {
    for primary in [
        json!({ "used_percent": null, "duration_minutes": WEEKLY, "resets_at": ORIGIN + 1 }),
        json!({ "used_percent": -1.0, "duration_minutes": WEEKLY, "resets_at": ORIGIN + 1 }),
        json!({ "used_percent": 10.0, "duration_minutes": null, "resets_at": ORIGIN + 1 }),
        json!({ "used_percent": 10.0, "duration_minutes": 0, "resets_at": ORIGIN + 1 }),
        json!({ "used_percent": 10.0, "duration_minutes": WEEKLY, "resets_at": null }),
        json!({ "used_percent": 10.0, "duration_minutes": WEEKLY, "resets_at": -1 }),
        json!({ "used_percent": 10.0, "duration_minutes": u64::MAX, "resets_at": ORIGIN + 1 }),
        // A reset already in the past cannot describe the sample's own window.
        json!({ "used_percent": 10.0, "duration_minutes": WEEKLY, "resets_at": ORIGIN - 1 }),
    ] {
        let rows = [row("solo", account("a@example.com", "pro", vec![primary]))];

        let forecast = forecast(&rows);

        assert_eq!(
            forecast.outcome,
            FleetOutcome::Unsupported(FleetUnsupported::NoEligibleAccount)
        );
        assert_eq!(
            forecast.coverage.exclusions,
            vec![(FleetExclusion::IncompleteWindowData, 1)]
        );
    }
}

#[test]
fn nonfinite_usage_never_becomes_a_quota_or_a_pace() {
    for used_percent in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let window = RateLimitWindow {
            used_percent: Some(used_percent),
            duration_minutes: Some(WEEKLY),
            resets_at: Some(i64::try_from(ORIGIN + 60).unwrap()),
        };

        assert!(fleet_window(&window, ORIGIN).is_none());
    }
}
