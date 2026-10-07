//! Read-only diagnostics for the repository-local state streams.
//!
//! This module deliberately does not use the normal state-layout or JSONL
//! mutation helpers. Diagnosis must be safe to run before `.agent/state`
//! exists, and a legacy record can be hundreds of megabytes. Each stream is
//! therefore inspected one physical record at a time. The session, plan,
//! decision, and receipt streams are no longer written or read by Jig; they
//! are still sized and checked because adopted repositories keep them.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::de::IgnoredAny;
use serde_json::{Value, json};

use jig_context::RepoContext;

use super::jsonl::scan_jsonl_raw;

const STATE_STREAMS: [(&str, &str); 5] = [
    ("sessions", "sessions.jsonl"),
    ("plans", "plans.jsonl"),
    ("receipts", "receipts.jsonl"),
    ("decisions", "decisions.jsonl"),
    ("runs", "runs.jsonl"),
];
/// Streams Jig no longer writes or reads.
const LEGACY_STREAMS: [&str; 4] = ["sessions", "plans", "decisions", "receipts"];
const OVERSIZED_RECORD_BYTES: u64 = 1024 * 1024;
const RUN_RETENTION_RECOMMENDATION_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_SAMPLES: usize = 20;

pub fn state_diagnose(ctx: &RepoContext) -> Value {
    let mut streams = BTreeMap::new();
    for (stream_name, file_name) in STATE_STREAMS {
        let path = ctx.state_file(file_name);
        streams.insert(stream_name.to_string(), inspect_stream(ctx.root(), &path));
    }

    let legacy_archive = inspect_legacy_archive(
        ctx.root(),
        &ctx.state_dir().join("archive"),
        MAX_DIAGNOSTIC_SAMPLES,
    );
    let maintenance_cache = inspect_maintenance_cache(ctx.root(), MAX_DIAGNOSTIC_SAMPLES);
    let git = inspect_git_facts(ctx.root());
    let totals = state_totals(&streams, &legacy_archive, &maintenance_cache);
    let recommendations = recommendations(&streams, &legacy_archive, &maintenance_cache);
    let integrity = integrity_summary(&streams);

    json!({
        "ok": true,
        "command": "state diagnose",
        "integrity": integrity,
        "state_dir": display_repo_path(ctx.root(), &ctx.state_dir()),
        "state_dir_exists": ctx.state_dir().is_dir(),
        "totals": totals,
        "streams": streams,
        "legacy_archive": legacy_archive,
        "maintenance_cache": maintenance_cache,
        "git": git,
        "recommendations": recommendations,
    })
}

fn inspect_stream(root: &Path, path: &Path) -> StreamDiagnostics {
    let mut report = StreamDiagnostics {
        path: display_repo_path(root, path),
        ..StreamDiagnostics::default()
    };

    let mut visit = |raw: super::jsonl::RawJsonlRecord<'_>| {
        let line_number = raw.line_number;
        let record = raw.bytes;
        report.records += 1;
        if record.len() as u64 > report.max_record_bytes {
            report.max_record_bytes = record.len() as u64;
            report.max_record_line = Some(line_number);
        }
        if record.len() as u64 >= OVERSIZED_RECORD_BYTES {
            report.oversized_records += 1;
            push_sample(
                &mut report.oversized_record_samples,
                RecordSizeSample {
                    line: line_number,
                    bytes: record.len() as u64,
                },
            );
        }

        let parse_result = serde_json::from_slice::<IgnoredAny>(record);
        if let Err(error) = parse_result {
            report.malformed_records += 1;
            push_sample(
                &mut report.malformed_record_samples,
                MalformedRecordSample {
                    line: line_number,
                    error: error.to_string(),
                },
            );
            return Ok(());
        }
        Ok(())
    };
    let result = scan_jsonl_raw(path, &|| false, &mut visit);

    match result {
        Ok(scan) => {
            report.exists = path.is_file();
            report.bytes = scan.file_bytes;
            report.physical_lines = scan.physical_lines;
            report.blank_lines = scan.blank_lines;
            report.max_line_bytes = scan.max_line_bytes;
            report.max_line = scan.max_line_number;
            report.unterminated_final_record = scan.unterminated_final_record;
            report.torn_tail = scan.unterminated_final_record;
        }
        Err(error) => {
            report.exists = path.exists();
            report.bytes = fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            report.scan_error = Some(format!("{error:#}"));
        }
    }
    report.malformed_samples_truncated =
        report.malformed_records as usize > report.malformed_record_samples.len();
    report.oversized_samples_truncated =
        report.oversized_records as usize > report.oversized_record_samples.len();
    report
}

