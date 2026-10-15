//! The overview uses the same distinct pools and aligned budgets as the forecast.

use super::*;

#[test]
fn pooled_meters_count_distinct_capacity_and_preserve_the_pace_basis() {
    let rows = [
        row(
            "ExampleOne",
            account("a@example.com", "pro", vec![paced(50.0, WEEKLY, 0.5)]),
        ),
        row(
            "ExampleAlias",
            account("a@example.com", "pro", vec![paced(50.0, WEEKLY, 0.5)]),
        ),
        row(
            "ExampleTwo",
            account("b@example.com", "pro", vec![paced(10.0, WEEKLY, 0.5)]),
        ),
    ];
    let forecast = forecast(&rows, CODEX);
    let capacity = forecast.capacity.unwrap();
    assert_eq!(capacity.usable_accounts, 2);
    assert_eq!(capacity.windows.len(), 1);
    let gauge = capacity.windows[0].gauge;
    // Two distinct equal quotas have 50 + 90 points left, or 70% of the pool.
    assert_eq!(gauge.used, Some(30.0));
    assert_eq!(gauge.projected, Some(60.0));
    assert_eq!(
        gauge.elapsed, None,
        "staggered pools have no common reset tick"
    );
}

#[test]
fn surviving_quota_is_not_presented_as_a_usable_account() {
    let rows = [
        row(
            "ExampleOne",
            account(
                "a@example.com",
                "pro",
                vec![paced(100.0, FIVE_HOUR, 0.5), paced(10.0, WEEKLY, 0.5)],
            ),
        ),
        row(
            "ExampleTwo",
            account(
                "b@example.com",
                "pro",
                vec![paced(10.0, FIVE_HOUR, 0.5), paced(100.0, WEEKLY, 0.5)],
            ),
        ),
    ];
    let forecast = forecast(&rows, CODEX);
    assert!(matches!(forecast.outcome, FleetOutcome::BlockedNow { .. }));
    let capacity = forecast.capacity.unwrap();
    assert_eq!(capacity.usable_accounts, 0);
    assert_eq!(capacity.windows.len(), 2);
    // Each dimension has 45% quota left, but neither account can serve work.
    assert!(
        capacity
            .windows
            .iter()
            .all(|window| window.gauge.used == Some(55.0))
    );
}

#[test]
fn pooled_capacity_uses_budgets_at_the_common_origin() {
    let rows = [
        row_at(
            "ExampleOlder",
            ORIGIN - 600,
            account("a@example.com", "pro", vec![window(20.0, 60, 1_800)]),
        ),
        row(
            "ExampleCurrent",
            account("b@example.com", "pro", vec![window(0.0, 60, 1_800)]),
        ),
    ];
    let capacity = forecast(&rows, CODEX).capacity.unwrap();
    // The older sample is 1,200 seconds into its hour: 20/1,200 points/s.
    // It spends ten more points reaching the origin, making pooled usage 15%.
    assert_eq!(capacity.windows[0].gauge.used, Some(15.0));
    assert_eq!(capacity.windows[0].gauge.projected, Some(30.0));
}

#[test]
fn warmup_shows_observed_capacity_without_a_projected_ghost() {
    let rows = [row(
        "ExampleWarming",
        account("a@example.com", "pro", vec![paced(95.0, WEEKLY, 0.03)]),
    )];
    let forecast = forecast(&rows, CODEX);
    assert_eq!(forecast.outcome, FleetOutcome::Collecting);
    let capacity = forecast.capacity.unwrap();
    assert_eq!(capacity.usable_accounts, 1);
    assert_eq!(capacity.windows[0].gauge.used, Some(95.0));
    assert_eq!(capacity.windows[0].gauge.projected, None);
}
