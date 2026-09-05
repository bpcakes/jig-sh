use super::*;

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
fn status_overview_local_counts_and_errors_are_reachable() {
    let dashboard = Dashboard::from_value(fixture()).unwrap();
    assert_repository_status_fields(&dashboard);
    assert_local_status_fields(&dashboard);
    assert_status_parity_rendered();
}

#[test]
fn provider_identity_progress_and_failure_fields_are_reachable() {
    let dashboard = Dashboard::from_value(fixture()).unwrap();
    assert_provider_identity_and_progress_fields(&dashboard);
    assert_provider_observation_fields(&dashboard);
    assert_status_parity_rendered();
}

#[test]
fn provider_freshness_and_diagnostics_are_reachable() {
    let dashboard = Dashboard::from_value(fixture()).unwrap();
    assert_provider_observation_fields(&dashboard);
    assert_status_parity_rendered();
}

#[test]
fn status_repository_and_upstream_variants_are_rendered() {
    let mut app = App::default();
    app.accept_snapshot(fixture());
    let tracked = normalized(&render_text(&app, 120, 36));
    for expected in [
        "main@1234567890ab [dirty]",
        "origin/main [diverged], ahead 2, behind",
        "1 (local_tracking_ref)",
    ] {
        assert!(
            tracked.contains(expected),
            "missing {expected:?} from:\n{tracked}"
        );
    }

    let mut detached = fixture();
    detached["repository"]["head_revision"] = Value::Null;
    detached["repository"]["branch"] = Value::Null;
    detached["repository"]["detached"] = json!(true);
    detached["repository"]["dirty"] = json!(false);
    detached["repository"]["upstream"] = Value::Null;
    app.accept_snapshot(detached);
    let detached = normalized(&render_text(&app, 120, 36));
    assert!(detached.contains("detached@no HEAD [clean]"));
    assert!(detached.contains("Tracking: none (no remote fetch is performed)"));

    let mut unknown = fixture();
    unknown["repository"]["branch"] = Value::Null;
    unknown["repository"]["dirty"] = Value::Null;
    app.accept_snapshot(unknown);
    assert!(normalized(&render_text(&app, 120, 36)).contains("main@1234567890ab [unknown]"));

    for (ahead, behind, state, expected) in [
        (3, 0, "ahead", "ahead 3, behind 0"),
        (0, 4, "behind", "ahead 0, behind 4"),
    ] {
        let mut value = fixture();
        value["repository"]["upstream"]["ahead"] = json!(ahead);
        value["repository"]["upstream"]["behind"] = json!(behind);
        value["repository"]["upstream"]["state"] = json!(state);
        app.accept_snapshot(value);
        let rendered = normalized(&render_text(&app, 120, 36));
        assert!(rendered.contains(expected), "{rendered}");
        assert!(
            rendered.contains(&format!("origin/main [{state}]")),
            "{rendered}"
        );
        assert!(rendered.contains("local_tracking_ref"), "{rendered}");
    }
}

#[test]
fn provider_status_variants_keep_status_and_duration_distinct() {
    let mut app = App::default();
    for (status, duration_ms, expected) in [
        ("complete", 4_700, "complete in 4.7s"),
        ("partial", 650, "partial in 650ms"),
        ("failed", 9_200, "failed in 9.2s"),
    ] {
        let mut value = fixture();
        value["providers"][0]["status"] = json!(status);
        value["providers"][0]["duration_ms"] = json!(duration_ms);
        app.accept_snapshot(value);
        let rendered = normalized(&render_text(&app, 120, 36));
        assert!(rendered.contains(expected), "{rendered}");
    }
}

fn assert_repository_status_fields(dashboard: &Dashboard) {
    let repository = &dashboard.repository;
    assert_eq!(repository.name, "rewrite");
    assert_eq!(repository.default_branch, "main");
    assert_eq!(
        repository.head_revision.as_deref(),
        Some("1234567890abcdef")
    );
    assert_eq!(repository.branch.as_deref(), Some("main"));
    assert!(!repository.detached);
    assert_eq!(repository.dirty, Some(true));
    let upstream = repository.upstream.as_ref().unwrap();
    assert_eq!(upstream.reference, "origin/main");
    assert_eq!((upstream.ahead, upstream.behind), (2, 1));
    assert_eq!(upstream.state, "diverged");
    assert_eq!(upstream.basis, "local_tracking_ref");
}

