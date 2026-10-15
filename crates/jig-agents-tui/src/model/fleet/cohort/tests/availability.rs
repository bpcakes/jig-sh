//! Availability is an observed fact even at a window's exact start.

use super::*;

#[test]
fn exhausted_quota_at_zero_elapsed_time_reports_blocking_and_recovery() {
    for durations in [vec![FIVE_HOUR], vec![FIVE_HOUR, WEEKLY]] {
        let rows = [row(
            "ExampleAccount",
            account(
                "account@example.com",
                "pro",
                durations
                    .iter()
                    .map(|duration| paced(100.0, *duration, 0.0))
                    .collect(),
            ),
        )];
        let forecast = forecast(&rows, CODEX);
        let FleetOutcome::BlockedNow {
            recovers_at,
            limiting,
        } = &forecast.outcome
        else {
            panic!(
                "observed exhaustion must be blocked, got {:?}",
                forecast.outcome
            );
        };
        // An account needs every exhausted window restored. The short reset alone is not
        // recovery for the two-window case, even though neither pace can be measured.
        assert_eq!(
            *recovers_at,
            Some(ORIGIN + durations.iter().max().unwrap() * 60)
        );
        assert_eq!(limiting.len(), durations.len());
        assert_eq!(forecast.capacity.as_ref().unwrap().usable_accounts, 0);
        assert!(forecast.pace_collecting);
    }
}

#[test]
fn quota_remaining_at_zero_elapsed_time_still_collects_pace() {
    let rows = [row(
        "ExampleAccount",
        account(
            "account@example.com",
            "pro",
            vec![paced(50.0, FIVE_HOUR, 0.0), paced(0.0, WEEKLY, 0.0)],
        ),
    )];
    let forecast = forecast(&rows, CODEX);
    assert_eq!(forecast.outcome, FleetOutcome::Collecting);
    let capacity = forecast.capacity.unwrap();
    assert_eq!(capacity.usable_accounts, 1);
    assert!(
        capacity
            .windows
            .iter()
            .all(|window| window.gauge.projected.is_none())
    );
}

#[test]
fn unused_quota_at_zero_elapsed_time_reports_no_observed_burn() {
    let rows = [row(
        "ExampleAccount",
        account(
            "account@example.com",
            "pro",
            vec![paced(0.0, FIVE_HOUR, 0.0), paced(0.0, WEEKLY, 0.0)],
        ),
    )];
    let forecast = forecast(&rows, CODEX);
    assert_eq!(
        forecast.outcome,
        FleetOutcome::NoGap {
            burn_observed: false
        }
    );
    assert_eq!(forecast.capacity.unwrap().usable_accounts, 1);
    assert!(!forecast.pace_collecting);
}