#[derive(Debug, Default, serde::Serialize)]
struct StreamDiagnostics {
    path: String,
    exists: bool,
    bytes: u64,
    physical_lines: u64,
    records: u64,
    blank_lines: u64,
    max_line_bytes: u64,
    max_line: Option<u64>,
    max_record_bytes: u64,
    max_record_line: Option<u64>,
    malformed_records: u64,
    malformed_record_samples: Vec<MalformedRecordSample>,
    malformed_samples_truncated: bool,
    unterminated_final_record: bool,
    torn_tail: bool,
    oversized_records: u64,
    oversized_record_samples: Vec<RecordSizeSample>,
    oversized_samples_truncated: bool,
    scan_error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct MalformedRecordSample {
    line: u64,
    error: String,
}

#[derive(Debug, serde::Serialize)]
struct RecordSizeSample {
    line: u64,
    bytes: u64,
}

#[derive(Debug, Default, serde::Serialize)]
struct LegacyArchiveDiagnostics {
    path: String,
    exists: bool,
    files: u64,
    bytes: u64,
    symlinks_skipped: u64,
    errors: Vec<String>,
    errors_truncated: bool,
}

#[derive(Debug, Default, serde::Serialize)]
struct MaintenanceCacheDiagnostics {
    path: String,
    exists: bool,
    files: u64,
    bytes: u64,
    symlinks_skipped: u64,
    state_backups: LegacyArchiveDiagnostics,
    state_archives: LegacyArchiveDiagnostics,
}

fn inspect_maintenance_cache(root: &Path, max_errors: usize) -> MaintenanceCacheDiagnostics {
    let cache = root.join(".agent/.cache");
    let state_backups = inspect_legacy_archive(root, &cache.join("state-backups"), max_errors);
    let state_archives = inspect_legacy_archive(root, &cache.join("state-archives"), max_errors);
    MaintenanceCacheDiagnostics {
        path: display_repo_path(root, &cache),
        exists: cache.is_dir(),
        files: state_backups.files.saturating_add(state_archives.files),
        bytes: state_backups.bytes.saturating_add(state_archives.bytes),
        symlinks_skipped: state_backups
            .symlinks_skipped
            .saturating_add(state_archives.symlinks_skipped),
        state_backups,
        state_archives,
    }
}

fn inspect_legacy_archive(
    root: &Path,
    archive: &Path,
    max_errors: usize,
) -> LegacyArchiveDiagnostics {
    let mut report = LegacyArchiveDiagnostics {
        path: display_repo_path(root, archive),
        ..LegacyArchiveDiagnostics::default()
    };
    let mut error_count = 0usize;
    let archive_metadata = match fs::symlink_metadata(archive) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return report,
        Err(error) => {
            report
                .errors
                .push(format!("{}: {error}", display_repo_path(root, archive)));
            return report;
        }
    };
    report.exists = true;
    if archive_metadata.file_type().is_symlink() {
        report.symlinks_skipped = 1;
        return report;
    }
    if archive_metadata.is_file() {
        report.files = 1;
        report.bytes = archive_metadata.len();
        return report;
    }
    if !archive_metadata.is_dir() {
        return report;
    }

    let mut pending = vec![archive.to_path_buf()];
    while let Some(path) = pending.pop() {
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) => {
                error_count += 1;
                push_sample(
                    &mut report.errors,
                    format!("{}: {error}", display_repo_path(root, &path)),
                );
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    error_count += 1;
                    push_sample(&mut report.errors, error.to_string());
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    error_count += 1;
                    push_sample(
                        &mut report.errors,
                        format!("{}: {error}", display_repo_path(root, &entry.path())),
                    );
                    continue;
                }
            };
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                match entry.metadata() {
                    Ok(metadata) => {
                        report.files += 1;
                        report.bytes = report.bytes.saturating_add(metadata.len());
                    }
                    Err(error) => {
                        error_count += 1;
                        push_sample(
                            &mut report.errors,
                            format!("{}: {error}", display_repo_path(root, &entry.path())),
                        );
                    }
                }
            } else if file_type.is_symlink() {
                report.symlinks_skipped += 1;
            }
        }
    }
    report.errors_truncated = error_count > max_errors;
    report
}

