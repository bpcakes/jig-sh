use super::*;

fn open_reuse(
    path: &std::path::Path,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<OriginalReceiptIndex> {
    OriginalReceiptIndex::open_for_work_reuse(
        path,
        "plan_consumer",
        &BTreeSet::from([
            "api:test".parse().unwrap(),
            "web:test".parse().unwrap(),
            "other:test".parse().unwrap(),
        ]),
        budget,
    )
}

fn work_receipt(id: &str, target: &str, plan: &str, started: u64, ended: u64) -> Value {
    let mut receipt = receipt(id, target, "run_example", started, ended);
    receipt["plan_id"] = json!(plan);
    receipt
}

fn evaluate_reuse(records: &[Value], selected_id: &str) -> TargetFreshness {
    let temp = journal(records);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index = open_reuse(&temp.path().join("receipts.jsonl"), &mut budget).unwrap();
    let selected = index.get(selected_id, &mut budget).unwrap().unwrap();
    let expected = complete(&selected)
        .map(|(identity, _)| identity.clone())
        .unwrap_or_else(|| identity("web:test"));
    let mut validator = OriginalProofValidator::for_work_reuse(index, 90);
    let result = validator.evaluate(&selected, &Ok(expected), &mut budget);
    validator.revalidate(&budget).unwrap();
    result
}

#[test]
fn work_reuse_validates_foreign_roots_and_retains_original_dependency_provenance() {
    let child = work_receipt("receipt_child", "api:test", "plan_first", 10, 20);
    let mut parent = work_receipt("receipt_parent", "web:test", "plan_first", 30, 40);
    depend(&mut parent, &child);
    let other = work_receipt("receipt_other", "other:test", "plan_second", 50, 60);
    let newer_child = work_receipt("receipt_newer", "api:test", "plan_second", 70, 80);
    let temp = journal(&[child, parent.clone(), other.clone(), newer_child]);
    let path = temp.path().join("receipts.jsonl");
    let before = std::fs::read(&path).unwrap();
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index = open_reuse(&path, &mut budget).unwrap();
    let selected: Vec<_> = [&parent, &other]
        .map(|record| {
            index
                .get(record["id"].as_str().unwrap(), &mut budget)
                .unwrap()
                .unwrap()
        })
        .into();
    let mut validator = OriginalProofValidator::for_work_reuse(index, 90);
    for (selected, original) in selected.iter().zip([&parent, &other]) {
        let expected = complete(selected).unwrap().0.clone();
        let result = validator.evaluate(selected, &Ok(expected), &mut budget);
        assert_eq!(result.status, Status::Fresh, "{result:?}");
        assert_eq!(selected.plan_id.as_deref(), original["plan_id"].as_str());
        assert_eq!(selected.receipt_id, original["id"].as_str().unwrap());
    }
    validator.revalidate(&budget).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn work_reuse_rejects_mismatched_reference_and_cross_plan_dependency_edges() {
    for foreign_child in [false, true] {
        let child_plan = if foreign_child {
            "plan_second"
        } else {
            "plan_first"
        };
        let child = work_receipt("receipt_child", "api:test", child_plan, 10, 20);
        let mut parent = work_receipt("receipt_parent", "web:test", "plan_first", 30, 40);
        depend(&mut parent, &child);
        if !foreign_child {
            parent["target_freshness"]["dependency_execution_proof"][0]["plan_id"] =
                json!("plan_second");
        }
        let result = evaluate_reuse(&[child, parent], "receipt_parent");
        assert!(has(&result, Code::DependencyProofInvalid), "{result:?}");
    }
}

#[test]
fn work_reuse_does_not_resurrect_pass_before_foreign_failure_or_unknown_outcome() {
    let passed = work_receipt("receipt_pass", "web:test", "plan_first", 10, 20);
    for unknown in [false, true] {
        let mut blocker = work_receipt("receipt_blocker", "web:test", "plan_second", 30, 40);
        if unknown {
            blocker.as_object_mut().unwrap().remove("target_freshness");
        } else {
            blocker["exit_status"] = json!(7);
        }
        // An older pass never wins, even if it is physically later in the journal.
        let records = [blocker.clone(), passed.clone()];
        let result = evaluate_reuse(&records, "receipt_pass");
        assert!(has(&result, Code::SourceRaced), "{result:?}");
        if unknown {
            assert!(has(
                &evaluate_reuse(&records, "receipt_blocker"),
                Code::LegacyMetadata
            ));
        }
        let temp = journal(&records);
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(2)),
            &|| false,
        );
        let mut index = open_reuse(&temp.path().join("receipts.jsonl"), &mut budget).unwrap();
        let selected = index.get("receipt_blocker", &mut budget).unwrap().unwrap();
        assert!(index.selected_is_current(&selected));
        assert_eq!(selected.exit_status, if unknown { 0 } else { 7 });
    }
}

