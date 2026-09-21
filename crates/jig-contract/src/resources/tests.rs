use super::*;
use crate::{ActionIntent, ActionRunner, ActionSpec, PlannedTarget, TargetRunResult};
use serde_json::json;

#[test]
fn cargo_resource_v1_round_trips_with_closed_nested_context() {
    let value = json!({"kind":"cargo_v1", "workspace_manifest":"example/Cargo.toml"});
    let resource: ExecutionResourceV1 = serde_json::from_value(value).unwrap();
    let ExecutionResourceV1::CargoV1 {
        working_directory,
        context,
        ..
    } = &resource
    else {
        panic!("expected Cargo resource");
    };
    assert!(working_directory.is_none());
    assert_eq!(context, &CargoImpactContextV1::default());
    assert_eq!(
        serde_json::from_value::<ExecutionResourceV1>(serde_json::to_value(&resource).unwrap())
            .unwrap(),
        resource
    );
    for value in [
        json!({"kind":"cargo_v2", "workspace_manifest":"Cargo.toml"}),
        json!({"kind":"cargo_v1", "workspace_manifest":"Cargo.toml", "global":true}),
        json!({"kind":"cargo_v1", "workspace_manifest":"Cargo.toml", "context":{"metadata_format_version":1,"locked":true,"offline":true,"unknown":true}}),
        json!({"kind":"cargo_v1"}),
    ] {
        assert!(serde_json::from_value::<ExecutionResourceV1>(value).is_err());
    }
}

#[test]
fn playwright_server_resource_is_fieldless_and_strict() {
    let value = json!({"kind":"playwright_servers_v1"});
    let resource: ExecutionResourceV1 = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(resource, ExecutionResourceV1::PlaywrightServersV1 {});
    assert_eq!(serde_json::to_value(resource).unwrap(), value);
    for invalid in [
        json!({"kind":"playwright_servers_v2"}),
        json!({"kind":"playwright_servers_v1", "workspace_manifest":"Cargo.toml"}),
        json!({"kind":"playwright_servers_v1", "ports":[4173,4174]}),
        json!({"kind":"playwright_servers_v1", "context":{}}),
    ] {
        assert!(serde_json::from_value::<ExecutionResourceV1>(invalid).is_err());
    }
}

#[test]
fn omitted_execution_resources_and_reuse_retain_the_legacy_wire_shape() {
    let target = "repo:test".parse().unwrap();
    let action = ActionSpec::new(
        target,
        ActionIntent::Check,
        ActionRunner::command("example_command"),
    );
    let plan = PlannedTarget::new(
        action.target.clone(),
        action.intent,
        action.runner.clone(),
        "input",
    );
    let result = TargetRunResult::queued(action.target.clone(), "config", "input");
    let action_value = serde_json::to_value(&action).unwrap();
    let plan_value = serde_json::to_value(&plan).unwrap();
    let result_value = serde_json::to_value(&result).unwrap();
    assert!(action_value.get("resources").is_none());
    assert!(plan_value.get("resources").is_none());
    assert!(result_value.get("reused_from").is_none());
    assert_eq!(
        serde_json::from_value::<ActionSpec>(action_value).unwrap(),
        action
    );
    assert_eq!(
        serde_json::from_value::<PlannedTarget>(plan_value).unwrap(),
        plan
    );
    assert_eq!(
        serde_json::from_value::<TargetRunResult>(result_value).unwrap(),
        result
    );
}

#[test]
fn resource_lists_round_trip_on_action_and_immutable_plan() {
    let resource: ExecutionResourceV1 = serde_json::from_value(
        json!({"kind":"cargo_v1", "workspace_manifest":"Cargo.toml", "working_directory":"."}),
    )
    .unwrap();
    let mut action = ActionSpec::new(
        "repo:test".parse().unwrap(),
        ActionIntent::Check,
        ActionRunner::command("example_command"),
    );
    action.resources = vec![resource, ExecutionResourceV1::PlaywrightServersV1 {}];
    let mut plan = PlannedTarget::new(
        action.target.clone(),
        action.intent,
        action.runner.clone(),
        "input",
    );
    plan.resources = action.resources.clone();
    assert_eq!(
        serde_json::from_value::<ActionSpec>(serde_json::to_value(&action).unwrap()).unwrap(),
        action
    );
    assert_eq!(
        serde_json::from_value::<PlannedTarget>(serde_json::to_value(&plan).unwrap()).unwrap(),
        plan
    );
    let mut invalid = serde_json::to_value(action).unwrap();
    invalid["resources"][0]["kind"] = json!("unsupported_v9");
    assert!(serde_json::from_value::<ActionSpec>(invalid).is_err());
}

#[test]
fn reuse_records_only_exact_original_evidence_identity() {
    let original = ReusedTargetEvidenceV1 {
        receipt_id: "receipt_example".into(),
        run_id: "run_example".into(),
        plan_id: "plan_example".into(),
    };
    let mut result = TargetRunResult::queued("repo:test".parse().unwrap(), "config", "input");
    result.reused_from = Some(original.clone());
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        value["reused_from"],
        json!({"receipt_id":"receipt_example", "run_id":"run_example", "plan_id":"plan_example"})
    );
    assert_eq!(
        serde_json::from_value::<TargetRunResult>(value).unwrap(),
        result
    );
    let mut invalid = serde_json::to_value(original).unwrap();
    invalid["new_receipt"] = json!(true);
    assert!(serde_json::from_value::<ReusedTargetEvidenceV1>(invalid).is_err());
    assert!(
        serde_json::from_value::<ReusedTargetEvidenceV1>(
            json!({"receipt_id":"receipt_example", "run_id":"run_example"})
        )
        .is_err()
    );
}
