use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use serde_json::{Value, json};

use super::{
    model::{App, DETAIL_SECTION_ITEM_LIMIT, Dashboard, Tab},
    render::{self, LayoutTier},
};
use crate::dashboard::{AcceptedProviderReport, scenarios};

mod local;
mod regressions;

fn fixture() -> Value {
    json!({
        "schema_version": 1,
        "observed_at_ms": 1_785_142_200_000_u64,
        "outcome": "partial",
        "future_aggregate_field": {"ignored": true},
        "repository": {
            "name": "rewrite",
            "default_branch": "main",
            "head_revision": "1234567890abcdef",
            "branch": "main",
            "detached": false,
            "dirty": true,
            "upstream": {
                "reference": "origin/main",
                "ahead": 2,
                "behind": 1,
                "state": "diverged",
                "basis": "local_tracking_ref"
            }
        },
        "work": {
            "state": {
                "current_session_id": "session_1",
                "counts": {"open_plans": 2}
            },
            "gates": [{
                "plan_id": "plan_1",
                "snapshot": {},
                "error": null
            }, {
                "plan_id": "plan_2",
                "snapshot": null,
                "error": "gate unavailable"
            }]
        },
        "loops": {
            "leases": [{}],
            "attempts": [{}, {}],
            "needs_attention": {"exhausted_attempts": [{}]}
        },
        "providers": [{
            "id": "example.rewrite",
            "status": "complete",
            "duration_ms": 4700,
            "summary": {
                "work_packages": 2,
                "work_packages_with_blockers": 1,
                "blockers": 1,
                "acceptance_checks": 3,
                "diagnostics": {
                    "total": 1,
                    "info": 0,
                    "warning": 1,
                    "error": 0
                },
                "specification": {
                    "ready": 1,
                    "blocked": 1
                },
                "implementation": {
                    "pending": 1,
                    "complete": 1
                },
                "verification": {
                    "pending": 1,
                    "complete": 1
                },
                "acceptance": {
                    "pending": 2,
                    "complete": 1
                }
            },
            "input_freshness": [{
                "name": "legacy",
                "kind": "git",
                "path": "legacy/app",
                "expected_revision": "aaaaaaaaaaaaaaaa",
                "observed_revision": "bbbbbbbbbbbbbbbb",
                "dirty": false,
                "status": "stale",
                "reason": "legacy checkout moved"
            }, {
                "name": "target",
                "kind": "git",
                "path": null,
                "expected_revision": "1234567890abcdef",
                "observed_revision": "1234567890abcdef",
                "dirty": true,
                "status": "dirty",
                "reason": null
            }],
            "report": {
                "provider": {
                    "id": "example.rewrite",
                    "display_name": "Rewrite readiness",
                    "adapter_version": "1.2.3"
                },
                "work_packages": [{
                    "id": "WP-001",
                    "title": "Blocked package",
                    "specification": {
                        "state": "needs_review",
                        "category": "blocked",
                        "summary": "Spec changed",
                        "source": {"path": "docs/packages.md", "line": 10},
                        "digest": "sha256:specification"
                    },
                    "implementation": {
                        "state": "not_started",
                        "category": "pending"
                    },
                    "verification": {
                        "state": "unverified",
                        "category": "pending"
                    },
                    "dependencies": ["WP-000"],
                    "acceptance_checks": [{
                        "ordinal": 1,
                        "id": "acceptance-one",
                        "state": "uncovered",
                        "category": "pending",
                        "target": "test/models/order_test.rb:42",
                        "source": {"path": "docs/packages.md", "line": 13}
                    }, {
                        "ordinal": 2,
                        "state": "uncovered",
                        "category": "pending"
                    }],
                    "blockers": [{
                        "code": "dependency_not_verified",
                        "message": "WP-000 must be verified first",
                        "related_work_package": "WP-000",
                        "source": {"path": "docs/packages.md", "line": 12, "column": 3}
                    }],
                    "evidence": [{
                        "kind": "legacy_test",
                        "reference": "test/models/order_test.rb",
                        "source": {"path": "legacy/app/test.rb", "line": 4},
                        "digest": "sha256:evidence"
                    }],
                    "extensions": {
                        "example.rewrite": {
                            "acceptance_check_text": [
                                "First acceptance criterion",
                                "Second acceptance criterion"
                            ],
                            "rails_route_actions": ["orders#show"]
                        }
                    }
                }, {
                    "id": "WP-002",
                    "title": "Complete package",
                    "specification": {
                        "state": "implemented",
                        "category": "complete"
                    },
                    "implementation": {
                        "state": "implemented",
                        "category": "complete"
                    },
                    "verification": {
                        "state": "verified",
                        "category": "complete"
                    },
                    "acceptance_checks": [{
                        "ordinal": 1,
                        "state": "covered",
                        "category": "complete"
                    }]
                }],
                "diagnostics": [{
                    "code": "catalog_lag",
                    "level": "warning",
                    "message": "Catalog is one revision behind",
                    "work_package": "WP-001",
                    "source": {"path": "docs/catalog.yml", "line": 2}
                }],
                "future_report_field": true
            },
            "error": null
        }, {
            "id": "example.failed",
            "status": "failed",
            "duration_ms": 30000,
            "summary": null,
            "input_freshness": [],
            "report": null,
            "error": {
                "code": "timed_out",
                "message": "Provider timed out",
                "stderr": "last diagnostic",
                "stderr_truncated": true
            }
        }],
        "errors": [{
            "scope": "work.gates.plan_2",
            "code": "work_gates_unavailable",
            "message": "Gate snapshot failed"
        }]
    })
}

