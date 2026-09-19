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
        "State diagnose: complete (command status; integrity is reported below)".to_string(),
        format!("  Integrity: {}", integrity_verdict(value)),
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
    push_run_linkage_lines(&mut lines, &value["run_linkage"]);
    if let Some(recommendations) = value["recommendations"].as_array()
        && !recommendations.is_empty()
    {
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
    lines.push("  full report: rerun with --json".into());
    lines.join("\n")
}

/// Summarizes integrity separately from command completion so a successful
/// `state diagnose` never reads as a clean bill of health.
fn integrity_verdict(value: &serde_json::Value) -> String {
    let integrity = &value["integrity"];
    let malformed = value_u64(integrity, "malformed_records")
        .or_else(|| value_u64(&value["totals"], "malformed_records"))
        .unwrap_or(0);
    let torn = value_u64(integrity, "torn_streams")
        .or_else(|| value_u64(&value["totals"], "torn_streams"))
        .unwrap_or(0);
    let scan_errors = value_u64(integrity, "scan_errors").unwrap_or(0);
    let mut problems = Vec::new();
    if malformed > 0 {
        problems.push(format!("{malformed} malformed records"));
    }
    if torn > 0 {
        problems.push(format!("{torn} torn streams"));
    }
    if scan_errors > 0 {
        problems.push(format!("{scan_errors} stream scan errors"));
    }
    let linkage = match value_str(&value["run_linkage"], "verdict") {
        Some("clean") => "run linkage clean".to_string(),
        Some("findings") => format!(
            "{} run linkage findings",
            value_u64(&value["run_linkage"], "finding_count").unwrap_or(0)
        ),
        Some("incomplete") => "run linkage incomplete (no clean verdict)".to_string(),
        _ => "run linkage not checked (rerun with --deep)".to_string(),
    };
    problems.push(linkage);
    problems.join("; ")
}

fn push_run_linkage_lines(lines: &mut Vec<String>, linkage: &serde_json::Value) {
    const MAX_FINDING_LINES: usize = 5;
    match value_str(linkage, "verdict") {
        Some("clean") => {
            let runs = &linkage["runs"];
            lines.push(format!(
                "  Run linkage: clean ({} referenced runs: {} active, {} completed, {} archived)",
                value_u64(linkage, "referenced_runs").unwrap_or(0),
                value_u64(runs, "active").unwrap_or(0),
                value_u64(runs, "completed").unwrap_or(0),
                value_u64(runs, "archived_verified").unwrap_or(0),
            ));
        }
        Some("findings") => {
            let runs = &linkage["runs"];
            let count = value_u64(linkage, "finding_count").unwrap_or(0);
            let suffix = if value_bool(linkage, "complete").unwrap_or(false) {
                ""
            } else {
                "; scan incomplete, more may exist"
            };
            lines.push(format!(
                "  Run linkage: {count} finding(s) ({} missing, {} unverifiable, {} inconsistent, {} recoverable from backup){suffix}",
                value_u64(runs, "missing").unwrap_or(0),
                value_u64(runs, "unverifiable").unwrap_or(0),
                value_u64(runs, "inconsistent").unwrap_or(0),
                value_u64(runs, "recoverable_from_backup").unwrap_or(0),
            ));
            let findings = linkage["findings"].as_array().cloned().unwrap_or_default();
            for finding in findings.iter().take(MAX_FINDING_LINES) {
                lines.push(format!(
                    "    {}: {}; receipts: {}; batch receipts: {}",
                    value_str(finding, "run_id").unwrap_or("<unknown run>"),
                    value_str(finding, "status").unwrap_or("unknown"),
                    id_preview(&finding["receipt_ids"], value_u64(finding, "receipt_count")),
                    id_preview(
                        &finding["batch_receipt_ids"],
                        value_u64(finding, "batch_receipt_count")
                    ),
                ));
            }
            let shown = findings.len().min(MAX_FINDING_LINES) as u64;
            if count > shown {
                lines.push(format!(
                    "    ... {} more finding(s); rerun with --json for structured findings and truncation metadata",
                    count - shown
                ));
            }
        }
        Some("incomplete") => {
            let reasons = linkage["incomplete_reasons"]
                .as_array()
                .map(|reasons| {
                    reasons
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            lines.push(format!(
                "  Run linkage: incomplete; no clean verdict ({reasons})"
            ));
        }
        _ => lines.push("  Run linkage: not checked (rerun with --deep)".into()),
    }
}

fn id_preview(ids: &serde_json::Value, total: Option<u64>) -> String {
    const MAX_IDS: usize = 3;
    let ids = ids
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return "none".into();
    }
    let total = total.unwrap_or(ids.len() as u64);
    let shown_ids = ids.iter().take(MAX_IDS).copied().collect::<Vec<_>>();
    let omitted = total.saturating_sub(shown_ids.len() as u64);
    let shown = shown_ids.join(", ");
    if omitted == 0 {
        shown
    } else {
        format!("{shown} (+{omitted} more)")
    }
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
    let runs_included = value_bool(value, "runs_included").unwrap_or(false);
    let runs_archived = value_u64(value, "runs_archived").unwrap_or(0);
    let runs_retained = value_u64(value, "runs_retained").unwrap_or(0);
    let before = value_str(value, "before").unwrap_or("<unknown>");
    let changes_available = archived > 0 || (runs_included && runs_archived > 0);
    let status = match (dry_run, changes_available) {
        (true, true) => "dry run (changes available)",
        (true, false) => "dry run (no eligible state)",
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
            if !dry_run && changes_available {
                "yes"
            } else {
                "no"
            }
        ),
    ];
    if runs_included {
        lines.push(format!("  Runs archived: {runs_archived}"));
        lines.push(format!("  Runs retained: {runs_retained}"));
    }
    if let Some(protected) = value_u64(value, "protected_receipts_retained") {
        lines.push(format!("  Protected receipts retained: {protected}"));
    }
    if runs_included && let Some(protected) = value_u64(value, "protected_runs_retained") {
        lines.push(format!("  Protected runs retained: {protected}"));
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
    if runs_included {
        match value_str(value, "runs_archive_path") {
            Some(path) => lines.push(format!("  Run archive: {path}")),
            None if dry_run => lines.push("  Run archive: not written during dry run".into()),
            None => lines.push("  Run archive: not written; no runs were eligible".into()),
        }
        match value_str(value, "runs_recovery_backup_path") {
            Some(path) => lines.push(format!("  Exact runs recovery backup: {path}")),
            None if dry_run => {
                lines.push("  Exact runs recovery backup: not written during dry run".into());
            }
            None => lines
                .push("  Exact runs recovery backup: not written; run state was unchanged".into()),
        }
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
