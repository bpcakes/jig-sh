//! Window roles: how usage windows are named, ordered, and projected.

use super::*;

#[test]
fn update_is_indexed_and_single_codex_window_is_weekly() {
    let mut app = app(homes());
    app.apply_update(ready_update(1));

    assert_eq!(app.completed, 1);
    assert_eq!(app.rows[1].account(), "person@example.com");
    assert_eq!(
        app.rows[1].primary_usage().unwrap().1,
        [(WindowRole::Weekly, Some(25.0))]
    );
    assert!(matches!(app.rows[0].inspection(), Inspection::Loading));
}

#[test]
fn single_codex_window_uses_its_reported_duration_role() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["primary"]["duration_minutes"] = json!(300);

    app.apply_update(update);

    assert_eq!(
        app.rows[0].primary_usage().unwrap().1,
        [(WindowRole::FiveHour, Some(25.0))]
    );
}

#[test]
fn duplicate_codex_window_durations_receive_the_same_role() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["primary"] =
        json!({ "used_percent": 10, "duration_minutes": 300 });
    update.details["rate_limits"][0]["secondary"] =
        json!({ "used_percent": 20, "duration_minutes": 300 });

    app.apply_update(update);

    assert_eq!(
        app.rows[0].primary_usage().unwrap().1,
        [
            (WindowRole::FiveHour, Some(10.0)),
            (WindowRole::FiveHour, Some(20.0))
        ]
    );
}

#[test]
fn unrecognized_codex_window_durations_remain_distinguishable() {
    let mut app = app(homes());
    let mut update = ready_update(0);
    update.details["rate_limits"][0]["primary"] =
        json!({ "used_percent": 10, "duration_minutes": 120 });
    update.details["rate_limits"][0]["secondary"] =
        json!({ "used_percent": 20, "duration_minutes": 240 });

    app.apply_update(update);

    assert_eq!(
        app.rows[0].primary_usage().unwrap().1,
        [
            (WindowRole::DurationMinutes(120), Some(10.0)),
            (WindowRole::DurationMinutes(240), Some(20.0))
        ]
    );
}

#[test]
fn unrecognized_codex_duration_remains_identifiable_in_projection() {
    const NOW: u64 = 2_000_000_000;
    let mut app = app(homes());
    app.apply_update_at(projected_update(0, 25.0, 120, 0.5, NOW), NOW);

    assert!(matches!(
        app.rows[0].projection(),
        Projection::Remaining {
            role: WindowRole::DurationMinutes(120),
            percent,
            partial: false,
        } if (percent - 50.0).abs() < PROJECTION_TOLERANCE
    ));
    assert_eq!(app.rows[0].projection().label(), "2h: ~50% left at reset");
}
