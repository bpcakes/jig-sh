use serde_json::json;

use super::{fixture, normalized, render_text};
use crate::{
    dashboard::{
        AcceptedProviderReport, RecorderRefresh, StatusLocalSnapshot, StatusRefresh, scenarios,
    },
    terminal::model::{App, Tab},
};

#[test]
fn disappearing_provider_resets_provider_scoped_child_state() {
    let mut value = fixture();
    value["providers"][0]["report"]["work_packages"][0]["blockers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "code": "second_blocker",
            "message": "Second blocker"
        }));
    value["providers"][1]["report"] = value["providers"][0]["report"].clone();

    let mut app = App::default();
    app.accept_snapshot(value.clone());
    app.select_tab(Tab::Blockers);
    app.move_selection(1);
    assert_eq!(app.blocker_index, 1);
    app.select_tab(Tab::Packages);
    app.move_selection(1);
    assert_eq!(app.selected_package().unwrap().id, "WP-002");
    assert!(app.open_package_detail());

    value["providers"].as_array_mut().unwrap().remove(0);
    app.accept_snapshot(value);

    assert_eq!(app.current_provider().unwrap().id, "example.failed");
    assert_eq!(app.package_index, 0);
    assert_eq!(app.blocker_index, 0);
    assert!(!app.package_detail_is_open());
}

#[test]
fn compact_lists_follow_selection_and_package_detail_remains_reachable() {
    let mut value = fixture();
    let template = value["providers"][0]["report"]["work_packages"][0].clone();
    value["providers"][0]["report"]["work_packages"] = json!(
        (0..20)
            .map(|index| {
                let mut package = template.clone();
                package["id"] = json!(format!("WP-{index:03}"));
                package["title"] = json!(format!("Package {index:03}"));
                package
            })
            .collect::<Vec<_>>()
    );
    let mut app = App::default();
    app.accept_snapshot(value);
    app.select_tab(Tab::Packages);
    app.move_to_edge(true);

    let packages = normalized(&render_text(&app, 60, 15));
    assert!(packages.contains("▶ WP-019 Package 019"));
    assert!(packages.contains("q quit | Tab | j/k | Enter | b | [/]"));

    assert!(app.open_package_detail());
    let detail = render_text(&app, 60, 15);
    assert!(detail.contains("Package detail"));
    assert!(detail.contains("Esc/Enter back"));
    let micro = render_text(&app, 39, 11);
    assert!(micro.contains("Package detail"));
    assert!(micro.contains("Esc back"));
    assert!(micro.contains("q quit"));

    app.move_package_detail_to_edge(true);
    let end = render_text(&app, 60, 15);
    assert!(end.contains("Provider-specific details"));
}

#[test]
fn compact_blockers_show_the_selected_row() {
    let mut value = fixture();
    let blockers = &mut value["providers"][0]["report"]["work_packages"][0]["blockers"];
    blockers.as_array_mut().unwrap().push(json!({
        "code": "second_blocker",
        "message": "Second blocker"
    }));
    let mut app = App::default();
    app.accept_snapshot(value);
    app.select_tab(Tab::Blockers);
    app.move_selection(1);

    assert!(normalized(&render_text(&app, 60, 15)).contains("▶ WP-001 [second_blocker]"));
}

#[test]
fn micro_package_navigation_and_errors_are_visible() {
    let mut app = App::default();
    app.accept_snapshot(fixture());
    app.select_tab(Tab::Packages);
    assert!(render_text(&app, 39, 11).contains("Selected: WP-001"));
    app.move_selection(1);
    assert!(render_text(&app, 39, 11).contains("Selected: WP-002"));
    app.accept_error(Tab::Packages, "provider refresh failed".to_string());
    let failed = render_text(&app, 39, 11);
    assert!(failed.contains("Error: provider refresh failed"));
}

#[test]
fn every_layout_reports_stale_active_domain_and_uses_its_repository() {
    let mut app = App::default();
    app.accept_status_snapshot(scenarios::status_snapshot());
    let mut recorder = scenarios::recorder_snapshot();
    recorder.repo.name = "local-repository".to_string();
    app.recorder.data = Some(recorder.into());
    app.accept_error(Tab::Work, "local refresh failed".to_string());
    app.select_tab(Tab::Work);

    let standard = render_text(&app, 80, 24);
    assert!(standard.contains("local-repository"));
    assert!(standard.contains("[stale]"));
    assert!(standard.contains("Last refresh error: local refresh failed"));
    assert!(render_text(&app, 60, 15).contains("stale - local refresh failed"));
    let micro = render_text(&app, 39, 11);
    assert!(micro.contains("Jig Work stale"));
    assert!(micro.contains("Error: local refresh failed"));
}

#[test]
fn schema_failures_are_domain_local_and_retain_last_good_recorder() {
    let mut app = App::new(Tab::Work);
    let mut invalid_status = scenarios::status_snapshot();
    invalid_status.schema_version += 1;
    app.accept_status_snapshot(invalid_status);
    assert!(
        app.status
            .error
            .as_deref()
            .unwrap()
            .contains("unsupported status")
    );
    assert!(app.recorder.error.is_none());

    let previous = scenarios::recorder_snapshot();
    app.recorder.data = Some(previous.clone().into());
    app.recorder.refresh_queued = true;
    let mut invalid_recorder = previous;
    invalid_recorder.schema_version += 1;
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder: invalid_recorder,
    });
    assert_eq!(
        app.recorder.data.as_ref().unwrap().schema_version,
        crate::dashboard::RECORDER_SCHEMA_VERSION
    );
    assert!(
        app.recorder
            .error
            .as_deref()
            .unwrap()
            .contains("unsupported recorder")
    );
    assert!(app.recorder.refresh_queued);
}

