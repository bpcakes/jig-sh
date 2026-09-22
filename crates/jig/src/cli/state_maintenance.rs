// State maintenance summaries. Included from `output_tests_parts.rs` inside
// the `cli::output::tests` module.

#[test]
fn state_diagnose_shallow_summary_does_not_imply_deep_cleanliness() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": false,
        "totals": { "bytes": 42 },
        "sessions": null,
        "receipts": null,
        "run_linkage": { "checked": false, "verdict": "not_checked" }
    }));

    assert!(summary.contains("State diagnose: complete (command status"));
    assert!(summary.contains("Integrity: run linkage not checked (rerun with --deep)"));
    assert!(summary.contains("Total bytes: 42"));
    assert!(summary.contains("Session recursion: not analyzed"));
    assert!(summary.contains("Receipt payloads: not analyzed"));
    assert!(summary.contains("Run linkage: not checked (rerun with --deep)"));
    assert!(!summary.contains("Recursive session records: 0"));
    assert!(!summary.contains("Run linkage: clean"));
}

#[test]
fn state_diagnose_summary_without_linkage_report_never_claims_a_clean_verdict() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": { "bytes": 1 }
    }));

    assert!(summary.contains("Integrity: run linkage not checked"));
    assert!(summary.contains("Run linkage: not checked (rerun with --deep)"));
}

#[test]
fn state_diagnose_deep_summary_reports_clean_run_linkage_with_counts() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": { "bytes": 1 },
        "integrity": { "malformed_records": 0, "torn_streams": 0, "scan_errors": 0 },
        "sessions": { "recursive_session_records": 0, "estimated_reclaimable_bytes": 0 },
        "run_linkage": {
            "checked": true,
            "verdict": "clean",
            "complete": true,
            "referenced_runs": 4,
            "runs": { "active": 1, "completed": 2, "archived_verified": 1 },
            "finding_count": 0
        }
    }));

    assert!(summary.contains("Integrity: run linkage clean"));
    assert!(
        summary
            .contains("Run linkage: clean (4 referenced runs: 1 active, 2 completed, 1 archived)")
    );
}

#[test]
fn state_diagnose_summary_lists_run_linkage_findings_and_affected_ids() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": { "bytes": 1 },
        "integrity": { "malformed_records": 1, "torn_streams": 0, "scan_errors": 0 },
        "sessions": { "recursive_session_records": 0, "estimated_reclaimable_bytes": 0 },
        "run_linkage": {
            "checked": true,
            "verdict": "findings",
            "complete": false,
            "finding_count": 7,
            "findings_truncated": true,
            "runs": { "missing": 5, "unverifiable": 1, "inconsistent": 0, "recoverable_from_backup": 1 },
            "findings": [{
                "run_id": "run_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "status": "missing",
                "receipt_ids": ["receipt_a, alias", "receipt_b", "receipt_c", "receipt_d"],
                "receipt_count": 6,
                "batch_receipt_ids": ["receipt_batch"],
                "batch_receipt_count": 1
            }, {
                "run_id": "run_01ARZ3NDEKTSV4RRFFQ69G5FB2",
                "status": "recoverable_from_backup",
                "receipt_ids": [],
                "receipt_count": 0,
                "batch_receipt_ids": [],
                "batch_receipt_count": 0
            }]
        },
        "recommendations": [{
            "kind": "preserve_unlinked_receipt_evidence",
            "reason": "6 receipt(s) reference 5 run(s) whose lifecycle is unavailable.",
            "command": "jig state export receipts --before <YYYY-MM-DD> --output receipts-preserved.jsonl.gz",
            "alternative_command": "jig work decide --title \"Run history unavailable\" --selected-option \"Preserve receipts; record affected IDs\" --rationale \"...\""
        }]
    }));

    assert!(summary.contains("Integrity: 1 malformed records; 7 run linkage findings"));
    assert!(summary.contains(
        "Run linkage: 7 finding(s) (5 missing, 1 unverifiable, 0 inconsistent, 1 recoverable from backup); scan incomplete, more may exist"
    ));
    assert!(summary.contains(
        "run_01ARZ3NDEKTSV4RRFFQ69G5FAV: missing; receipts: receipt_a, alias, receipt_b, receipt_c (+3 more); batch receipts: receipt_batch"
    ));
    assert!(summary.contains(
        "run_01ARZ3NDEKTSV4RRFFQ69G5FB2: recoverable_from_backup; receipts: none; batch receipts: none"
    ));
    assert!(summary.contains(
        "... 5 more finding(s); rerun with --json for structured findings and truncation metadata"
    ));
    assert!(!summary.contains("every affected ID"));
    assert!(summary.contains("Command: jig state export receipts"));
    assert!(summary.contains("Alternative: jig work decide"));
}

