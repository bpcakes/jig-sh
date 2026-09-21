use super::*;
use crate::repository::freshness::observations as metrics;
use crate::surface::ResponseSurface;
use crate::tool_defs::tool;
use std::cell::Cell;

fn add_plans(ctx: &RepoContext, count: usize) {
    for _ in 0..count {
        crate::state::plans_open(
            ctx,
            crate::state::PlanOpenRequest {
                title: "Example observation sharing".into(),
                body: None,
                body_file: None,
                base: Some("HEAD".into()),
            },
        )
        .unwrap();
    }
}

fn checked_fixture(root: &Path) -> RepoContext {
    let ctx = fixture(root, true, true);
    assert_eq!(
        crate::runtime::call_tool(&ctx, tool::WORK_CHECK, json!({"plan_id":"plan_1"})).unwrap()["ok"],
        true
    );
    ctx
}

fn status(ctx: &RepoContext, cancelled: &dyn Fn() -> bool) -> anyhow::Result<Value> {
    crate::status::snapshot_with_freshness_timeout(ctx, cancelled, Some(30_000))
}

#[test]
fn inspection_shares_one_eligible_observation_per_request_not_across_requests() {
    let temp = tempdir().unwrap();
    let ctx = checked_fixture(temp.path());
    add_plans(&ctx, 19);
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    metrics::reset();
    for expected_scans in [1, 2] {
        let report = status(&ctx, &|| false).unwrap();
        let plans = report["work"]["gates"].as_array().unwrap();
        assert_eq!(plans.len(), 20);
        assert!(
            plans
                .iter()
                .all(|plan| plan["snapshot"]["gates_ok"] == true),
            "{report:#}"
        );
        assert_eq!(metrics::snapshot().original_index_scans, expected_scans);
        assert_eq!(metrics::snapshot().identity_collections, expected_scans);
        assert_eq!(metrics::snapshot().source_collections, expected_scans);
    }
    assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).unwrap());
    fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap();
    assert!(
        crate::runtime::call_tool(&ctx, tool::WORK_FINISH, json!({"plan_id":"plan_1"})).is_err()
    );
    crate::state::ensure_plan_is_open(&ctx, "plan_1").unwrap();
}

#[test]
fn inspection_retained_observation_rejects_source_journal_and_configuration_changes() {
    for changed in ["source", "journal", "configuration"] {
        let temp = tempdir().unwrap();
        let ctx = checked_fixture(temp.path());
        add_plans(&ctx, 1);
        let changed_once = Cell::new(false);
        metrics::reset();
        let mutate = || {
            // Identity collection has completed, so a subsequent plan can
            // only reuse it by revalidating the retained live observations.
            if metrics::snapshot().identity_collections == 1 && !changed_once.replace(true) {
                match changed {
                    "source" => {
                        fs::write(ctx.root().join("api/example.go"), "package changed\n").unwrap()
                    }
                    "journal" => fs::OpenOptions::new()
                        .append(true)
                        .open(ctx.state_file("receipts.jsonl"))
                        .unwrap()
                        .write_all(b"\n")
                        .unwrap(),
                    "configuration" => fs::OpenOptions::new()
                        .append(true)
                        .open(ctx.root().join(".jig.toml"))
                        .unwrap()
                        .write_all(b"\n# Example configuration observation changed\n")
                        .unwrap(),
                    _ => unreachable!(),
                }
            }
            false
        };
        let report = status(&ctx, &mutate).unwrap();
        assert!(changed_once.get());
        assert_eq!(metrics::snapshot().original_index_scans, 1);
        assert_eq!(metrics::snapshot().identity_collections, 1);
        assert!(
            report["work"]["gates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|plan| {
                    plan["snapshot"]["gates"][0]["freshness"] == "unknown"
                        && plan["snapshot"]["recovery"]["inspection"] == "unavailable"
                        && plan["snapshot"]["recovery"]["next_step"].is_null()
                }),
            "{changed}: {report:#}"
        );
    }
}

#[test]
fn inspection_cancellation_stops_before_using_a_retained_observation() {
    let temp = tempdir().unwrap();
    let ctx = checked_fixture(temp.path());
    add_plans(&ctx, 1);
    let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
    metrics::reset();
    let error = status(&ctx, &|| metrics::snapshot().identity_collections == 1).unwrap_err();
    assert!(
        crate::cancellation::is_status_collection_cancellation(&error),
        "{error:#}"
    );
    assert_eq!(metrics::snapshot().original_index_scans, 1);
    assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).unwrap());
}

#[test]
fn inspection_resource_ceiling_and_deadline_keep_read_only_compact_recovery() {
    for resource_limit in [false, true] {
        let temp = tempdir().unwrap();
        let ctx = checked_fixture(temp.path());
        if resource_limit {
            fs::OpenOptions::new()
                .append(true)
                .open(ctx.state_file("receipts.jsonl"))
                .unwrap()
                .write_all("\n".repeat(250_001).as_bytes())
                .unwrap();
        }
        let before = fs::read(ctx.state_file("receipts.jsonl")).unwrap();
        let summary = crate::runtime::call_tool_on_surface(&ctx, tool::WORK_GATES,
            json!({"plan_id":"plan_1", "freshness_timeout_ms": if resource_limit {30_000} else {1}}),
            ResponseSurface::AgentV1).unwrap();
        assert_eq!(summary["finish_ready"], false, "{summary:#}");
        assert_eq!(summary["gates"][0]["freshness"], "unknown");
        assert_eq!(summary["next_step"]["read_only"], true);
        assert_eq!(summary["next_step"]["argv"][2], "gates");
        assert_eq!(before, fs::read(ctx.state_file("receipts.jsonl")).unwrap());
        if resource_limit {
            let detail = crate::runtime::call_tool(
                &ctx,
                tool::WORK_GATES,
                json!({"plan_id":"plan_1", "freshness_timeout_ms":30_000}),
            )
            .unwrap();
            assert_eq!(
                detail["gates"][0]["freshness_collection"]["limit"], "resource",
                "{detail:#}"
            );
        }
    }
}

#[test]
fn inspection_never_shares_plan_bound_native_observations() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, true);
    native::configure_native(
        &ctx,
        "version=1\n[[rules]]\nid='source'\ninclude=['api/**']\nmax_lines=1000\n",
    );
    let ctx = RepoContext::load_from(ctx.root()).unwrap();
    add_plans(&ctx, 1);
    metrics::reset();
    let report = status(&ctx, &|| false).unwrap();
    assert_eq!(report["work"]["gates"].as_array().unwrap().len(), 2);
    assert_eq!(metrics::snapshot().original_index_scans, 2);
    assert_eq!(metrics::snapshot().identity_collections, 2);
}
