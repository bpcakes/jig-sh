// State maintenance summaries. Included from `output_tests_parts.rs` inside
// the `cli::output::tests` module.

#[test]
fn state_diagnose_summary_reports_integrity_without_claiming_run_linkage() {
    let clean = format_state_diagnose_summary(&json!({
        "totals": { "bytes": 42 },
        "integrity": { "malformed_records": 0, "torn_streams": 0, "scan_errors": 0 }
    }));
    let damaged = format_state_diagnose_summary(&json!({
        "totals": { "bytes": 42 },
        "integrity": { "malformed_records": 2, "torn_streams": 1, "scan_errors": 0 }
    }));

    assert!(clean.contains("State diagnose: complete (command status"));
    assert!(clean.contains("Integrity: no malformed, torn, or unreadable streams"));
    assert!(clean.contains("Total bytes: 42"));
    assert!(!clean.contains("--deep"));
    assert!(!clean.contains("Run linkage"));
    assert!(!clean.contains("Receipt"));
    assert!(damaged.contains("Integrity: 2 malformed records; 1 torn streams"));
}

#[test]
fn state_diagnose_summary_reports_cache_and_actionable_recommendations() {
    let summary = format_state_diagnose_summary(&json!({
        "totals": {
            "checkout_state_bytes": 1_000,
            "maintenance_cache_bytes": 250,
            "local_disk_bytes": 1_250
        },
        "maintenance_cache": {
            "state_backups": { "bytes": 200 },
            "state_archives": { "bytes": 50 }
        },
        "recommendations": [{
            "reason": "Run state is large.",
            "command": "jig state archive --before <YYYY-MM-DD> --dry-run"
        }]
    }));

    assert!(summary.contains("Total bytes: 1250"));
    assert!(summary.contains("State checkout bytes: 1000"));
    assert!(summary.contains("Maintenance cache bytes: 250"));
    assert!(summary.contains("State recovery backups: 200"));
    assert!(summary.contains("State archives: 50"));
    assert!(summary.contains("Recommendations:"));
    assert!(summary.contains("Command: jig state archive --before <YYYY-MM-DD> --dry-run"));
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
fn state_archive_summary_reports_runs_storage_and_durability() {
    let archived = format_state_archive_summary(&json!({
        "dry_run": false,
        "before": "2026-01-01",
        "runs_archive_path": ".agent/.cache/state-archives/runs.jsonl.gz",
        "runs_recovery_backup_path": ".agent/.cache/state-backups/runs-1",
        "runs_archived": 3,
        "runs_retained": 4,
        "runs_uncompressed_bytes": 10_000,
        "runs_compressed_bytes": 1_000,
        "runs_content_sha256": "content-checksum"
    }));
    let preview = format_state_archive_summary(&json!({
        "dry_run": true,
        "before": "2026-01-01",
        "runs_archived": 0,
        "runs_retained": 4
    }));

    assert!(archived.contains("State archive: archived"));
    assert!(archived.contains("Runs archived: 3"));
    assert!(archived.contains("Runs retained: 4"));
    assert!(archived.contains("Active state changed: yes"));
    assert!(archived.contains("Run archive: .agent/.cache"));
    assert!(archived.contains("Exact runs recovery backup: .agent/.cache"));
    assert!(archived.contains("Compressed bytes: 1000"));
    assert!(archived.contains("JSONL SHA-256: content-checksum"));
    assert!(archived.contains("not an off-machine backup"));
    assert!(archived.contains("does not remove reachable Git blobs"));
    assert!(!archived.contains("Receipt"));
    assert!(preview.contains("State archive: dry run (no eligible runs)"));
    assert!(preview.contains("Run archive: not written during dry run"));
}
