use super::{value_bool, value_str, value_u64};

pub(super) fn format_state_summary(value: &serde_json::Value) -> String {
    let counts = &value["counts"];
    let repo = &value["repo"];
    let repo_name = value_str(repo, "name").unwrap_or("<unknown>");
    let sessions = value_u64(counts, "sessions").unwrap_or(0);
    let session_events = value_u64(counts, "session_events").unwrap_or(0);
    let plans = value_u64(counts, "plans").unwrap_or(0);
    let plan_events = value_u64(counts, "plan_events").unwrap_or(0);
    let open_plans = value_u64(counts, "open_plans").unwrap_or(0);
    let receipts = value_u64(counts, "receipts").unwrap_or(0);
    let failed_receipts = value_u64(counts, "failed_receipts").unwrap_or(0);
    let decisions = value_u64(counts, "decisions").unwrap_or(0);

    [
        "State summary:".into(),
        format!("  Sessions: {sessions} ({session_events} events)"),
        format!("  Plans: {plans} ({open_plans} open, {plan_events} events)"),
        format!("  Receipts: {receipts} ({failed_receipts} failed)"),
        format!("  Decisions: {decisions}"),
        format!("Repo: {repo_name}"),
        format!(
            "Current session: {}",
            value_str(value, "current_session_id").unwrap_or("none")
        ),
    ]
    .join("\n")
}

