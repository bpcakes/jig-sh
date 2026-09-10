use std::time::Duration;

use jig_contract::ActionInputsPolicy;
use jig_contract::freshness::DependencyIdentity;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::repository::freshness::CollectionLimits;

fn identity(target: &str) -> TargetIdentityV1 {
    let mut identity = TargetIdentityV1 {
        contract_epoch: 9,
        schema_version: 1,
        digest_domain: TARGET_IDENTITY_DOMAIN.into(),
        target: target.parse().unwrap(),
        inputs_policy: ActionInputsPolicy::Exhaustive,
        source_digest: "source".into(),
        authority_digest: "authority".into(),
        dependency_digest: "dependencies".into(),
        identity_digest: format!("identity:{target}"),
        configuration_digest: "configuration".into(),
        runner_digest: "runner".into(),
        invocation_digest: "invocation".into(),
        source_preview: vec![],
        source_entry_count: 0,
        source_preview_truncated: false,
        dependencies: vec![],
    };
    encode_identity(&mut identity);
    identity
}

fn encode_identity(identity: &mut TargetIdentityV1) {
    use crate::repository::freshness::encoding::IdentityEncoder;
    let mut dependencies =
        IdentityEncoder::new("jig-target-dependencies-v1", identity.contract_epoch);
    dependencies.number(identity.dependencies.len() as u64);
    for dependency in &identity.dependencies {
        dependencies.target(&dependency.target);
        dependencies.text(&dependency.identity_digest);
    }
    identity.dependency_digest = dependencies.finish();
    let mut complete = IdentityEncoder::new(TARGET_IDENTITY_DOMAIN, identity.contract_epoch);
    complete.target(&identity.target);
    complete.text(&identity.source_digest);
    complete.text(&identity.authority_digest);
    complete.text(&identity.dependency_digest);
    identity.identity_digest = complete.finish();
}

fn receipt(id: &str, target: &str, run: &str, started: u64, ended: u64) -> Value {
    json!({
        "id": id, "plan_id": "plan_example", "run_id": run,
        "target": target.parse::<jig_contract::TargetId>().unwrap(), "tool_name": "jig_target",
        "args": {}, "started_at_ms": started, "ended_at_ms": ended, "exit_status": 0,
        "stdout_preview": "", "stderr_preview": "", "changed_paths": [],
        "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
        "config_digest": "configuration", "input_digest": "legacy-input", "worktree_fingerprint": "global-source",
        "target_freshness": TargetFreshnessV1 {
            schema_version: 1, contract_epoch: 9, effective_valid_until_ms: None,
            effective_requires_time_validity: false,
            global_execution_proof: GlobalExecutionProofV1::Unchanged {
                before_source_digest: "global-source".into(), after_source_digest: "global-source".into(),
            },
            state: TargetFreshnessStateV1::Complete {
                identity: Box::new(identity(target)), dependency_execution_proof: vec![],
            },
        }
    })
}

fn depend(parent: &mut Value, child: &Value) {
    let dependency = DependencyIdentity {
        target: serde_json::from_value(child["target"].clone()).unwrap(),
        identity_digest: child["target_freshness"]["identity"]["identity_digest"]
            .as_str()
            .unwrap()
            .into(),
    };
    parent["target_freshness"]["identity"]["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(json!(dependency));
    let mut identity: TargetIdentityV1 =
        serde_json::from_value(parent["target_freshness"]["identity"].clone()).unwrap();
    encode_identity(&mut identity);
    parent["target_freshness"]["identity"] = json!(identity);
    parent["target_freshness"]["dependency_execution_proof"].as_array_mut().unwrap().push(json!({
        "target": child["target"], "receipt_id": child["id"], "run_id": child["run_id"],
        "plan_id": child["plan_id"], "identity_digest": dependency.identity_digest, "conclusion": "success",
        "effective_valid_until_ms": child["target_freshness"]["effective_valid_until_ms"],
        "effective_requires_time_validity": child["target_freshness"]["effective_requires_time_validity"],
    }));
    parent["target_freshness"]["effective_valid_until_ms"] =
        child["target_freshness"]["effective_valid_until_ms"].clone();
    parent["target_freshness"]["effective_requires_time_validity"] =
        child["target_freshness"]["effective_requires_time_validity"].clone();
}

fn journal(records: &[Value]) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let body = records
        .iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    std::fs::write(temp.path().join("receipts.jsonl"), body).unwrap();
    temp
}

