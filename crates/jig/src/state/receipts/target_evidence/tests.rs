use super::*;

fn receipt(run: &str, target: &str, ended: u64, exit_status: i32) -> TargetReceiptStatus {
    TargetReceiptStatus {
        receipt_id: format!("receipt_{run}_{target}_{ended}"),
        run_id: Some(run.into()),
        plan_id: Some("plan_example".into()),
        target_freshness: None,
        target: target.parse().unwrap(),
        config_digest: Some("config".into()),
        input_digest: Some("inputs".into()),
        exit_status,
        started_at_ms: ended,
        ended_at_ms: ended,
        changed_paths: Vec::new(),
        changed_path_count: 0,
        changed_paths_truncated: false,
        changed_paths_digest: None,
        diff_summary: String::new(),
        worktree_fingerprint: Some("source".into()),
        worktree_fingerprint_error: None,
        valid_until_ms: None,
        requires_time_validity: false,
    }
}

fn index() -> IndexedTargetReceipts {
    IndexedTargetReceipts::new(BTreeSet::from([
        "api:test".parse().unwrap(),
        "web:test".parse().unwrap(),
    ]))
}

#[test]
fn targeted_retry_keeps_original_provenance_for_other_targets() {
    let mut index = index();
    let web = receipt("full", "web:test", 1, 0);
    index.observe(&web);
    index.observe(&receipt("full", "api:test", 2, 1));
    let retry = receipt("retry", "api:test", 3, 0);
    index.observe(&retry);
    assert_eq!(index.selected().len(), 2);
    let api = &index.selected()[&"api:test".parse().unwrap()];
    assert_eq!(api.receipt_id, retry.receipt_id);
    assert_eq!(api.run_id.as_deref(), Some("retry"));
    let retained = &index.selected()[&"web:test".parse().unwrap()];
    assert_eq!(retained.receipt_id, web.receipt_id);
    assert_eq!(retained.run_id.as_deref(), Some("full"));
}

#[test]
fn newer_failure_and_unknown_freshness_do_not_fall_back_to_success() {
    let mut index = index();
    index.observe(&receipt("pass", "api:test", 1, 0));
    let mut failed = receipt("failure", "api:test", 2, 7);
    failed.input_digest = None;
    index.observe(&failed);
    index.observe(&receipt("pass", "api:test", 1, 0));
    let selected = &index.selected()[&"api:test".parse().unwrap()];
    assert_eq!(selected.exit_status, 7);
    assert_eq!(selected.input_digest, None);
    assert_eq!(selected.receipt_id, failed.receipt_id);
}

#[test]
fn ordering_and_duplicates_preserve_newest_receipt_at_constant_memory() {
    let mut forward = index();
    let mut reverse = index();
    for ended in 0..10_000 {
        forward.observe(&receipt("history", "api:test", ended, 0));
        let old = receipt("history", "api:test", 9_999 - ended, 0);
        reverse.observe(&old);
        reverse.observe(&old);
        assert_eq!(forward.selected().len(), 1);
        assert_eq!(reverse.selected().len(), 1);
    }
    let target = "api:test".parse().unwrap();
    assert_eq!(
        forward.selected()[&target].receipt_id,
        reverse.selected()[&target].receipt_id
    );
    assert_eq!(forward.selected()[&target].ended_at_ms, 9_999);
}

#[test]
fn timestamp_ties_use_receipt_identity_and_ignore_unrequired_targets() {
    let mut forward = index();
    let mut reverse = index();
    let a = receipt("a", "api:test", 1, 0);
    let b = receipt("b", "api:test", 1, 1);
    for row in [&a, &b] {
        forward.observe(row);
    }
    for row in [&b, &a] {
        reverse.observe(row);
    }
    forward.observe(&receipt("foreign", "other:test", 100, 0));
    let target = "api:test".parse().unwrap();
    assert_eq!(forward.selected()[&target].receipt_id, b.receipt_id);
    assert_eq!(reverse.selected()[&target].receipt_id, b.receipt_id);
    assert_eq!(forward.selected().len(), 1);
}