#[test]
fn model_decodes_version_one_and_ignores_additive_fields() {
    let dashboard = Dashboard::from_value(fixture()).unwrap();

    assert_eq!(dashboard.repository.name, "rewrite");
    assert_eq!(dashboard.work.open_plans, 2);
    assert_eq!(dashboard.work.gate_errors, 1);
    assert_eq!(dashboard.loops.exhausted_attempts, 1);
    assert_eq!(dashboard.providers.len(), 2);
    assert_eq!(dashboard.providers[0].packages.len(), 2);
    assert_eq!(dashboard.providers[0].blockers.len(), 1);
    assert_eq!(dashboard.providers[0].packages[1].acceptance_complete, 1);
    assert_eq!(
        dashboard.providers[0].packages[0].acceptance_checks[0]
            .target
            .as_deref(),
        Some("test/models/order_test.rb:42")
    );
    assert_eq!(
        dashboard.providers[0].packages[0]
            .specification
            .digest
            .as_deref(),
        Some("sha256:specification")
    );
    assert_eq!(dashboard.providers[0].input_freshness[0].status, "stale");

    let mut unsupported = fixture();
    unsupported["schema_version"] = json!(2);
    let error = Dashboard::from_value(unsupported).unwrap_err();
    assert!(error.contains("unsupported status aggregate schema version 2"));

    let mut unsafe_text = fixture();
    unsafe_text["providers"][0]["report"]["diagnostics"][0]["message"] =
        json!("unsafe\u{1b}[31m \u{202e}diagnostic\u{2069}");
    let dashboard = Dashboard::from_value(unsafe_text).unwrap();
    assert_eq!(
        dashboard.providers[0].diagnostics[0].message,
        "unsafe\u{fffd}[31m \u{fffd}diagnostic\u{fffd}"
    );

    let mut unsafe_key = fixture();
    unsafe_key["providers"][0]["report"]["work_packages"][0]["extensions"] =
        json!({"unsafe\u{1b}[31m": {"value": true}});
    let dashboard = Dashboard::from_value(unsafe_key).unwrap();
    assert!(
        dashboard.providers[0].packages[0]
            .extensions
            .contains_key("unsafe\u{1b}[31m")
    );
}

#[test]
fn six_tabs_keep_the_contract_order_and_initial_focus() {
    assert_eq!(
        Tab::ALL.map(Tab::title),
        [
            "1 Status",
            "2 Packages",
            "3 Blockers",
            "4 Work",
            "5 Timeline",
            "6 Health",
        ]
    );
    assert_eq!(App::default().tab, Tab::Status);
    assert_eq!(App::new(Tab::Work).tab, Tab::Work);
}

#[test]
fn layout_tiers_cover_wide_standard_compact_micro_and_zero() {
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 120, 36)),
        LayoutTier::Wide
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 80, 24)),
        LayoutTier::Standard
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 60, 15)),
        LayoutTier::Compact
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 39, 11)),
        LayoutTier::Micro
    );
    assert_eq!(
        render::layout_tier(Rect::new(0, 0, 0, 0)),
        LayoutTier::Micro
    );
}

