use super::*;

#[test]
fn cross_plan_target_sharing_keeps_legacy_tools_reviews_and_native_targets_local() {
    let shared: TargetId = "api:test".parse().unwrap();
    let native: TargetId = "repo:file-budget".parse().unwrap();
    let mut indexes = WorkGateReceiptIndexes::new(
        &BTreeSet::from(["plan_consumer".into()]),
        &BTreeSet::from([tool::TEST.into()]),
        &BTreeSet::from(["review".into()]),
        &BTreeMap::from([(
            "verify".into(),
            BTreeSet::from([shared.clone(), native.clone()]),
        )]),
        BTreeSet::from([shared.clone()]),
    );
    for plan in ["plan_consumer", "plan_other"] {
        for (kind, tool, target) in [
            ("check", tool::TEST, None),
            ("review", tool::WORK_REVIEW, None),
            ("shared", "jig.target_run", Some(shared.clone())),
            ("native", "jig.target_run", Some(native.clone())),
        ] {
            let mut receipt = test_receipt(
                &format!("{plan}_{kind}"),
                plan,
                tool,
                if plan == "plan_other" { 20 } else { 10 },
                json!({"gate_id": "review"}),
            );
            receipt.target = target;
            indexes.observe(&receipt);
        }
    }
    let index = indexes.into_indexes().remove("plan_consumer").unwrap();
    assert_eq!(
        index.tool_receipt(tool::TEST).unwrap().receipt_id,
        "plan_consumer_check"
    );
    assert_eq!(
        index.review_receipt("review").unwrap().receipt_id,
        "plan_consumer_review"
    );
    let targets = index.target_receipts("verify").unwrap();
    assert_eq!(targets[&shared].receipt_id, "plan_other_shared");
    assert_eq!(targets[&native].receipt_id, "plan_consumer_native");
}