fn assert_local_status_fields(dashboard: &Dashboard) {
    assert_eq!(dashboard.outcome, "partial");
    assert_eq!(dashboard.local_observed_at_ms, 1_785_142_200_000);
    assert_eq!(dashboard.provider_observed_at_ms, 1_785_142_200_000);
    assert_eq!(dashboard.work.open_plans, 2);
    assert_eq!(
        dashboard.work.current_session_id.as_deref(),
        Some("session_1")
    );
    assert_eq!(dashboard.work.gate_snapshots, 2);
    assert_eq!(dashboard.work.gate_errors, 1);
    assert_eq!(dashboard.loops.workflows, 3);
    assert_eq!(dashboard.loops.leases, 1);
    assert_eq!(dashboard.loops.attempts, 2);
    assert_eq!(dashboard.loops.waiting_attempts, 4);
    assert_eq!(dashboard.loops.exhausted_attempts, 1);
    assert_eq!(dashboard.errors.len(), 1);
    assert_eq!(dashboard.errors[0].scope, "work.gates.plan_2");
    assert_eq!(dashboard.errors[0].code, "work_gates_unavailable");
    assert_eq!(dashboard.errors[0].message, "Gate snapshot failed");
}

fn assert_provider_identity_and_progress_fields(dashboard: &Dashboard) {
    let provider = &dashboard.providers[0];
    assert_eq!(provider.id, "example.rewrite");
    assert_eq!(provider.display_name.as_deref(), Some("Rewrite readiness"));
    assert_eq!(provider.adapter_version.as_deref(), Some("1.2.3"));
    assert_eq!(provider.status, "complete");
    assert_eq!(provider.duration_ms, 4_700);
    assert_eq!(provider.summary.work_packages, 2);
    assert_eq!(provider.summary.work_packages_with_blockers, 1);
    assert_eq!(provider.summary.blockers, 1);
    assert_eq!(provider.summary.acceptance_checks, 3);
    assert_eq!(provider.summary.diagnostics.warning, 1);
    assert_eq!(provider.summary.specification.ready, 1);
    assert_eq!(provider.summary.specification.blocked, 1);
    assert_eq!(provider.summary.implementation.pending, 1);
    assert_eq!(provider.summary.implementation.complete, 1);
    assert_eq!(provider.summary.verification.pending, 1);
    assert_eq!(provider.summary.verification.complete, 1);
    assert_eq!(provider.summary.acceptance.pending, 2);
    assert_eq!(provider.summary.acceptance.complete, 1);
}

fn assert_provider_observation_fields(dashboard: &Dashboard) {
    let provider = &dashboard.providers[0];
    let freshness = &provider.input_freshness[0];
    assert_eq!(freshness.name, "legacy");
    assert_eq!(freshness.kind, "git");
    assert_eq!(freshness.path.as_deref(), Some("legacy/app"));
    assert_eq!(
        freshness.expected_revision.as_deref(),
        Some("aaaaaaaaaaaaaaaa")
    );
    assert_eq!(
        freshness.observed_revision.as_deref(),
        Some("bbbbbbbbbbbbbbbb")
    );
    assert_eq!(freshness.dirty, Some(false));
    assert_eq!(freshness.status, "stale");
    assert_eq!(freshness.reason.as_deref(), Some("legacy checkout moved"));

    let diagnostic = &provider.diagnostics[0];
    assert_eq!(diagnostic.code, "catalog_lag");
    assert_eq!(diagnostic.level, "warning");
    assert_eq!(diagnostic.message, "Catalog is one revision behind");
    assert_eq!(diagnostic.work_package.as_deref(), Some("WP-001"));
    assert_eq!(
        diagnostic.source.as_ref().unwrap().display(),
        "docs/catalog.yml:2"
    );

    let failed = &dashboard.providers[1];
    let error = failed.error.as_ref().unwrap();
    assert_eq!(error.code, "timed_out");
    assert_eq!(error.message, "Provider timed out");
    assert_eq!(error.stderr.as_deref(), Some("last diagnostic"));
    assert!(error.stderr_truncated);
}

fn assert_status_parity_rendered() {
    let mut app = App::default();
    app.accept_snapshot(fixture());
    let rendered = normalized(&render_text(&app, 120, 80));
    for expected in [
        "rewrite",
        "main",
        "origin/main",
        "Rewrite readiness",
        "1.2.3",
        "complete in 4.7s",
        "2 packages 1 blockers across 1 packages",
        "Specification: 1 ready 1 blocked",
        "Implementation: 1 complete 1 pending",
        "Verification: 1 complete 1 pending",
        "Acceptance: 1 complete 2 pending",
        "Checks: 3 total",
        "Work: 2 open plans, session session_1",
        "Gates: 2 snapshots, 1 collection errors",
        "Loops: 3 workflows",
        "waiting, 1 exhausted",
        "legacy [stale]",
        "Catalog is one",
        "revision behind",
        "work_gates_unavailable",
        "Observed:",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} from:\n{rendered}"
        );
    }

    app.switch_provider(false);
    let failed = normalized(&render_text(&app, 120, 80));
    for expected in [
        "Provider timed_out",
        "Provider timed out",
        "last diagnostic [truncated]",
    ] {
        assert!(
            failed.contains(expected),
            "missing {expected:?} from:\n{failed}"
        );
    }
}

#[test]
fn package_preview_facets_and_dependencies_are_reachable() {
    let dashboard = Dashboard::from_value(fixture()).unwrap();
    assert_package_facet_fields(&dashboard);
    assert_package_parity_rendered();
}

