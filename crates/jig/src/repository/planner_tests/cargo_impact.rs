use std::fs;

use jig_contract::{CargoImpactReasonV1, CargoImpactV1, ComponentId, SourceIdentity};

use super::{reidentify, v7_file_budget_repository};
use crate::git_receipts::repository_source_snapshot;
use crate::repository::planner::{PlanRunRequest, plan_run, validate_run_plan};

#[test]
fn plan_validation_rejects_tampered_cargo_impact_facts() {
    let (_temp, ctx, catalog) = v7_file_budget_repository();
    let mut plan = plan_run(&ctx, &catalog, PlanRunRequest::default()).unwrap();
    plan.cargo_impacts.push(CargoImpactV1::unavailable(
        ComponentId::parse("repo").unwrap(),
        "Cargo.toml",
        Default::default(),
        CargoImpactReasonV1::MetadataMalformed,
    ));
    reidentify(&mut plan);

    let error = validate_run_plan(&ctx, &catalog, &plan)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("stale") || error.contains("modified"),
        "{error}"
    );
}

#[test]
fn source_mutation_after_cargo_discovery_is_rejected() {
    let (_temp, ctx, _catalog) = v7_file_budget_repository();
    let snapshot = repository_source_snapshot(ctx.root()).unwrap();
    let expected = SourceIdentity::new(snapshot.head_commit, snapshot.worktree_fingerprint);
    fs::write(ctx.root().join("source.rs"), "fn changed() {}\n").unwrap();

    let error = super::super::validate_source_after_cargo_discovery(&ctx, &expected)
        .unwrap_err()
        .to_string();
    assert!(error.contains("source changed while discovering Cargo metadata"));
}