#[test]
fn extension_key_sanitization_preserves_colliding_entries_for_rendering() {
    let mut value = fixture();
    value["providers"][0]["report"]["work_packages"][0]["extensions"] = json!({
        "collision\u{1}": {"first": true},
        "collision\u{2}": {"second": true}
    });
    let mut app = App::default();
    app.accept_snapshot(value);
    app.select_tab(Tab::Packages);
    assert!(app.open_package_detail());

    let extensions = &app.detail_package().unwrap().extensions;
    assert_eq!(extensions.len(), 2);
    assert!(extensions.contains_key("collision\u{1}"));
    assert!(extensions.contains_key("collision\u{2}"));

    let detail = render_text(&app, 120, 80);
    assert!(detail.contains("first: true"));
    assert!(detail.contains("second: true"));
    assert!(!detail.contains('\u{1}'));
    assert!(!detail.contains('\u{2}'));
}

#[test]
fn extension_truncation_notice_only_appears_when_rows_are_omitted() {
    let mut exact = fixture();
    exact["providers"][0]["report"]["work_packages"][0]["extensions"] = json!({
        "provider.exact": (0..199).collect::<Vec<_>>()
    });
    let mut app = App::default();
    app.accept_snapshot(exact);
    app.select_tab(Tab::Packages);
    assert!(app.open_package_detail());
    let _ = render_text(&app, 120, 80);
    app.move_package_detail_to_edge(true);
    assert!(!render_text(&app, 120, 80).contains("details truncated"));

    let mut overflow = fixture();
    overflow["providers"][0]["report"]["work_packages"][0]["extensions"] = json!({
        "provider.overflow": (0..200).collect::<Vec<_>>()
    });
    app.accept_snapshot(overflow);
    let _ = render_text(&app, 120, 80);
    app.move_package_detail_to_edge(true);
    assert!(render_text(&app, 120, 80).contains("details truncated after 200 rows"));
}

#[test]
fn package_detail_bounds_large_sections_and_oversized_fields() {
    let mut value = fixture();
    let blockers = (0..=DETAIL_SECTION_ITEM_LIMIT)
        .map(|index| {
            json!({
                "code": format!("blocker-{index}"),
                "message": format!("{}TAIL-MARKER", "x".repeat(512)),
            })
        })
        .collect::<Vec<_>>();
    value["providers"][0]["report"]["work_packages"][0]["blockers"] = json!(blockers);

    let mut app = App::default();
    app.accept_snapshot(value);
    app.select_tab(Tab::Packages);
    assert!(app.open_package_detail());
    let _ = render_text(&app, 120, 80);
    app.move_package_detail_to_edge(true);

    let detail = render_text(&app, 120, 80);
    assert!(detail.contains("1 additional blockers omitted"));
    assert!(!detail.contains("TAIL-MARKER"));
    assert!(detail.contains('…'));
    let _ = render_text(&app, 120, 80);
    assert!(app.package_detail_scroll() <= u16::MAX as usize);
}

#[test]
fn navigation_filters_and_preserves_stable_selection_across_refresh() {
    let mut app = App::default();
    app.accept_snapshot(fixture());
    app.select_tab(Tab::Packages);
    app.move_selection(1);
    assert_eq!(app.selected_package().unwrap().id, "WP-002");

    app.toggle_blocked_only();
    assert_eq!(app.package_rows().len(), 1);
    assert_eq!(app.selected_package().unwrap().id, "WP-001");
    app.select_tab(Tab::Blockers);
    assert_eq!(
        app.selected_blocker().unwrap().blocker.code,
        "dependency_not_verified"
    );

    app.accept_snapshot(fixture());
    assert_eq!(app.current_provider().unwrap().id, "example.rewrite");
    assert_eq!(app.selected_package().unwrap().id, "WP-001");
    assert_eq!(
        app.selected_blocker().unwrap().blocker.code,
        "dependency_not_verified"
    );

    app.switch_provider(false);
    assert_eq!(app.current_provider().unwrap().id, "example.failed");
    app.switch_provider(false);
    assert_eq!(app.current_provider().unwrap().id, "example.rewrite");
}