#[test]
fn work_reuse_timestamp_ties_match_receipt_ordering_across_plans() {
    let older = work_receipt("receipt_a", "web:test", "plan_first", 10, 20);
    let newer = work_receipt("receipt_z", "web:test", "plan_second", 10, 20);
    let records = [newer, older];
    assert!(has(
        &evaluate_reuse(&records, "receipt_a"),
        Code::SourceRaced
    ));
    assert_eq!(evaluate_reuse(&records, "receipt_z").status, Status::Fresh);
}

#[test]
fn work_reuse_revalidates_journal_after_a_foreign_outcome_is_appended() {
    use std::io::Write;

    let passed = work_receipt("receipt_pass", "web:test", "plan_first", 10, 20);
    let temp = journal(&[passed]);
    let path = temp.path().join("receipts.jsonl");
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index = open_reuse(&path, &mut budget).unwrap();
    let selected = index.get("receipt_pass", &mut budget).unwrap().unwrap();
    let mut validator = OriginalProofValidator::for_work_reuse(index, 90);
    assert_eq!(
        validator.evaluate_original(&selected, &mut budget).status,
        Status::Fresh
    );
    let newer = work_receipt("receipt_newer", "web:test", "plan_second", 30, 40);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{newer}").unwrap();
    assert!(
        matches!(validator.revalidate(&budget), Err(error) if error.reason.code == Code::SourceRaced)
    );
    // A new index also rejects a selection retained from before the append.
    let index = open_reuse(&path, &mut budget).unwrap();
    let mut validator = OriginalProofValidator::for_work_reuse(index, 90);
    assert!(has(
        &validator.evaluate_original(&selected, &mut budget),
        Code::SourceRaced
    ));
}

#[test]
fn work_reuse_rejects_selected_receipt_with_rewritten_plan_provenance() {
    let passed = work_receipt("receipt_pass", "web:test", "plan_first", 10, 20);
    let temp = journal(&[passed]);
    let path = temp.path().join("receipts.jsonl");
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index = open_reuse(&path, &mut budget).unwrap();
    let mut selected = index.get("receipt_pass", &mut budget).unwrap().unwrap();
    selected.plan_id = Some("plan_second".into());
    let mut validator = OriginalProofValidator::for_work_reuse(index, 90);
    assert!(has(
        &validator.evaluate_original(&selected, &mut budget),
        Code::SourceRaced
    ));
}

#[test]
fn work_reuse_keeps_plan_sensitive_outcomes_local_and_shared_blockers_global() {
    let local_native = work_receipt(
        "receipt_local_native",
        "repo:budget",
        "plan_consumer",
        10,
        20,
    );
    let foreign_native = work_receipt(
        "receipt_foreign_native",
        "repo:budget",
        "plan_other",
        30,
        40,
    );
    let local_shared = work_receipt("receipt_local_shared", "web:test", "plan_consumer", 10, 20);
    let mut foreign_shared =
        work_receipt("receipt_foreign_shared", "web:test", "plan_other", 30, 40);
    foreign_shared["exit_status"] = json!(7);
    let temp = journal(&[local_native, foreign_native, local_shared, foreign_shared]);
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let mut index = open_reuse(&temp.path().join("receipts.jsonl"), &mut budget).unwrap();
    let local_native = index
        .get("receipt_local_native", &mut budget)
        .unwrap()
        .unwrap();
    let foreign_native = index
        .get("receipt_foreign_native", &mut budget)
        .unwrap()
        .unwrap();
    let local_shared = index
        .get("receipt_local_shared", &mut budget)
        .unwrap()
        .unwrap();
    let foreign_shared = index
        .get("receipt_foreign_shared", &mut budget)
        .unwrap()
        .unwrap();
    assert!(index.selected_is_current(&local_native));
    assert!(!index.selected_is_current(&foreign_native));
    assert!(!index.selected_is_current(&local_shared));
    assert!(index.selected_is_current(&foreign_shared));
    assert_eq!(foreign_shared.exit_status, 7);
    let mut validator = OriginalProofValidator::for_work_reuse(index, 90);
    assert_eq!(
        validator
            .evaluate_original(&local_native, &mut budget)
            .status,
        Status::Fresh
    );
    assert!(has(
        &validator.evaluate_original(&foreign_native, &mut budget),
        Code::SourceRaced
    ));
    assert!(has(
        &validator.evaluate_original(&local_shared, &mut budget),
        Code::SourceRaced
    ));
    validator.revalidate(&budget).unwrap();
}