#[test]
fn successful_status_refresh_satisfies_a_queued_local_projection() {
    let mut app = App::default();
    app.recorder.refresh_queued = true;
    app.accept_status_refresh(StatusRefresh {
        status: scenarios::status_snapshot(),
        recorder: scenarios::recorder_snapshot(),
    });

    assert!(!app.recorder.refresh_queued);
    assert!(app.recorder.data.is_some());
}

#[test]
fn recorder_refresh_reprojects_local_status_without_replacing_providers() {
    let mut app = App::default();
    app.accept_status_snapshot(scenarios::status_snapshot());
    let provider_id = app.current_provider().unwrap().id.clone();
    let mut local = scenarios::status_snapshot();
    local.repository.name = "new-local-repository".to_string();
    local.observed_at_ms += 10;
    local.work.state.as_mut().unwrap().counts.open_plans = 42;
    let recorder = scenarios::recorder_snapshot();
    let status_local = StatusLocalSnapshot {
        epoch_id: recorder.epoch_id,
        observed_at_ms: local.observed_at_ms,
        repository: local.repository,
        work: local.work,
        loops: local.loops,
        errors: local.errors,
    };

    app.accept_recorder_refresh(RecorderRefresh {
        recorder,
        status_local,
    });

    let status = app.status.data.as_ref().unwrap();
    assert_eq!(status.repository.name, "new-local-repository");
    assert_eq!(status.work.open_plans, 42);
    assert_eq!(app.current_provider().unwrap().id, provider_id);
}

#[test]
fn typed_report_fallback_summary_matches_all_categories_and_diagnostics() {
    let mut raw = scenarios::provider_raw_report();
    raw["work_packages"][0]["acceptance_checks"] = json!([{
        "ordinal": 1,
        "state": "blocked",
        "category": "blocked"
    }, {
        "ordinal": 2,
        "state": "failed",
        "category": "failed"
    }]);
    raw["diagnostics"] = json!([{
        "code": "info",
        "level": "info",
        "message": "Informational"
    }, {
        "code": "warning",
        "level": "warning",
        "message": "Warning"
    }, {
        "code": "error",
        "level": "error",
        "message": "Error"
    }]);
    let mut snapshot = scenarios::status_snapshot();
    snapshot.providers[0].summary = None;
    snapshot.providers[0].report = Some(AcceptedProviderReport::from_raw(raw).unwrap());
    let mut app = App::default();
    app.accept_status_snapshot(snapshot);

    let summary = &app.current_provider().unwrap().summary;
    assert_eq!(summary.acceptance.blocked, 1);
    assert_eq!(summary.acceptance.failed, 1);
    assert_eq!(summary.acceptance.pending, 0);
    assert_eq!(summary.diagnostics.total, 3);
    assert_eq!(summary.diagnostics.info, 1);
    assert_eq!(summary.diagnostics.warning, 1);
    assert_eq!(summary.diagnostics.error, 1);
}

#[test]
fn typed_snapshot_drives_status_packages_blockers_and_detail_renderers() {
    let mut raw = scenarios::provider_raw_report();
    raw["work_packages"][0]["blockers"] = json!([{
        "code": "dependency_pending",
        "message": "Example dependency is pending"
    }]);
    let mut snapshot = scenarios::status_snapshot();
    snapshot.providers[0].summary = None;
    snapshot.providers[0].report = Some(AcceptedProviderReport::from_raw(raw).unwrap());
    let mut app = App::default();
    app.accept_status_snapshot(snapshot);

    assert!(render_text(&app, 120, 36).contains("Rewrite progress"));
    app.select_tab(Tab::Packages);
    assert!(render_text(&app, 120, 36).contains("package-example"));
    assert!(app.open_package_detail());
    assert!(render_text(&app, 120, 36).contains("Progress facets"));
    app.close_package_detail();
    app.select_tab(Tab::Blockers);
    assert!(render_text(&app, 120, 36).contains("Example dependency is pending"));
}

#[test]
fn typed_detail_is_bound_to_provider_and_package_identity() {
    let mut first_report = scenarios::provider_raw_report();
    first_report["work_packages"][0]["id"] = json!("shared-package");
    let mut snapshot = scenarios::status_snapshot();
    snapshot.providers[0].id = "provider-one".to_string();
    snapshot.providers[0].report =
        Some(AcceptedProviderReport::from_raw(first_report.clone()).unwrap());
    let mut second = snapshot.providers[0].clone();
    second.id = "provider-two".to_string();
    second.report = Some(AcceptedProviderReport::from_raw(first_report).unwrap());
    snapshot.providers.push(second);

    let mut app = App::default();
    app.accept_status_snapshot(snapshot.clone());
    app.select_tab(Tab::Packages);
    assert!(app.open_package_detail());
    snapshot.providers.remove(0);
    app.accept_status_snapshot(snapshot);

    assert_eq!(app.current_provider().unwrap().id, "provider-two");
    assert!(!app.package_detail_is_open());
}