fn evaluate(records: &[Value], selected_id: &str, now: u64) -> TargetFreshness {
    let temp = journal(records);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index =
        OriginalReceiptIndex::open(&temp.path().join("receipts.jsonl"), &mut budget).unwrap();
    let selected = index.get(selected_id, &mut budget).unwrap().unwrap();
    let expected = complete(&selected)
        .map(|(identity, _)| identity.clone())
        .unwrap_or_else(|| identity("web:test"));
    let mut validator = OriginalProofValidator::new(index, "plan_example", now);
    let result = validator.evaluate(&selected, &Ok(expected), &mut budget);
    validator.revalidate(&budget).unwrap();
    result
}

fn has(result: &TargetFreshness, code: Code) -> bool {
    result
        .reasons
        .reasons
        .iter()
        .any(|reason| reason.code == code)
}

#[test]
fn original_cross_run_chain_survives_newer_standalone_dependency_failure() {
    let original = receipt("receipt_original", "shared:verify", "run_one", 10, 20);
    let mut middle = receipt("receipt_middle", "api:test", "run_two", 30, 40);
    depend(&mut middle, &original);
    let mut parent = receipt("receipt_parent", "web:test", "run_three", 50, 60);
    depend(&mut parent, &middle);
    let mut newer = receipt("receipt_newer", "shared:verify", "run_four", 70, 80);
    newer["exit_status"] = json!(1);
    let result = evaluate(&[newer, parent, original, middle], "receipt_parent", 90);
    assert_eq!(result.status, Status::Fresh);
    assert_eq!(result.effective_valid_until_ms, None);
    assert!(!result.effective_requires_time_validity);
}

#[test]
fn inherited_boundary_expires_at_equality_and_cannot_be_extended() {
    let mut child = receipt("receipt_child", "api:test", "run_one", 10, 20);
    child["valid_until_ms"] = json!(100);
    child["target_freshness"]["effective_valid_until_ms"] = json!(100);
    child["target_freshness"]["effective_requires_time_validity"] = json!(true);
    let mut parent = receipt("receipt_parent", "web:test", "run_two", 30, 40);
    depend(&mut parent, &child);
    let records = [child.clone(), parent.clone()];
    let before = evaluate(&records, "receipt_parent", 99);
    assert_eq!(before.status, Status::Fresh);
    assert_eq!(before.effective_valid_until_ms, Some(100));
    assert!(before.effective_requires_time_validity);
    let expired = evaluate(&records, "receipt_parent", 100);
    assert_eq!(expired.status, Status::Stale);
    assert!(has(&expired, Code::TimeExpired));
    parent["target_freshness"]["effective_valid_until_ms"] = json!(200);
    let extended = evaluate(&[child.clone(), parent.clone()], "receipt_parent", 90);
    assert_eq!(extended.status, Status::Unknown);
    assert!(has(&extended, Code::DependencyProofInvalid));
    assert_eq!(extended.effective_valid_until_ms, Some(100));
    parent["started_at_ms"] = json!(100);
    parent["ended_at_ms"] = json!(101);
    parent["target_freshness"]["effective_valid_until_ms"] = json!(100);
    let invalid_at_execution = evaluate(&[child, parent], "receipt_parent", 102);
    assert!(has(&invalid_at_execution, Code::DependencyProofInvalid));
}

#[test]
fn every_dependency_reference_field_must_match_its_original() {
    let child = receipt("receipt_child", "api:test", "run_one", 10, 20);
    let mut parent = receipt("receipt_parent", "web:test", "run_two", 30, 40);
    depend(&mut parent, &child);
    for (field, value, reason) in [
        (
            "receipt_id",
            json!("receipt_missing"),
            Code::DependencyProofMissing,
        ),
        (
            "run_id",
            json!("run_different"),
            Code::DependencyProofInvalid,
        ),
        (
            "plan_id",
            json!("plan_different"),
            Code::DependencyProofInvalid,
        ),
        (
            "identity_digest",
            json!("different"),
            Code::DependencyProofInvalid,
        ),
        ("conclusion", json!("failure"), Code::DependencyProofInvalid),
        (
            "effective_valid_until_ms",
            json!(200),
            Code::DependencyProofInvalid,
        ),
        (
            "effective_requires_time_validity",
            json!(true),
            Code::DependencyProofInvalid,
        ),
        (
            "target",
            json!("other:test".parse::<jig_contract::TargetId>().unwrap()),
            Code::DependencyProofInvalid,
        ),
    ] {
        let mut parent = parent.clone();
        parent["target_freshness"]["dependency_execution_proof"][0][field] = value;
        let result = evaluate(&[child.clone(), parent], "receipt_parent", 90);
        assert_eq!(result.status, Status::Unknown, "{field}");
        assert!(has(&result, reason), "{field}: {result:?}");
    }
}

