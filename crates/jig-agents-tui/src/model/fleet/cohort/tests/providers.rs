//! Provider-shaped inputs: the forecast pools whichever subscription bucket the picker names.

use super::*;

const CLAUDE: &str = "claude";
const MAX_5X: &str = "default_claude_max_5x";
const MAX_20X: &str = "default_claude_max_20x";

/// A window its provider reports as not started: no usage and no reset yet.
fn unstarted(duration_minutes: u64) -> Value {
    json!({
        "used_percent": 0.0, "duration_minutes": duration_minutes, "resets_at": null,
        "started": false
    })
}

/// A Claude account as its inspection reports it: the subscription type on the account, the
/// stated rate-limit tier (possibly null) on the subscription bucket, a model allowance beside
/// it, and no banked resets.
fn claude_account(email: &str, tier: Option<&str>, windows: Vec<Value>) -> Value {
    let mut iterator = windows.into_iter();
    json!({
        "account": { "type": "Claude", "email": email, "plan_type": "max" },
        "status": "authenticated",
        "rate_limits": [
            {
                "id": CLAUDE,
                "name": "Claude",
                "capacity_tier": tier,
                "primary": iterator.next().unwrap_or(Value::Null),
                "secondary": iterator.next().unwrap_or(Value::Null)
            },
            {
                "id": "seven_day_opus",
                "name": "Opus",
                "primary": window(99.0, WEEKLY, 302_400),
                "secondary": null
            }
        ],
        "usage_included": true,
        "inspection_error": null,
        "usage_error": null
    })
}

fn claude_row(name: &str, details: Value) -> HomeRow {
    let mut row = HomeRow::new(Home {
        path: format!("/tmp/.claude-{name}").into(),
        name: name.to_owned(),
        current: false,
    });
    row.set_inspection(Inspection::Ready(Details::from_value(
        details,
        ORIGIN,
        &[CLAUDE.to_owned()],
    )));
    row
}

#[test]
fn only_the_named_subscription_bucket_joins_the_cohort() {
    let rows = [
        claude_row(
            "a",
            claude_account(
                "a@example.com",
                Some(MAX_5X),
                vec![paced(40.0, FIVE_HOUR, 0.5), paced(20.0, WEEKLY, 0.5)],
            ),
        ),
        claude_row(
            "b",
            claude_account(
                "b@example.com",
                Some(MAX_5X),
                vec![unstarted(FIVE_HOUR), unstarted(WEEKLY)],
            ),
        ),
    ];

    let claude = forecast(&rows, CLAUDE);

    assert!(claude.coverage.is_complete(), "{:?}", claude.coverage);
    assert_eq!(
        claude.windows,
        vec![WindowRole::FiveHour, WindowRole::Weekly]
    );
    // The nearly exhausted model allowance is a separate bucket the scenario leaves out,
    // and the explanation says so.
    assert_eq!(
        claude.outcome,
        FleetOutcome::NoGap {
            burn_observed: true
        }
    );
    let details = claude.assess_at(ORIGIN).detail_lines_at(ORIGIN);
    assert!(details.iter().any(|(label, value)| label == "Quota scope"
        && value.contains("model and other allowances not modeled")));
    assert!(details.iter().all(|(label, _)| label != "Banked resets"));
    // An idle account contributes its whole allowance to the pooled meter.
    let capacity = claude.capacity.as_ref().unwrap();
    assert_eq!(capacity.usable_accounts, 2);
    assert_eq!(capacity.windows[0].gauge.used, Some(20.0));

    let codex = forecast(&rows, CODEX);
    assert_eq!(
        codex.coverage.exclusions,
        vec![(FleetExclusion::NoSubscriptionQuota, 2)]
    );
}

#[test]
fn the_stated_tier_separates_capacities_that_share_an_account_plan() {
    let account = |email: &str, tier: Option<&str>| {
        claude_account(email, tier, vec![paced(40.0, FIVE_HOUR, 0.5)])
    };

    let mixed = [
        claude_row("a", account("a@example.com", Some(MAX_5X))),
        claude_row("b", account("b@example.com", Some(MAX_20X))),
    ];
    assert_eq!(
        forecast(&mixed, CLAUDE).outcome,
        FleetOutcome::Unsupported(FleetUnsupported::MixedCapacityClasses)
    );

    let shared = [
        claude_row("a", account("a@example.com", Some(MAX_20X))),
        claude_row("b", account("b@example.com", Some(MAX_20X))),
    ];
    assert_eq!(
        forecast(&shared, CLAUDE).capacity_class.as_deref(),
        Some(MAX_20X)
    );

    // A stated but unknown tier is unknown capacity. Falling back to the `max` subscription
    // type would pool accounts whose quota sizes may differ.
    let untiered = [
        claude_row("a", account("a@example.com", None)),
        claude_row("b", account("b@example.com", None)),
    ];
    let forecast = forecast(&untiered, CLAUDE);
    assert_eq!(
        forecast.outcome,
        FleetOutcome::Unsupported(FleetUnsupported::NoEligibleAccount)
    );
    assert_eq!(
        forecast.coverage.exclusions,
        vec![(FleetExclusion::UnknownCapacityClass, 2)]
    );
}