#[test]
fn typed_snapshot_keeps_colliding_raw_identities_distinct_across_refresh() {
    let mut raw = scenarios::provider_raw_report();
    let mut first = raw["work_packages"][0].clone();
    first["id"] = json!("package\u{1}same");
    first["blockers"] = json!([{
        "code": "blocker\u{1}same",
        "message": "first blocker"
    }, {
        "code": "blocker\u{2}same",
        "message": "second blocker"
    }]);
    let mut second = first.clone();
    second["id"] = json!("package\u{2}same");
    second["title"] = json!("Second package");
    second["blockers"] = json!([]);
    raw["work_packages"] = json!([first, second]);

    let mut snapshot = scenarios::status_snapshot();
    snapshot.providers[0].id = "provider\u{1}same".to_string();
    snapshot.providers[0].report = Some(AcceptedProviderReport::from_raw(raw.clone()).unwrap());
    snapshot.providers[0].summary = None;
    let mut colliding_provider = snapshot.providers[0].clone();
    colliding_provider.id = "provider\u{2}same".to_string();
    snapshot.providers.push(colliding_provider);

    let mut app = App::default();
    app.accept_status_snapshot(snapshot.clone());
    assert_ne!(
        app.status.data.as_ref().unwrap().providers[0].id,
        app.status.data.as_ref().unwrap().providers[1].id
    );
    assert_eq!(
        app.status.data.as_ref().unwrap().providers[0].display_id,
        app.status.data.as_ref().unwrap().providers[1].display_id
    );
    app.switch_provider(false);
    assert_eq!(app.current_provider().unwrap().id, "provider\u{2}same");
    snapshot.providers.reverse();
    app.accept_status_snapshot(snapshot.clone());
    assert_eq!(app.provider_index, 0);
    assert_eq!(app.current_provider().unwrap().id, "provider\u{2}same");

    app.select_tab(Tab::Packages);
    app.move_selection(1);
    assert_eq!(app.selected_package().unwrap().id, "package\u{2}same");
    assert_eq!(
        app.package_rows()[0].display_id,
        app.package_rows()[1].display_id
    );

    raw["work_packages"].as_array_mut().unwrap().reverse();
    snapshot.providers[0].report = Some(AcceptedProviderReport::from_raw(raw.clone()).unwrap());
    app.accept_status_snapshot(snapshot.clone());
    assert_eq!(app.package_index, 0);
    assert_eq!(app.selected_package().unwrap().id, "package\u{2}same");

    app.select_tab(Tab::Blockers);
    assert_eq!(app.current_provider().unwrap().blockers.len(), 2);
    assert_ne!(
        app.current_provider().unwrap().blockers[0].blocker.code,
        app.current_provider().unwrap().blockers[1].blocker.code
    );
    assert_eq!(
        app.current_provider().unwrap().blockers[0]
            .blocker
            .display_code,
        app.current_provider().unwrap().blockers[1]
            .blocker
            .display_code
    );
    app.move_selection(1);
    assert_eq!(
        app.selected_blocker().unwrap().blocker.code,
        "blocker\u{2}same"
    );

    let mut reordered = raw;
    reordered["work_packages"][1]["blockers"]
        .as_array_mut()
        .unwrap()
        .reverse();
    snapshot.providers[0].report = Some(AcceptedProviderReport::from_raw(reordered).unwrap());
    app.accept_status_snapshot(snapshot);
    assert_eq!(
        app.selected_blocker().unwrap().blocker.code,
        "blocker\u{2}same"
    );
}

#[test]
fn typed_snapshot_sanitizes_hostile_display_text_without_mutating_identity() {
    let mut raw = scenarios::provider_raw_report();
    raw["work_packages"][0]["id"] = json!("raw\u{1b}[31m-id");
    raw["work_packages"][0]["title"] = json!("unsafe\u{1b}[31m \u{202e}title\u{2069}");
    raw["diagnostics"] = json!([{
        "code": "unsafe_code",
        "level": "warning",
        "message": "diagnostic\u{1b}[2J"
    }]);
    let mut snapshot = scenarios::status_snapshot();
    snapshot.providers[0].report = Some(AcceptedProviderReport::from_raw(raw).unwrap());
    snapshot.providers[0].summary = None;

    let mut app = App::default();
    app.accept_status_snapshot(snapshot);
    app.select_tab(Tab::Packages);
    assert_eq!(app.selected_package().unwrap().id, "raw\u{1b}[31m-id");
    assert_eq!(
        app.selected_package().unwrap().display_id,
        "raw\u{fffd}[31m-id"
    );

    let packages = render_text(&app, 120, 36);
    assert!(!packages.contains('\u{1b}'));
    assert!(!packages.contains('\u{202e}'));
    assert!(!packages.contains('\u{2069}'));
    assert!(packages.contains("raw\u{fffd}[31m-id"));
}