#[test]
fn failed_root_can_be_fresh_but_failed_or_late_dependencies_cannot_prove_execution() {
    let mut child = receipt("receipt_child", "api:test", "run_one", 10, 20);
    let mut parent = receipt("receipt_parent", "web:test", "run_two", 30, 40);
    depend(&mut parent, &child);
    parent["exit_status"] = json!(1);
    assert_eq!(
        evaluate(&[child.clone(), parent.clone()], "receipt_parent", 90).status,
        Status::Fresh
    );
    child["exit_status"] = json!(1);
    assert!(has(
        &evaluate(&[child.clone(), parent.clone()], "receipt_parent", 90),
        Code::DependencyProofInvalid
    ));
    child["exit_status"] = json!(0);
    child["ended_at_ms"] = json!(31);
    assert!(has(
        &evaluate(&[child, parent], "receipt_parent", 90),
        Code::DependencyProofInvalid
    ));
}

#[test]
fn missing_nonleaf_proof_cycles_and_cross_plan_originals_block() {
    let mut child = receipt("receipt_child", "api:test", "run_one", 10, 20);
    let mut parent = receipt("receipt_parent", "web:test", "run_two", 30, 40);
    depend(&mut parent, &child);
    let mut missing = parent.clone();
    missing["target_freshness"]["dependency_execution_proof"] = json!([]);
    assert!(has(
        &evaluate(&[child.clone(), missing.clone()], "receipt_parent", 90),
        Code::DependencyProofMissing
    ));
    missing["target_freshness"]["identity"]["dependencies"] = json!([]);
    assert!(has(
        &evaluate(&[child.clone(), missing], "receipt_parent", 90),
        Code::DependencyProofInvalid
    ));
    depend(&mut child, &parent);
    assert!(has(
        &evaluate(&[child.clone(), parent.clone()], "receipt_parent", 90),
        Code::DependencyProofInvalid
    ));
    child["target_freshness"]["dependency_execution_proof"] = json!([]);
    child["target_freshness"]["identity"]["dependencies"] = json!([]);
    child["plan_id"] = json!("plan_other");
    assert!(has(
        &evaluate(&[child, parent], "receipt_parent", 90),
        Code::DependencyProofInvalid
    ));
}

#[test]
fn legacy_future_old_and_mutated_metadata_never_use_matching_legacy_digests() {
    let original = receipt("receipt_parent", "web:test", "run_one", 10, 20);
    let mut legacy = original.clone();
    legacy.as_object_mut().unwrap().remove("target_freshness");
    let result = evaluate(&[legacy], "receipt_parent", 90);
    assert_eq!(result.status, Status::Unknown);
    assert!(has(&result, Code::LegacyMetadata));
    let mut future = original.clone();
    future["target_freshness"] = json!({"schema_version": 2, "new_proof": {"ready": true}});
    assert_eq!(
        evaluate(&[future], "receipt_parent", 90).status,
        Status::Unsupported
    );
    let mut older = original.clone();
    older["target_freshness"]["contract_epoch"] = json!(8);
    older["target_freshness"]["identity"]["contract_epoch"] = json!(8);
    let result = evaluate(&[older], "receipt_parent", 90);
    assert_eq!(result.status, Status::Stale);
    assert!(has(&result, Code::AuthorityVersionChanged));
    let mut mutated = original;
    mutated["target_freshness"]["global_execution_proof"] = json!({"state": "mutated"});
    assert!(has(
        &evaluate(&[mutated], "receipt_parent", 90),
        Code::ExecutionMutated
    ));
}