pub(super) fn format_state_diagnose_summary(value: &serde_json::Value) -> String {
    let checkout_bytes = value["totals"]["checkout_state_bytes"]
        .as_u64()
        .or_else(|| value["totals"]["bytes"].as_u64())
        .unwrap_or(0);
    let cache_bytes = value["totals"]["maintenance_cache_bytes"]
        .as_u64()
        .or_else(|| value["maintenance_cache"]["bytes"].as_u64())
        .unwrap_or(0);
    let total_bytes = value["totals"]["local_disk_bytes"]
        .as_u64()
        .unwrap_or_else(|| checkout_bytes.saturating_add(cache_bytes));
    let mut lines = vec![
        "State diagnose: complete".to_string(),
        format!("  Total bytes: {total_bytes}"),
        format!("  State checkout bytes: {checkout_bytes}"),
        format!("  Maintenance cache bytes: {cache_bytes}"),
    ];
    if value.get("maintenance_cache").is_some() {
        let backup_bytes = value["maintenance_cache"]["state_backups"]["bytes"]
            .as_u64()
            .unwrap_or(0);
        let archive_bytes = value["maintenance_cache"]["state_archives"]["bytes"]
            .as_u64()
            .unwrap_or(0);
        lines.push(format!("    State recovery backups: {backup_bytes}"));
        lines.push(format!("    Receipt archives: {archive_bytes}"));
    }
    if value_bool(value, "deep").unwrap_or(false) {
        let recursive = value["sessions"]["recursive_session_records"]
            .as_u64()
            .unwrap_or(0);
        let reclaimable = value["sessions"]["estimated_reclaimable_bytes"]
            .as_u64()
            .unwrap_or(0);
        lines.push(format!("  Recursive session records: {recursive}"));
        lines.push(format!("  Estimated reclaimable bytes: {reclaimable}"));
    } else {
        lines.push("  Session recursion: not analyzed (rerun with --deep)".into());
        lines.push("  Receipt payloads: not analyzed (rerun with --deep)".into());
    }
    if let Some(recommendations) = value["recommendations"].as_array() {
        if !recommendations.is_empty() {
            lines.push("Recommendations:".into());
            for recommendation in recommendations {
                let reason = value_str(recommendation, "reason").unwrap_or("Review state health.");
                lines.push(format!("  - {reason}"));
                if let Some(command) = value_str(recommendation, "command") {
                    lines.push(format!("    Command: {command}"));
                }
                if let Some(command) = value_str(recommendation, "alternative_command") {
                    lines.push(format!("    Alternative: {command}"));
                }
            }
        }
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_state_compact_summary(value: &serde_json::Value) -> String {
    let dry_run = value_bool(value, "dry_run").unwrap_or(false);
    let changed = value_u64(value, "records_changed").unwrap_or(0);
    let duplicates = value_u64(value, "duplicate_records").unwrap_or(0);
    let has_changes = changed > 0 || duplicates > 0;
    let before = value_u64(value, "bytes_before").unwrap_or(0);
    let after = value_u64(value, "bytes_after").unwrap_or(0);
    let status = match (dry_run, has_changes) {
        (true, true) => "dry run (changes available)",
        (true, false) => "dry run (no changes)",
        (false, true) => "compacted",
        (false, false) => "no-op",
    };
    let mut lines = vec![
        format!("State compact sessions: {status}"),
        format!("  Records changed: {changed}"),
        format!("  Duplicate records removed: {duplicates}"),
        format!("  Bytes: {before} -> {after}"),
    ];
    if let Some(reclaimable) = value_u64(value, "bytes_reclaimable") {
        lines.push(format!("  Bytes reclaimable: {reclaimable}"));
    }
    if let Some(checksum) = value_str(value, "source_sha256") {
        lines.push(format!("  Source SHA-256: {checksum}"));
    }
    match value_str(value, "backup_path") {
        Some(backup) => lines.push(format!("  Recovery backup: {backup}")),
        None if dry_run => lines.push("  Recovery backup: not written during dry run".into()),
        None => lines.push("  Recovery backup: not written; state was already canonical".into()),
    }
    lines.push(
        "  Cache durability: recovery backups under .agent/.cache are local and ignored; copy them elsewhere for durable recovery."
            .into(),
    );
    lines
        .push("  Git history: working-tree compaction does not remove reachable Git blobs.".into());
    if let Some(note) = value_str(value, "writer_coordination_note") {
        lines.push(format!("  Writer coordination: {note}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_state_restore_summary(value: &serde_json::Value) -> String {
    let stream = value_str(value, "stream").unwrap_or("<unknown>");
    let bytes = value_u64(value, "bytes_restored").unwrap_or(0);
    let changed = value_bool(value, "changed").unwrap_or(true);
    let mut lines = vec![
        format!(
            "State restore: {}",
            if changed { "restored" } else { "no-op" }
        ),
        format!("  Stream: {stream}"),
        format!("  Bytes restored: {bytes}"),
    ];
    if let Some(backup) = value_str(value, "backup_path") {
        lines.push(format!("  Source backup: {backup}"));
    }
    if let Some(checksum) = value_str(value, "sha256_restored") {
        lines.push(format!("  Restored SHA-256: {checksum}"));
    }
    match value_str(value, "recovery_backup_path") {
        Some(path) => lines.push(format!("  Replaced-state recovery backup: {path}")),
        None if changed => lines.push("  Replaced-state recovery backup: unavailable".into()),
        None => lines.push("  Replaced-state recovery backup: not needed".into()),
    }
    lines.push(
        "  Cache durability: backup and recovery paths under .agent/.cache are local and ignored; copy them elsewhere for durable recovery."
            .into(),
    );
    lines.push(
        "  Git history: restore changes working-tree state only; it does not rewrite reachable Git blobs."
            .into(),
    );
    if let Some(note) = value_str(value, "writer_coordination_note") {
        lines.push(format!("  Writer coordination: {note}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_state_export_summary(value: &serde_json::Value) -> String {
    let exported = value_u64(value, "receipts_exported").unwrap_or(0);
    let path = value_str(value, "output_path").unwrap_or("<unknown>");
    let mut lines = vec![
        format!(
            "State export receipts: {}",
            if exported > 0 {
                "exported"
            } else {
                "empty export"
            }
        ),
        format!("  Output: {path}"),
        format!("  Receipts exported: {exported}"),
    ];
    if let Some(before) = value_str(value, "before") {
        lines.push(format!("  Before: {before}"));
    }
    if let Some(bytes) = value_u64(value, "uncompressed_bytes") {
        lines.push(format!("  Uncompressed bytes: {bytes}"));
    }
    if let Some(bytes) = value_u64(value, "compressed_bytes") {
        lines.push(format!("  Compressed bytes: {bytes}"));
    }
    if let Some(checksum) = value_str(value, "sha256") {
        lines.push(format!("  Gzip SHA-256: {checksum}"));
    }
    if let Some(checksum) = value_str(value, "content_sha256") {
        lines.push(format!("  JSONL SHA-256: {checksum}"));
    }
    lines.push("  Active state: unchanged; export is non-mutating.".into());
    lines.push(
        "  Cache durability: exports are not managed by Jig's local cache; durability depends on the selected destination."
            .into(),
    );
    lines.push("  Git history: export does not remove reachable Git blobs.".into());
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

pub(super) fn format_state_archive_summary(value: &serde_json::Value) -> String {
    let dry_run = value_bool(value, "dry_run").unwrap_or(false);
    let archived = value_u64(value, "receipts_archived").unwrap_or(0);
    let retained = value_u64(value, "receipts_retained").unwrap_or(0);
    let before = value_str(value, "before").unwrap_or("<unknown>");
    let status = match (dry_run, archived > 0) {
        (true, true) => "dry run (changes available)",
        (true, false) => "dry run (no eligible receipts)",
        (false, true) => "archived",
        (false, false) => "no-op",
    };
    let mut lines = vec![
        format!("State archive: {status}"),
        format!("  Before: {before}"),
        format!("  Receipts archived: {archived}"),
        format!("  Receipts retained: {retained}"),
        format!(
            "  Active state changed: {}",
            if !dry_run && archived > 0 {
                "yes"
            } else {
                "no"
            }
        ),
    ];
    if let Some(protected) = value_u64(value, "protected_receipts_retained") {
        lines.push(format!("  Protected receipts retained: {protected}"));
    }
    match value_str(value, "archive_path") {
        Some(path) => lines.push(format!("  Local archive: {path}")),
        None if dry_run => lines.push("  Local archive: not written during dry run".into()),
        None => lines.push("  Local archive: not written; no receipts were eligible".into()),
    }
    match value_str(value, "recovery_backup_path") {
        Some(path) => lines.push(format!("  Exact pre-archive recovery backup: {path}")),
        None if dry_run => {
            lines.push("  Exact pre-archive recovery backup: not written during dry run".into());
        }
        None => lines.push(
            "  Exact pre-archive recovery backup: not written; active state was unchanged".into(),
        ),
    }
    if let Some(bytes) = value_u64(value, "uncompressed_bytes") {
        lines.push(format!("  Uncompressed bytes: {bytes}"));
    }
    if let Some(bytes) = value_u64(value, "compressed_bytes") {
        lines.push(format!("  Compressed bytes: {bytes}"));
    }
    if let Some(checksum) = value_str(value, "sha256") {
        lines.push(format!("  Gzip SHA-256: {checksum}"));
    }
    if let Some(checksum) = value_str(value, "content_sha256") {
        lines.push(format!("  JSONL SHA-256: {checksum}"));
    }
    lines.push(
        "  Cache durability: local archives under .agent/.cache are ignored and are not an off-machine backup."
            .into(),
    );
    lines.push(
        "  Git history: archiving shrinks active state only; it does not remove reachable Git blobs."
            .into(),
    );
    if let Some(note) = value_str(value, "writer_coordination_note") {
        lines.push(format!("  Writer coordination: {note}"));
    }
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}