#[test]
fn blocker_selection_survives_insertions_duplicate_codes_and_display_changes() {
    let mut initial = fixture();
    let selected = initial["providers"][0]["report"]["work_packages"][0]["blockers"][0].clone();
    initial["providers"][0]["report"]["work_packages"][0]["blockers"] = json!([{
        "code": "dependency_not_verified",
        "message": "The first dependency is still pending",
        "related_work_package": "WP-000",
        "source": {"path": "docs/packages.md", "line": 11}
    }, selected]);

    let mut app = App::default();
    app.accept_snapshot(initial);
    app.select_tab(Tab::Blockers);
    app.move_selection(1);
    assert_eq!(
        app.selected_blocker().unwrap().blocker.message,
        "WP-000 must be verified first"
    );

    let mut refreshed = fixture();
    let mut updated_selected = selected;
    updated_selected["message"] = json!("WP-000 still needs verification");
    updated_selected["source"]["line"] = json!(42);
    refreshed["providers"][0]["report"]["work_packages"][0]["blockers"] = json!([{
        "code": "acceptance_incomplete",
        "message": "A newly discovered acceptance check is pending",
        "related_work_package": "WP-098",
        "source": {"path": "docs/packages.md", "line": 10}
    }, {
        "code": "dependency_not_verified",
        "message": "The first dependency is still pending",
        "related_work_package": "WP-000",
        "source": {"path": "docs/packages.md", "line": 11}
    }, updated_selected]);

    app.accept_snapshot(refreshed);

    assert_eq!(app.blocker_index, 2);
    assert_eq!(
        app.selected_blocker().unwrap().blocker.message,
        "WP-000 still needs verification"
    );
}