#[derive(Debug, Default, serde::Serialize)]
struct GitDiagnostics {
    repository: bool,
    error: Option<String>,
    paths: BTreeMap<String, GitPathDiagnostics>,
}

#[derive(Debug, Default, serde::Serialize)]
struct GitPathDiagnostics {
    path: String,
    tracked: Option<bool>,
    ignored: Option<bool>,
    merge_attribute: Option<String>,
    errors: Vec<String>,
}

fn inspect_git_facts(root: &Path) -> GitDiagnostics {
    let mut report = GitDiagnostics::default();
    let probe = run_git(root, ["rev-parse", "--is-inside-work-tree"]);
    match probe {
        Ok(output) if output.status.success() && trim_ascii(&output.stdout) == b"true" => {
            report.repository = true;
        }
        Ok(output) => {
            report.error = Some(git_failure("git repository probe", &output));
        }
        Err(error) => {
            report.error = Some(format!("Failed to run git repository probe: {error}"));
        }
    }

    for (name, file_name) in STATE_STREAMS {
        let relative = PathBuf::from(".agent/state").join(file_name);
        let mut path_report = GitPathDiagnostics {
            path: relative.display().to_string(),
            ..GitPathDiagnostics::default()
        };
        if report.repository {
            inspect_git_path(root, &relative, &mut path_report);
        }
        report.paths.insert(name.to_string(), path_report);
    }
    report
}

fn inspect_git_path(root: &Path, relative: &Path, report: &mut GitPathDiagnostics) {
    match run_git_path(root, ["ls-files", "--error-unmatch", "--"], relative) {
        Ok(output) if output.status.success() => report.tracked = Some(true),
        Ok(output) if output.status.code() == Some(1) => report.tracked = Some(false),
        Ok(output) => report.errors.push(git_failure("git ls-files", &output)),
        Err(error) => report
            .errors
            .push(format!("Failed to run git ls-files: {error}")),
    }

    match run_git_path(root, ["check-ignore", "--no-index", "-q", "--"], relative) {
        Ok(output) if output.status.success() => report.ignored = Some(true),
        Ok(output) if output.status.code() == Some(1) => report.ignored = Some(false),
        Ok(output) => report.errors.push(git_failure("git check-ignore", &output)),
        Err(error) => report
            .errors
            .push(format!("Failed to run git check-ignore: {error}")),
    }

    match run_git_path(root, ["check-attr", "-z", "merge", "--"], relative) {
        Ok(output) if output.status.success() => {
            let fields = output
                .stdout
                .split(|byte| *byte == b'\0')
                .filter(|field| !field.is_empty())
                .collect::<Vec<_>>();
            if fields.len() == 3 && fields[1] == b"merge" {
                report.merge_attribute = Some(String::from_utf8_lossy(fields[2]).into_owned());
            } else {
                report
                    .errors
                    .push("git check-attr returned an unexpected response".into());
            }
        }
        Ok(output) => report.errors.push(git_failure("git check-attr", &output)),
        Err(error) => report
            .errors
            .push(format!("Failed to run git check-attr: {error}")),
    }
}

fn run_git<'a>(
    root: &Path,
    args: impl IntoIterator<Item = &'a str>,
) -> io::Result<std::process::Output> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}

fn run_git_path<'a>(
    root: &Path,
    args: impl IntoIterator<Item = &'a str>,
    path: &Path,
) -> io::Result<std::process::Output> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(args)
        .arg(path)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}

