use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};

use crate::{
    model::{App, Dashboard, Tab},
    render,
};

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
            "id": "factorish.rewrite",
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
                    "id": "factorish.rewrite",
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
                        "source": {"path": "docs/packages.md", "line": 10}
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
                        "state": "uncovered",
                        "category": "pending"
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
                        "source": {"path": "legacy/app/test.rb", "line": 4}
                    }]
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
            "id": "factorish.failed",
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
    assert_eq!(dashboard.providers[0].input_freshness[0].status, "stale");

    let mut unsupported = fixture();
    unsupported["schema_version"] = json!(2);
    let error = Dashboard::from_value(unsupported).unwrap_err();
    assert!(error.contains("unsupported status aggregate schema version 2"));

    let mut unsafe_text = fixture();
    unsafe_text["providers"][0]["report"]["diagnostics"][0]["message"] =
        json!("unsafe\u{1b}[31m diagnostic");
    let dashboard = Dashboard::from_value(unsafe_text).unwrap();
    assert_eq!(
        dashboard.providers[0].diagnostics[0].message,
        "unsafe\u{fffd}[31m diagnostic"
    );
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
    assert_eq!(app.current_provider().unwrap().id, "factorish.rewrite");
    assert_eq!(app.selected_package().unwrap().id, "WP-001");
    assert_eq!(
        app.selected_blocker().unwrap().blocker.code,
        "dependency_not_verified"
    );

    app.switch_provider(false);
    assert_eq!(app.current_provider().unwrap().id, "factorish.failed");
    app.switch_provider(false);
    assert_eq!(app.current_provider().unwrap().id, "factorish.rewrite");
}

#[test]
fn renderer_surfaces_progress_freshness_packages_blockers_and_small_terminals() {
    let mut app = App::default();
    app.accept_snapshot(fixture());

    let overview = render_text(&app, 120, 36);
    assert!(overview.contains("Rewrite progress"));
    assert!(overview.contains("2 packages"));
    assert!(overview.contains("legacy [stale]"));
    assert!(overview.contains("Catalog is one"));
    assert!(overview.contains("revision behind"));
    assert!(overview.contains("no remote fetch"));

    app.select_tab(Tab::Packages);
    let packages = render_text(&app, 120, 36);
    let package_text = normalized(&packages);
    assert!(packages.contains("WP-001"));
    assert!(packages.contains("Blocked package"));
    assert!(package_text.contains("BLOCKER dependency_not_verified"));

    app.select_tab(Tab::Blockers);
    let blockers = render_text(&app, 120, 36);
    let blocker_text = normalized(&blockers);
    assert!(blockers.contains("Blocker queue (1)"));
    assert!(blocker_text.contains("WP-000 must be verified first"));
    assert!(blockers.contains("docs/packages.md:12:3"));

    app.switch_provider(false);
    app.select_tab(Tab::Overview);
    let failed = render_text(&app, 120, 36);
    assert!(failed.contains("Provider timed_out"));
    assert!(failed.contains("last diagnostic [truncated]"));

    let mut empty = fixture();
    empty["providers"] = json!([]);
    empty["errors"] = json!([]);
    let mut empty_app = App::default();
    empty_app.accept_snapshot(empty);
    let empty = render_text(&empty_app, 120, 36);
    assert!(empty.contains("No status providers are configured."));

    let small = render_text(&app, 60, 15);
    assert!(small.contains("Terminal too small: 60x15"));
    assert!(small.contains("at least 72x20"));
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