#[test]
fn package_detail_opens_scrolls_and_survives_a_stable_refresh() {
    let mut app = App::default();
    app.accept_snapshot(fixture());
    app.select_tab(Tab::Packages);

    assert!(app.open_package_detail());
    assert_eq!(app.detail_package().unwrap().id, "WP-001");
    app.move_package_detail_to_edge(true);
    assert!(app.package_detail_scroll() > 0);

    app.accept_snapshot(fixture());
    assert!(app.package_detail_is_open());
    assert_eq!(app.detail_package().unwrap().id, "WP-001");

    let mut removed = fixture();
    removed["providers"][0]["report"]["work_packages"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    app.accept_snapshot(removed);
    assert!(!app.package_detail_is_open());
}

#[test]
fn renderer_surfaces_progress_and_freshness() {
    let mut app = App::default();
    app.accept_snapshot(fixture());

    let overview = render_text(&app, 120, 36);
    assert!(overview.contains("Rewrite progress"));
    assert!(overview.contains("2 packages"));
    assert!(overview.contains("legacy [stale]"));
    assert!(overview.contains("Catalog is one"));
    assert!(overview.contains("revision behind"));
    assert!(overview.contains("no remote fetch"));
}

#[test]
fn renderer_surfaces_packages_and_full_details() {
    let mut app = App::default();
    app.accept_snapshot(fixture());

    app.select_tab(Tab::Packages);
    let packages = render_text(&app, 120, 36);
    let package_text = normalized(&packages);
    assert!(packages.contains("WP-001"));
    assert!(packages.contains("Blocked package"));
    assert!(package_text.contains("BLOCKER dependency_not_verified"));
    assert!(packages.contains("Enter for full detail"));

    assert!(app.open_package_detail());
    let detail = render_text(&app, 120, 80);
    let detail_text = normalized(&detail);
    assert!(detail.contains("Package detail"));
    assert!(detail.contains("Progress facets"));
    assert!(detail.contains("sha256:specification"));
    assert!(detail_text.contains("#1 acceptance-one"));
    assert!(detail.contains("test/models/order_test.rb:42"));
    assert!(detail.contains("sha256:evidence"));
    assert!(detail.contains("Provider-specific details"));
    assert!(detail.contains("First acceptance criterion"));
    assert!(detail.contains("Esc/Enter back"));
}

#[test]
fn renderer_surfaces_blockers() {
    let mut app = App::default();
    app.accept_snapshot(fixture());

    app.select_tab(Tab::Blockers);
    let blockers = render_text(&app, 120, 36);
    let blocker_text = normalized(&blockers);
    assert!(blockers.contains("Blocker queue (1)"));
    assert!(blocker_text.contains("WP-000 must be verified first"));
    assert!(blockers.contains("docs/packages.md:12:3"));
}

#[test]
fn renderer_surfaces_failed_providers() {
    let mut app = App::default();
    app.accept_snapshot(fixture());

    app.switch_provider(false);
    app.select_tab(Tab::Status);
    let failed = render_text(&app, 120, 36);
    assert!(failed.contains("Provider timed_out"));
    assert!(failed.contains("last diagnostic [truncated]"));
}

#[test]
fn renderer_surfaces_an_empty_provider_list() {
    let mut empty = fixture();
    empty["providers"] = json!([]);
    empty["errors"] = json!([]);
    let mut empty_app = App::default();
    empty_app.accept_snapshot(empty);
    let empty = render_text(&empty_app, 120, 36);
    assert!(empty.contains("No status providers are configured."));
}

#[test]
fn renderer_preserves_compact_content_and_micro_resize_guidance() {
    let mut app = App::default();
    app.accept_snapshot(fixture());

    let compact = render_text(&app, 60, 15);
    assert!(compact.contains("Jig [1 Status]"));
    assert!(compact.contains("2 packages, 1 blockers"));
    assert!(compact.contains("q quit"));

    let micro = render_text(&app, 39, 11);
    assert!(micro.contains("Jig Status ready - 39x11"));
    assert!(micro.contains("q quit | resize for details"));
}

#[test]
fn every_tab_renders_at_all_supported_nonzero_size_tiers() {
    let mut app = App::default();
    app.accept_status_snapshot(scenarios::status_snapshot());
    app.recorder.data = Some(scenarios::recorder_snapshot().into());

    for tab in Tab::ALL {
        app.select_tab(tab);
        for (width, height) in [(120, 36), (80, 24), (60, 15), (39, 11), (1, 1)] {
            let rendered = render_text(&app, width, height);
            assert!(
                !rendered.is_empty(),
                "{tab:?} rendered empty at {width}x{height}"
            );
        }
    }
}

#[test]
fn footer_only_advertises_bindings_valid_for_the_active_domain() {
    let mut app = App::default();
    app.accept_status_snapshot(scenarios::status_snapshot());
    app.recorder.data = Some(scenarios::recorder_snapshot().into());

    let status = render_text(&app, 120, 36);
    assert!(status.contains("[/] provider"));
    assert!(status.contains("R refresh all"));

    app.select_tab(Tab::Work);
    let work = render_text(&app, 120, 36);
    assert!(!work.contains("[/] provider"));
    assert!(!work.contains("blocked-only"));
}

#[test]
fn refresh_errors_retain_data_and_remain_domain_local() {
    let mut app = App::default();
    app.accept_status_snapshot(scenarios::status_snapshot());
    app.recorder.data = Some(scenarios::recorder_snapshot().into());

    app.accept_error(Tab::Work, "local\u{1b}[31m failed".to_string());
    assert!(app.recorder.data.is_some());
    assert_eq!(
        app.recorder.error.as_deref(),
        Some("local\u{fffd}[31m failed")
    );
    assert!(app.status.error.is_none());
    assert!(app.status.data.is_some());

    app.accept_error(Tab::Status, "provider failed".to_string());
    assert_eq!(app.status.error.as_deref(), Some("provider failed"));
    assert_eq!(
        app.recorder.error.as_deref(),
        Some("local\u{fffd}[31m failed")
    );
}

fn normalized(output: &str) -> String {
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_text(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in 0..height {
        for x in 0..width {
            output.push_str(buffer[(x, y)].symbol());
        }
        output.push('\n');
    }
    output
}
