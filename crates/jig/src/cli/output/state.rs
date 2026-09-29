use super::{value_bool, value_str, value_u64};

pub(super) fn format_state_summary(value: &serde_json::Value) -> String {
    let counts = &value["counts"];
    let repo = &value["repo"];
    let repo_name = value_str(repo, "name").unwrap_or("<unknown>");
    let runs = value_u64(counts, "runs").unwrap_or(0);
    let target_results = value_u64(counts, "target_results").unwrap_or(0);
    let failed = value_u64(counts, "failed_target_results").unwrap_or(0);

    [
        "State summary:".into(),
        format!("  Runs: {runs}"),
        format!("  Target results: {target_results} ({failed} failed)"),
        format!("Repo: {repo_name}"),
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
        lines.push(format!("    State archives: {archive_bytes}"));
    }
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
    if problems.is_empty() {
        return "no malformed, torn, or unreadable streams".into();
    }
    problems.join("; ")
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

pub(super) fn format_state_archive_summary(value: &serde_json::Value) -> String {
    let dry_run = value_bool(value, "dry_run").unwrap_or(false);
    let runs_archived = value_u64(value, "runs_archived").unwrap_or(0);
    let runs_retained = value_u64(value, "runs_retained").unwrap_or(0);
    let before = value_str(value, "before").unwrap_or("<unknown>");
    let changes_available = runs_archived > 0;
    let status = match (dry_run, changes_available) {
        (true, true) => "dry run (changes available)",
        (true, false) => "dry run (no eligible runs)",
        (false, true) => "archived",
        (false, false) => "no-op",
    };
    let mut lines = vec![
        format!("State archive: {status}"),
        format!("  Before: {before}"),
        format!("  Runs archived: {runs_archived}"),
        format!("  Runs retained: {runs_retained}"),
        format!(
            "  Active state changed: {}",
            if !dry_run && changes_available {
                "yes"
            } else {
                "no"
            }
        ),
    ];
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
        None => {
            lines.push("  Exact runs recovery backup: not written; run state was unchanged".into())
        }
    }
    if let Some(bytes) = value_u64(value, "runs_uncompressed_bytes") {
        lines.push(format!("  Uncompressed bytes: {bytes}"));
    }
    if let Some(bytes) = value_u64(value, "runs_compressed_bytes") {
        lines.push(format!("  Compressed bytes: {bytes}"));
    }
    if let Some(checksum) = value_str(value, "runs_content_sha256") {
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