#[test]
fn package_acceptance_blockers_and_evidence_are_reachable() {
    let dashboard = Dashboard::from_value(fixture()).unwrap();
    assert_package_child_fields(&dashboard);
    assert_package_parity_rendered();
}

fn assert_package_facet_fields(dashboard: &Dashboard) {
    let package = &dashboard.providers[0].packages[0];
    assert_eq!(package.id, "WP-001");
    assert_eq!(package.title, "Blocked package");
    assert_eq!(package.specification.state, "needs_review");
    assert_eq!(package.specification.category, "blocked");
    assert_eq!(
        package.specification.summary.as_deref(),
        Some("Spec changed")
    );
    assert_eq!(
        package.specification.source.as_ref().unwrap().display(),
        "docs/packages.md:10"
    );
    assert_eq!(
        package.specification.digest.as_deref(),
        Some("sha256:specification")
    );
    assert_eq!(package.implementation.state, "not_started");
    assert_eq!(package.implementation.category, "pending");
    assert_eq!(
        package.implementation.summary.as_deref(),
        Some("Implementation has not started")
    );
    assert_eq!(
        package.implementation.source.as_ref().unwrap().display(),
        "src/order.rs:20:4"
    );
    assert_eq!(
        package.implementation.digest.as_deref(),
        Some("sha256:implementation")
    );
    assert_eq!(package.verification.state, "unverified");
    assert_eq!(package.verification.category, "pending");
    assert_eq!(
        package.verification.summary.as_deref(),
        Some("Verification is outstanding")
    );
    assert_eq!(
        package.verification.source.as_ref().unwrap().display(),
        "tests/order.rs:30:5"
    );
    assert_eq!(
        package.verification.digest.as_deref(),
        Some("sha256:verification")
    );
    assert_eq!(package.dependencies, ["WP-000"]);
    assert_eq!(
        (package.acceptance_complete, package.acceptance_total),
        (0, 2)
    );
}

fn assert_package_child_fields(dashboard: &Dashboard) {
    let package = &dashboard.providers[0].packages[0];
    let acceptance = &package.acceptance_checks[0];
    assert_eq!(acceptance.ordinal, 1);
    assert_eq!(acceptance.id.as_deref(), Some("acceptance-one"));
    assert_eq!(acceptance.state, "uncovered");
    assert_eq!(acceptance.category, "pending");
    assert_eq!(
        acceptance.target.as_deref(),
        Some("test/models/order_test.rb:42")
    );
    assert_eq!(
        acceptance.source.as_ref().unwrap().display(),
        "docs/packages.md:13"
    );

    let blocker = &package.blockers[0];
    assert_eq!(blocker.code, "dependency_not_verified");
    assert_eq!(blocker.message, "WP-000 must be verified first");
    assert_eq!(blocker.related_work_package.as_deref(), Some("WP-000"));
    assert_eq!(
        blocker.source.as_ref().unwrap().display(),
        "docs/packages.md:12:3"
    );

    let evidence = &package.evidence[0];
    assert_eq!(evidence.kind, "legacy_test");
    assert_eq!(evidence.reference, "test/models/order_test.rb");
    assert_eq!(
        evidence.source.as_ref().unwrap().display(),
        "legacy/app/test.rb:4"
    );
    assert_eq!(evidence.digest.as_deref(), Some("sha256:evidence"));
    assert_eq!(
        package.extensions["example.rewrite"]["rails_route_actions"][0],
        "orders#show"
    );
}

fn assert_package_parity_rendered() {
    let mut app = App::default();
    app.accept_snapshot(fixture());
    app.select_tab(Tab::Packages);
    let preview = normalized(&render_text(&app, 120, 36));
    for expected in ["WP-001", "Blocked package", "dependency_not_verified"] {
        assert!(
            preview.contains(expected),
            "missing {expected:?} from:\n{preview}"
        );
    }
    let compact = normalized(&render_text(&app, 60, 15));
    assert!(compact.contains("WP-001 Blocked package"));
    assert!(app.open_package_detail());
    let detail = normalized(&render_text(&app, 120, 80));
    for expected in [
        "needs_review",
        "Spec changed",
        "docs/packages.md:10",
        "sha256:specification",
        "WP-000",
        "acceptance-one",
        "test/models/order_test.rb:42",
        "docs/packages.md:12:3",
        "test/models/order_test.rb",
        "sha256:evidence",
        "orders#show",
    ] {
        assert!(
            detail.contains(expected),
            "missing {expected:?} from:\n{detail}"
        );
    }

    let mut blockers = App::default();
    blockers.accept_snapshot(fixture());
    blockers.select_tab(Tab::Blockers);
    let blocker = normalized(&render_text(&blockers, 120, 36));
    for expected in [
        "dependency_not_verified",
        "WP-000 must be verified first",
        "docs/packages.md:12:3",
    ] {
        assert!(
            blocker.contains(expected),
            "missing {expected:?} from:\n{blocker}"
        );
    }
}
