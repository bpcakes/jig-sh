use super::*;

fn upgraded() -> Value {
    let mut record = receipt("receipt_worktree", "web:test", "run_worktree", 10, 20);
    let mut current = identity("web:test");
    current.contract_epoch = 8;
    current.source_state = Some(jig_contract::ActionSourceState::Worktree);
    encode_identity(&mut current);
    record["target_freshness"]["contract_epoch"] = json!(8);
    record["target_freshness"]["identity"] = json!(current);
    record
}

#[test]
fn current_epoch_proof_rejects_missing_wrong_and_future_source_authority() {
    let current = upgraded();
    assert_eq!(
        evaluate(std::slice::from_ref(&current), "receipt_worktree", 30).status,
        Status::Fresh
    );
    for (field, value) in [
        ("source_state", Value::Null),
        ("source_state", json!("git")),
        ("contract_epoch", json!(11)),
    ] {
        let mut invalid = current.clone();
        invalid["target_freshness"]["identity"][field] = value;
        assert_ne!(
            evaluate(&[invalid], "receipt_worktree", 30).status,
            Status::Fresh,
            "{field}"
        );
    }
    let mut malformed = current;
    malformed["target_freshness"]["identity"]["source_state"] = json!("future_state");
    let temp = journal(&[malformed]);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    assert!(OriginalReceiptIndex::open(&temp.path().join("receipts.jsonl"), &mut budget).is_err());
    let mut old = receipt("receipt_old", "web:test", "run_old", 10, 20);
    old["target_freshness"]["identity"]["source_state"] = json!("worktree");
    assert_ne!(evaluate(&[old], "receipt_old", 30).status, Status::Fresh);
}

#[test]
fn pre_release_epoch_nine_receipt_remains_readable_but_stale_after_v8_consolidation() {
    let mut old = receipt("receipt_old", "web:test", "run_old", 10, 20);
    old["target_freshness"]["contract_epoch"] = json!(9);
    old["target_freshness"]["identity"]["contract_epoch"] = json!(9);
    let mut old_identity: TargetIdentityV1 =
        serde_json::from_value(old["target_freshness"]["identity"].clone()).unwrap();
    old_identity.source_state = None;
    encode_identity(&mut old_identity);
    old["target_freshness"]["identity"] = json!(old_identity);
    let temp = journal(&[old]);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index =
        OriginalReceiptIndex::open(&temp.path().join("receipts.jsonl"), &mut budget).unwrap();
    let selected = index.get("receipt_old", &mut budget).unwrap().unwrap();
    let current: TargetIdentityV1 =
        serde_json::from_value(upgraded()["target_freshness"]["identity"].clone()).unwrap();
    let mut validator = OriginalProofValidator::new(index, "plan_example", 30);
    let result = validator.evaluate(&selected, &Ok(current), &mut budget);
    assert_eq!(result.status, Status::Stale);
    assert!(has(&result, Code::AuthorityVersionChanged));
    assert!(!has(&result, Code::DependencyProofInvalid));
}