#[test]
fn without_a_stated_tier_the_account_plan_still_decides_capacity() {
    // Codex states no tier, so its plan resolution is unchanged: the account plan first,
    // then the bucket plan when the account reports none.
    let mut details = account("a@example.com", "pro", vec![paced(20.0, WEEKLY, 0.5)]);
    details["rate_limits"][0]["plan_type"] = json!("plus");
    assert_eq!(
        forecast(&[row("a", details.clone())], CODEX)
            .capacity_class
            .as_deref(),
        Some("pro")
    );

    details["account"]["plan_type"] = json!(null);
    assert_eq!(
        forecast(&[row("a", details)], CODEX)
            .capacity_class
            .as_deref(),
        Some("plus")
    );
}

#[test]
fn an_unstarted_window_resets_one_duration_after_its_first_use() {
    // Account a serves first because its quota resets soonest. Its weekly remainder of 1%
    // runs out at 1 / (99 / 302,400) = 3,054.5 s, which blocks a until that weekly reset.
    // Account b then starts both windows. At a's five-hour pace of 50 / 3,600 per second its
    // five-hour allowance lasts 7,200 s, so the pool has a gap at 10,254.5 s. a's five-hour
    // reset at 14,400 s cannot end the gap while a's weekly window is exhausted. b's
    // five-hour window resets one duration after its first use, at 21,054.5 s.
    let rows = [
        claude_row(
            "a",
            claude_account(
                "a@example.com",
                Some(MAX_5X),
                vec![
                    window(50.0, FIVE_HOUR, 14_400),
                    window(99.0, WEEKLY, 302_400),
                ],
            ),
        ),
        claude_row(
            "b",
            claude_account(
                "b@example.com",
                Some(MAX_5X),
                vec![unstarted(FIVE_HOUR), unstarted(WEEKLY)],
            ),
        ),
    ];

    let FleetOutcome::GapRisk {
        gap_at,
        recovers_at,
        limiting,
    } = forecast(&rows, CLAUDE).outcome
    else {
        panic!("expected gap risk");
    };

    assert_near(gap_at, ORIGIN + 10_254);
    // Starting b's window at the origin would recover at 18,000 s, and never starting it
    // would wait for a's weekly reset at 302,400 s.
    assert_near(recovers_at.unwrap(), ORIGIN + 21_055);
    assert!(limiting.contains(&WindowRole::FiveHour), "{limiting:?}");
    assert!(limiting.contains(&WindowRole::Weekly), "{limiting:?}");
}

#[test]
fn a_missing_reset_is_unknown_timing_unless_the_provider_reports_the_window_not_started() {
    for primary in [
        // Zero usage alone does not show that a reset-less window has not started.
        json!({ "used_percent": 0.0, "duration_minutes": FIVE_HOUR, "resets_at": null }),
        json!({ "used_percent": 0.0, "duration_minutes": FIVE_HOUR }),
        json!({
            "used_percent": 0.0, "duration_minutes": FIVE_HOUR, "resets_at": null,
            "invalid": true
        }),
        // A window that has spent quota has started, whatever the report claims.
        json!({
            "used_percent": 5.0, "duration_minutes": FIVE_HOUR, "resets_at": null,
            "started": false
        }),
    ] {
        let rows = [claude_row(
            "a",
            claude_account("a@example.com", Some(MAX_5X), vec![primary]),
        )];

        assert_eq!(
            forecast(&rows, CLAUDE).coverage.exclusions,
            vec![(FleetExclusion::IncompleteWindowData, 1)]
        );
    }

    // Codex forwards its reset verbatim, so absent and malformed values reach the picker
    // without any marker. Both remain incomplete, as before the Claude forecast existed.
    for reset in [Value::Null, json!("invalid"), json!(12.5)] {
        let primary =
            json!({ "used_percent": 0.0, "duration_minutes": WEEKLY, "resets_at": reset });
        let rows = [row("a", account("a@example.com", "pro", vec![primary]))];

        assert_eq!(
            forecast(&rows, CODEX).coverage.exclusions,
            vec![(FleetExclusion::IncompleteWindowData, 1)]
        );
    }
}