fn git_failure(label: &str, output: &std::process::Output) -> String {
    format!(
        "{label} failed with status {}; stderr: {}",
        output
            .status
            .code()
            .map_or_else(|| "signal".into(), |code| code.to_string()),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

fn state_totals(
    streams: &BTreeMap<String, StreamDiagnostics>,
    legacy_archive: &LegacyArchiveDiagnostics,
    maintenance_cache: &MaintenanceCacheDiagnostics,
) -> Value {
    let stream_bytes = streams.values().map(|stream| stream.bytes).sum::<u64>();
    let checkout_state_bytes = stream_bytes.saturating_add(legacy_archive.bytes);
    let local_disk_bytes = checkout_state_bytes.saturating_add(maintenance_cache.bytes);
    json!({
        "bytes": checkout_state_bytes,
        "stream_bytes": stream_bytes,
        "stream_records": streams.values().map(|stream| stream.records).sum::<u64>(),
        "malformed_records": streams
            .values()
            .map(|stream| stream.malformed_records)
            .sum::<u64>(),
        "torn_streams": streams.values().filter(|stream| stream.torn_tail).count(),
        "legacy_archive_bytes": legacy_archive.bytes,
        "checkout_state_bytes": checkout_state_bytes,
        "maintenance_cache_bytes": maintenance_cache.bytes,
        "local_disk_bytes": local_disk_bytes,
    })
}

fn recommendations(
    streams: &BTreeMap<String, StreamDiagnostics>,
    legacy_archive: &LegacyArchiveDiagnostics,
    maintenance_cache: &MaintenanceCacheDiagnostics,
) -> Vec<Value> {
    let mut recommendations = Vec::new();
    if streams
        .values()
        .any(|stream| stream.malformed_records > 0 || stream.torn_tail)
    {
        recommendations.push(json!({
            "kind": "repair_malformed_state",
            "command": null,
            "reason": "Back up and repair malformed or unterminated state before mutation.",
        }));
    }
    let run_stream_bytes = streams.get("runs").map_or(0, |stream| stream.bytes);
    if run_stream_bytes >= RUN_RETENTION_RECOMMENDATION_BYTES {
        recommendations.push(json!({
            "kind": "archive_runs",
            "command": "jig state archive --before <YYYY-MM-DD> --dry-run",
            "reason": format!(
                "Run state uses {run_stream_bytes} bytes; preview archiving completed run histories after all known runs become terminal."
            ),
        }));
    }
    let legacy_streams = LEGACY_STREAMS
        .iter()
        .filter_map(|name| streams.get(*name).filter(|stream| stream.bytes > 0))
        .collect::<Vec<_>>();
    if !legacy_streams.is_empty() {
        recommendations.push(json!({
            "kind": "legacy_state_streams",
            "command": null,
            "reason": format!(
                "{} bytes remain in state streams Jig no longer writes or reads ({}); keep them as history or remove them.",
                legacy_streams.iter().map(|stream| stream.bytes).sum::<u64>(),
                legacy_streams
                    .iter()
                    .map(|stream| stream.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        }));
    }
    if legacy_archive.bytes > 0 {
        recommendations.push(json!({
            "kind": "legacy_archive",
            "command": null,
            "reason": format!(
                "{} bytes remain under the legacy .agent/state/archive directory",
                legacy_archive.bytes,
            ),
        }));
    }
    if maintenance_cache.bytes > 0 {
        recommendations.push(json!({
            "kind": "review_maintenance_cache",
            "command": null,
            "reason": format!(
                "{} bytes are retained in ignored state backups and archives; keep the latest rollback artifact until verification, copy durable artifacts elsewhere, and remove obsolete cache entries.",
                maintenance_cache.bytes,
            ),
        }));
    }
    recommendations
}

/// `ok` reports that diagnosis ran. Integrity findings live here and in
/// `recommendations`, so a successful command never implies healthy state.
fn integrity_summary(streams: &BTreeMap<String, StreamDiagnostics>) -> Value {
    json!({
        "note": "`ok` reports command completion, not state integrity.",
        "malformed_records": streams
            .values()
            .map(|stream| stream.malformed_records)
            .sum::<u64>(),
        "torn_streams": streams.values().filter(|stream| stream.torn_tail).count(),
        "scan_errors": streams
            .values()
            .filter(|stream| stream.scan_error.is_some())
            .count(),
    })
}

fn display_repo_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn push_sample<T>(samples: &mut Vec<T>, sample: T) {
    if samples.len() < MAX_DIAGNOSTIC_SAMPLES {
        samples.push(sample);
    }
}

#[cfg(test)]
mod tests;
