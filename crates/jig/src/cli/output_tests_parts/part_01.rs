#[test]
fn work_receipts_summary_handles_empty_results() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": []
    }));

    assert!(summary.contains("Showing: 0"));
    assert!(summary.contains("No receipts matched"));
}

#[test]
fn work_receipts_summary_omits_output_line_without_preview() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.test",
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": null
        }]
    }));

    assert!(summary.contains("jig.test (receipt_1): exit 0, no changes"));
    assert!(summary.contains("plan: none; session: none"));
    assert!(!summary.contains("output:"));
}

#[test]
fn state_diagnose_shallow_summary_does_not_imply_deep_cleanliness() {
    let summary = format_state_diagnose_summary(&json!({
        "deep": false,
        "totals": { "bytes": 42 },
        "sessions": null,
        "receipts": null
    }));

    assert!(summary.contains("Total bytes: 42"));
    assert!(summary.contains("Session recursion: not analyzed"));
    assert!(summary.contains("Receipt payloads: not analyzed"));
    assert!(!summary.contains("Recursive session records: 0"));
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