#[test]
fn state_diagnose_summary_reports_incomplete_linkage_without_a_clean_verdict() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": { "bytes": 1 },
        "integrity": { "scan_errors": 1 },
        "sessions": { "recursive_session_records": 0, "estimated_reclaimable_bytes": 0 },
        "run_linkage": {
            "checked": true,
            "verdict": "incomplete",
            "complete": false,
            "incomplete_reasons": ["run journal scan failed: is a directory"],
            "finding_count": 0
        }
    }));

    assert!(
        summary
            .contains("Integrity: 1 stream scan errors; run linkage incomplete (no clean verdict)")
    );
    assert!(summary.contains(
        "Run linkage: incomplete; no clean verdict (run journal scan failed: is a directory)"
    ));
    assert!(!summary.contains("Run linkage: clean"));
}

#[test]
fn state_diagnose_deep_summary_reports_compaction_opportunity() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": { "bytes": 1_000 },
        "sessions": {
            "recursive_session_records": 7,
            "estimated_reclaimable_bytes": 800
        }
    }));

    assert!(summary.contains("Recursive session records: 7"));
    assert!(summary.contains("Estimated reclaimable bytes: 800"));
}

#[test]
fn state_diagnose_summary_reports_cache_and_actionable_recommendations() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": true,
        "totals": {
            "checkout_state_bytes": 1_000,
            "maintenance_cache_bytes": 250,
            "local_disk_bytes": 1_250
        },
        "maintenance_cache": {
            "state_backups": { "bytes": 200 },
            "state_archives": { "bytes": 50 }
        },
        "sessions": {
            "recursive_session_records": 0,
            "estimated_reclaimable_bytes": 0
        },
        "recommendations": [{
            "reason": "Receipt state is large.",
            "command": "jig state archive --before <YYYY-MM-DD> --dry-run",
            "alternative_command": "jig state export receipts --before <YYYY-MM-DD> --output receipts.jsonl.gz"
        }]
    }));

    assert!(summary.contains("Total bytes: 1250"));
    assert!(summary.contains("State checkout bytes: 1000"));
    assert!(summary.contains("Maintenance cache bytes: 250"));
    assert!(summary.contains("State recovery backups: 200"));
    assert!(summary.contains("Receipt archives: 50"));
    assert!(summary.contains("Recommendations:"));
    assert!(summary.contains("state archive"));
    assert!(summary.contains("state export receipts"));
}

#[test]
fn state_compact_summary_distinguishes_noop_and_recovery_artifact() {
    let compacted = format_state_compact_summary(&json!({
        "dry_run": false,
        "records_changed": 3,
        "duplicate_records": 1,
        "bytes_before": 1_000,
        "bytes_after": 100,
        "bytes_reclaimable": 900,
        "source_sha256": "source-checksum",
        "backup_path": ".agent/.cache/state-backups/sessions-1"
    }));
    let noop = format_state_compact_summary(&json!({
        "dry_run": false,
        "records_changed": 0,
        "duplicate_records": 0,
        "bytes_before": 100,
        "bytes_after": 100,
        "backup_path": null
    }));

    assert!(compacted.contains("State compact sessions: compacted"));
    assert!(compacted.contains("Recovery backup: .agent/.cache/state-backups/sessions-1"));
    assert!(compacted.contains("Source SHA-256: source-checksum"));
    assert!(compacted.contains("local and ignored"));
    assert!(compacted.contains("does not remove reachable Git blobs"));
    assert!(noop.contains("State compact sessions: no-op"));
    assert!(noop.contains("state was already canonical"));
}

