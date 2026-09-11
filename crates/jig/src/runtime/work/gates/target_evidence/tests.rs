use super::*;

#[test]
fn freshness_limit_remedy_uses_actual_cause_for_any_inspection_budget() {
    use jig_contract::freshness::FreshnessCollectionLimit;
    let mut stats = FreshnessCollectionStats {
        limit: Some(FreshnessCollectionLimit::Resource),
        timeout_ms: 1,
        elapsed_us: 2_000_001,
        discovered_entries: 250_001,
        ..FreshnessCollectionStats::default()
    };
    let resource = collection_limit_reason(&stats);
    assert!(!resource.contains("--freshness-timeout-ms"));
    assert!(resource.contains("does not raise entry"));
    stats.limit = Some(FreshnessCollectionLimit::Deadline);
    let elapsed = collection_limit_reason(&stats);
    assert!(elapsed.contains("--freshness-timeout-ms 30000"));
    assert!(elapsed.contains("Resource ceilings are unchanged"));
    stats.timeout_ms = 30_000;
    assert!(!collection_limit_reason(&stats).contains("--freshness-timeout-ms"));
    assert!(collection_limit_reason(&stats).contains("maximum timeout"));
}

fn target_receipt(
    valid_until_ms: Option<u64>,
    requires_time_validity: bool,
) -> TargetReceiptStatus {
    TargetReceiptStatus {
        receipt_id: "receipt_target".into(),
        run_id: Some("run_target".into()),
        plan_id: Some("plan_example".into()),
        target_freshness: None,
        target: "repo:file-budget".parse().unwrap(),
        config_digest: Some("sha256:config".into()),
        input_digest: Some("sha256:input".into()),
        exit_status: 0,
        started_at_ms: 0,
        ended_at_ms: 1,
        changed_paths: Vec::new(),
        changed_path_count: 0,
        changed_paths_truncated: false,
        changed_paths_digest: None,
        diff_summary: String::new(),
        worktree_fingerprint: Some("fingerprint".into()),
        worktree_fingerprint_error: None,
        valid_until_ms,
        requires_time_validity,
    }
}

#[test]
fn target_evidence_enforces_time_validity_before_source_identity() {
    let current = CurrentWorktreeFingerprint {
        fingerprint: Some("fingerprint".into()),
        error: None,
    };
    let (expired, reason) = target_evidence_freshness(
        Some(&target_receipt(Some(0), true)),
        "sha256:config",
        Some("sha256:input"),
        &current,
    );
    assert_eq!(expired, GateFreshness::Stale);
    assert!(reason.contains("expired"));

    let (missing, reason) = target_evidence_freshness(
        Some(&target_receipt(None, true)),
        "sha256:config",
        Some("sha256:input"),
        &current,
    );
    assert_eq!(missing, GateFreshness::Unknown);
    assert!(reason.contains("no boundary"));
}