#[test]
fn location_index_detects_conflicts_replacement_and_shared_limits() {
    let original = receipt("receipt_original", "web:test", "run_one", 10, 20);
    let temp = journal(&[original.clone(), original.clone()]);
    let path = temp.path().join("receipts.jsonl");
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let index = OriginalReceiptIndex::open(&path, &mut budget).unwrap();
    std::fs::rename(&path, temp.path().join("previous.jsonl")).unwrap();
    std::fs::write(&path, format!("{original}\n{original}\n")).unwrap();
    assert_eq!(
        index.revalidate(&budget).unwrap_err().reason.code,
        Code::SourceRaced
    );
    let mut conflicting = original.clone();
    conflicting["exit_status"] = json!(1);
    std::fs::write(&path, format!("{original}\n{conflicting}\n")).unwrap();
    assert!(OriginalReceiptIndex::open(&path, &mut budget).is_err());
    let mut limits = CollectionLimits::with_timeout(Duration::from_secs(2));
    limits.bytes = 10;
    let mut small = CollectionBudget::new(limits, &|| false);
    assert!(
        matches!(OriginalReceiptIndex::open(&path, &mut small), Err(error) if error.reason.code == Code::CollectionLimit)
    );
}

#[test]
fn original_lookup_does_not_accept_selection_that_predates_a_newer_blocker() {
    let original = receipt("receipt_original", "web:test", "run_one", 10, 20);
    let temp = journal(std::slice::from_ref(&original));
    let path = temp.path().join("receipts.jsonl");
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index = OriginalReceiptIndex::open(&path, &mut budget).unwrap();
    let selected = index.get("receipt_original", &mut budget).unwrap().unwrap();
    let mut blocker = receipt("receipt_blocker", "web:test", "run_two", 30, 40);
    blocker["exit_status"] = json!(1);
    std::fs::write(&path, format!("{original}\n{blocker}\n")).unwrap();
    let originals =
        OriginalReceiptIndex::open_for_plan(&path, "plan_example", &mut budget).unwrap();
    let mut validator = OriginalProofValidator::new(originals, "plan_example", 50);
    let result = validator.evaluate(&selected, &Ok(identity("web:test")), &mut budget);
    assert_eq!(result.status, Status::Unknown);
    assert!(has(&result, Code::SourceRaced));
}

#[test]
fn transitive_missing_required_boundary_cannot_be_repaired_by_finite_sibling() {
    let mut missing = receipt("receipt_missing_time", "api:test", "run_one", 10, 20);
    missing["evidence"] = json!({"requires_time_validity": true});
    missing["target_freshness"]["effective_requires_time_validity"] = json!(true);
    let mut finite = receipt("receipt_finite", "shared:verify", "run_two", 10, 20);
    finite["valid_until_ms"] = json!(100);
    finite["target_freshness"]["effective_valid_until_ms"] = json!(100);
    finite["target_freshness"]["effective_requires_time_validity"] = json!(true);
    let mut middle = receipt("receipt_middle", "web:test", "run_three", 30, 40);
    depend(&mut middle, &missing);
    depend(&mut middle, &finite);
    middle["target_freshness"]["effective_valid_until_ms"] = Value::Null;
    let mut root = receipt("receipt_root", "workspace:verify", "run_four", 50, 60);
    depend(&mut root, &middle);
    let result = evaluate(&[missing, finite, middle, root], "receipt_root", 90);
    assert_eq!(result.status, Status::Unknown);
    assert!(has(&result, Code::TimeBoundaryMissing));
    assert_eq!(result.effective_valid_until_ms, None);
    assert!(result.effective_requires_time_validity);
}

#[test]
fn unverified_original_preserves_declared_time_constraints() {
    let child = receipt("receipt_child", "api:test", "run_one", 10, 20);
    for (boundary, required) in [(Some(100), true), (None, true), (None, false)] {
        let mut parent = receipt("receipt_parent", "web:test", "run_two", 30, 40);
        depend(&mut parent, &child);
        parent["target_freshness"]["effective_valid_until_ms"] = json!(boundary);
        parent["target_freshness"]["effective_requires_time_validity"] = json!(required);
        let result = evaluate(&[parent.clone()], "receipt_parent", 90);
        assert_eq!(result.status, Status::Unknown);
        assert!(has(&result, Code::DependencyProofMissing));
        assert_eq!(result.effective_valid_until_ms, boundary);
        assert_eq!(result.effective_requires_time_validity, required);
        let expired = evaluate(&[parent], "receipt_parent", 100);
        if boundary.is_some() {
            assert_eq!(expired.status, Status::Stale);
            assert!(has(&expired, Code::TimeExpired));
        }
    }
}