#[test]
fn state_restore_summary_reports_noop_checksum_and_recovery_path() {
    let restored = format_state_restore_summary(&json!({
        "stream": "sessions",
        "changed": true,
        "bytes_restored": 1_000,
        "backup_path": ".agent/.cache/state-backups/sessions-1",
        "sha256_restored": "restored-checksum",
        "recovery_backup_path": ".agent/.cache/state-backups/recovery-1"
    }));
    let noop = format_state_restore_summary(&json!({
        "stream": "sessions",
        "changed": false,
        "bytes_restored": 1_000,
        "recovery_backup_path": null
    }));

    assert!(restored.contains("State restore: restored"));
    assert!(restored.contains("Restored SHA-256: restored-checksum"));
    assert!(restored.contains("Replaced-state recovery backup: .agent/.cache"));
    assert!(restored.contains("local and ignored"));
    assert!(restored.contains("does not rewrite reachable Git blobs"));
    assert!(noop.contains("State restore: no-op"));
    assert!(noop.contains("Replaced-state recovery backup: not needed"));
}

#[test]
fn state_archive_and_export_summaries_report_storage_and_durability() {
    let archived = format_state_archive_summary(&json!({
        "dry_run": false,
        "before": "2026-01-01",
        "archive_path": ".agent/.cache/state-archives/receipts.jsonl.gz",
        "recovery_backup_path": ".agent/.cache/state-backups/receipts-1",
        "receipts_archived": 20,
        "receipts_retained": 5,
        "protected_receipts_retained": 2,
        "runs_included": true,
        "runs_archive_path": ".agent/.cache/state-archives/runs.jsonl.gz",
        "runs_recovery_backup_path": ".agent/.cache/state-backups/runs-1",
        "runs_archived": 3,
        "runs_retained": 4,
        "protected_runs_retained": 1,
        "uncompressed_bytes": 10_000,
        "compressed_bytes": 1_000,
        "sha256": "gzip-checksum",
        "content_sha256": "content-checksum"
    }));
    let exported = format_state_export_summary(&json!({
        "before": "2026-01-01",
        "output_path": "receipts.jsonl.gz",
        "receipts_exported": 20,
        "uncompressed_bytes": 10_000,
        "compressed_bytes": 1_000,
        "sha256": "gzip-checksum",
        "content_sha256": "content-checksum"
    }));

    assert!(archived.contains("State archive: archived"));
    assert!(archived.contains("Protected receipts retained: 2"));
    assert!(archived.contains("Runs archived: 3"));
    assert!(archived.contains("Protected runs retained: 1"));
    assert!(archived.contains("Run archive: .agent/.cache"));
    assert!(archived.contains("Exact runs recovery backup: .agent/.cache"));
    assert!(archived.contains("Compressed bytes: 1000"));
    assert!(archived.contains("Gzip SHA-256: gzip-checksum"));
    assert!(archived.contains("Exact pre-archive recovery backup: .agent/.cache"));
    assert!(archived.contains("not an off-machine backup"));
    assert!(archived.contains("does not remove reachable Git blobs"));

    assert!(exported.contains("State export receipts: exported"));
    assert!(exported.contains("Active state: unchanged"));
    assert!(exported.contains("JSONL SHA-256: content-checksum"));
    assert!(exported.contains("durability depends on the selected destination"));
    assert!(exported.contains("does not remove reachable Git blobs"));
}
