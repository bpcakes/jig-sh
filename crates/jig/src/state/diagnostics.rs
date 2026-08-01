//! Read-only diagnostics for the repository-local state streams.
//!
//! This module deliberately does not use the normal state-layout or JSONL
//! mutation helpers. Diagnosis must be safe to run before `.agent/state`
//! exists, and a legacy recursive session record can be hundreds of megabytes.
//! Each stream is therefore inspected one physical record at a time.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::command::StateDiagnoseRequest;
use crate::context::RepoContext;

use super::jsonl::scan_jsonl_raw;

const STATE_STREAMS: [(&str, &str); 4] = [
    ("sessions", "sessions.jsonl"),
    ("plans", "plans.jsonl"),
    ("receipts", "receipts.jsonl"),
    ("decisions", "decisions.jsonl"),
];
const OVERSIZED_RECORD_BYTES: u64 = 1024 * 1024;
const RECEIPT_RETENTION_RECOMMENDATION_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_SAMPLES: usize = 20;

pub(crate) fn state_diagnose(ctx: &RepoContext, request: StateDiagnoseRequest) -> Result<Value> {
    let mut streams = BTreeMap::new();
    let mut session_compaction = SessionCompactionDiagnostics::default();
    let mut receipt_payload = ReceiptPayloadDiagnostics::default();

    for (stream_name, file_name) in STATE_STREAMS {
        let path = ctx.state_file(file_name);
        let report = inspect_stream(
            ctx.root(),
            &path,
            request.deep.then_some(stream_name),
            &mut session_compaction,
            &mut receipt_payload,
        );
        streams.insert(stream_name.to_string(), report);
    }

    let legacy_archive = inspect_legacy_archive(
        ctx.root(),
        &ctx.state_dir().join("archive"),
        MAX_DIAGNOSTIC_SAMPLES,
    );
    let maintenance_cache = inspect_maintenance_cache(ctx.root(), MAX_DIAGNOSTIC_SAMPLES);
    let git = inspect_git_facts(ctx.root());
    let totals = state_totals(&streams, &legacy_archive, &maintenance_cache);
    let recommendations = recommendations(
        request.deep,
        &streams,
        &session_compaction,
        &receipt_payload,
        &legacy_archive,
        &maintenance_cache,
    );

    Ok(json!({
        "ok": true,
        "command": "state diagnose",
        "deep": request.deep,
        "state_dir": display_repo_path(ctx.root(), &ctx.state_dir()),
        "state_dir_exists": ctx.state_dir().is_dir(),
        "totals": totals,
        "streams": streams,
        "sessions": request.deep.then_some(session_compaction),
        "receipts": request.deep.then_some(receipt_payload),
        "legacy_archive": legacy_archive,
        "maintenance_cache": maintenance_cache,
        "git": git,
        "recommendations": recommendations,
    }))
}

fn inspect_stream(
    root: &Path,
    path: &Path,
    deep_stream: Option<&str>,
    session_compaction: &mut SessionCompactionDiagnostics,
    receipt_payload: &mut ReceiptPayloadDiagnostics,
) -> StreamDiagnostics {
    let mut report = StreamDiagnostics {
        path: display_repo_path(root, path),
        ..StreamDiagnostics::default()
    };
    let mut deep_analysis_error_lines = Vec::new();
    let mut deep_analysis_error_count = 0u64;

    let result = scan_jsonl_raw(path, &|| false, |raw| {
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

        let deep_result: Result<()> = match deep_stream {
            Some("sessions") => {
                let projection = analyze_session_record(record)?;
                session_compaction.analyzed_records += 1;
                session_compaction.recursive_summary_values = session_compaction
                    .recursive_summary_values
                    .saturating_add(projection.recursive_summary_values);
                session_compaction.estimated_reclaimable_bytes = session_compaction
                    .estimated_reclaimable_bytes
                    .saturating_add(projection.reclaimable_bytes);
                session_compaction.growth_bytes = session_compaction
                    .growth_bytes
                    .saturating_add(projection.growth_bytes);
                if projection.recursive_summary_values > 0 {
                    session_compaction.recursive_session_records += 1;
                }
                Ok(())
            }
            Some("receipts") => {
                analyze_receipt_record(record, receipt_payload)?;
                receipt_payload.analyzed_records += 1;
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = deep_result {
            deep_analysis_error_count += 1;
            push_sample(
                &mut deep_analysis_error_lines,
                MalformedRecordSample {
                    line: line_number,
                    error: format!("{error:#}"),
                },
            );
        }
        Ok(())
    });

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

            if deep_stream == Some("sessions") {
                // Blank and malformed records are not transformed, so start
                // with all source bytes and subtract only the exact value-span
                // savings discovered in valid session records.
                session_compaction.source_bytes = scan.file_bytes;
                session_compaction.projected_shallow_bytes = scan
                    .file_bytes
                    .saturating_sub(session_compaction.estimated_reclaimable_bytes)
                    .saturating_add(session_compaction.growth_bytes);
            }
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
    report.deep_analysis_errors = deep_analysis_error_lines;
    report.deep_analysis_error_count = deep_analysis_error_count;
    report.deep_analysis_errors_truncated =
        deep_analysis_error_count as usize > report.deep_analysis_errors.len();
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
    deep_analysis_error_count: u64,
    deep_analysis_errors: Vec<MalformedRecordSample>,
    deep_analysis_errors_truncated: bool,
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
struct SessionCompactionDiagnostics {
    source_bytes: u64,
    analyzed_records: u64,
    recursive_session_records: u64,
    recursive_summary_values: u64,
    projected_shallow_bytes: u64,
    estimated_reclaimable_bytes: u64,
    growth_bytes: u64,
}

#[derive(Debug, Default)]
struct SessionRecordProjection {
    recursive_summary_values: u64,
    projected_record_bytes: u64,
    reclaimable_bytes: u64,
    growth_bytes: u64,
}

fn analyze_session_record(record: &[u8]) -> Result<SessionRecordProjection> {
    let mut projection = SessionRecordProjection {
        projected_record_bytes: record.len() as u64,
        ..SessionRecordProjection::default()
    };
    visit_object_members(record, 0..record.len(), &mut |key, value| {
        if key != "summary" || first_non_whitespace(record, value.clone()) != Some(b'{') {
            return Ok(());
        }
        visit_object_members(record, value, &mut |key, value| {
            if key != "recent_sessions" || first_non_whitespace(record, value.clone()) != Some(b'[')
            {
                return Ok(());
            }
            visit_array_values(record, value, &mut |reference| {
                if first_non_whitespace(record, reference.clone()) != Some(b'{') {
                    return Ok(());
                }
                visit_object_members(record, reference, &mut |key, nested_summary| {
                    if key == "summary" && record[nested_summary.clone()] != *b"null" {
                        projection.recursive_summary_values += 1;
                        let original_bytes = nested_summary.len() as u64;
                        projection.projected_record_bytes = projection
                            .projected_record_bytes
                            .saturating_sub(original_bytes)
                            .saturating_add(4);
                        if original_bytes > 4 {
                            projection.reclaimable_bytes += original_bytes - 4;
                        } else {
                            projection.growth_bytes += 4 - original_bytes;
                        }
                    }
                    Ok(())
                })
            })
        })
    })?;
    Ok(projection)
}

#[derive(Debug, Default, serde::Serialize)]
struct ReceiptPayloadDiagnostics {
    analyzed_records: u64,
    args_bytes: u64,
    stdout_preview_bytes: u64,
    stderr_preview_bytes: u64,
    output_preview_bytes: u64,
    evidence_bytes: u64,
    changed_paths_bytes: u64,
    diff_stat_bytes: u64,
    other_top_level_value_bytes: u64,
    total_top_level_value_bytes: u64,
}

fn analyze_receipt_record(
    record: &[u8],
    diagnostics: &mut ReceiptPayloadDiagnostics,
) -> Result<()> {
    visit_object_members(record, 0..record.len(), &mut |key, value| {
        let bytes = value.len() as u64;
        diagnostics.total_top_level_value_bytes = diagnostics
            .total_top_level_value_bytes
            .saturating_add(bytes);
        match key {
            "args" => diagnostics.args_bytes = diagnostics.args_bytes.saturating_add(bytes),
            "stdout_preview" => {
                diagnostics.stdout_preview_bytes =
                    diagnostics.stdout_preview_bytes.saturating_add(bytes);
                diagnostics.output_preview_bytes =
                    diagnostics.output_preview_bytes.saturating_add(bytes);
            }
            "stderr_preview" => {
                diagnostics.stderr_preview_bytes =
                    diagnostics.stderr_preview_bytes.saturating_add(bytes);
                diagnostics.output_preview_bytes =
                    diagnostics.output_preview_bytes.saturating_add(bytes);
            }
            "evidence" => {
                diagnostics.evidence_bytes = diagnostics.evidence_bytes.saturating_add(bytes);
            }
            "changed_paths" => {
                diagnostics.changed_paths_bytes =
                    diagnostics.changed_paths_bytes.saturating_add(bytes);
            }
            "diff_stat" => {
                diagnostics.diff_stat_bytes = diagnostics.diff_stat_bytes.saturating_add(bytes);
            }
            _ => {
                diagnostics.other_top_level_value_bytes = diagnostics
                    .other_top_level_value_bytes
                    .saturating_add(bytes);
            }
        }
        Ok(())
    })
}

fn visit_object_members(
    input: &[u8],
    range: Range<usize>,
    visitor: &mut impl FnMut(&str, Range<usize>) -> Result<()>,
) -> Result<()> {
    let mut cursor = skip_whitespace(input, range.start, range.end);
    if input.get(cursor) != Some(&b'{') {
        return Ok(());
    }
    cursor += 1;
    loop {
        cursor = skip_whitespace(input, cursor, range.end);
        match input.get(cursor) {
            Some(b'}') => return Ok(()),
            Some(b'"') => {}
            _ => bail!("Expected object key at byte {cursor}"),
        }
        let key_start = cursor;
        cursor = skip_json_string(input, cursor, range.end)?;
        let key: String = serde_json::from_slice(&input[key_start..cursor])
            .context("Failed to decode JSON object key")?;
        cursor = skip_whitespace(input, cursor, range.end);
        if input.get(cursor) != Some(&b':') {
            bail!("Expected ':' after object key at byte {cursor}");
        }
        cursor = skip_whitespace(input, cursor + 1, range.end);
        let value_start = cursor;
        cursor = skip_json_value(input, cursor, range.end)?;
        visitor(&key, value_start..cursor)?;
        cursor = skip_whitespace(input, cursor, range.end);
        match input.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b'}') => return Ok(()),
            _ => bail!("Expected ',' or '}}' at byte {cursor}"),
        }
    }
}

fn visit_array_values(
    input: &[u8],
    range: Range<usize>,
    visitor: &mut impl FnMut(Range<usize>) -> Result<()>,
) -> Result<()> {
    let mut cursor = skip_whitespace(input, range.start, range.end);
    if input.get(cursor) != Some(&b'[') {
        return Ok(());
    }
    cursor += 1;
    loop {
        cursor = skip_whitespace(input, cursor, range.end);
        if input.get(cursor) == Some(&b']') {
            return Ok(());
        }
        let value_start = cursor;
        cursor = skip_json_value(input, cursor, range.end)?;
        visitor(value_start..cursor)?;
        cursor = skip_whitespace(input, cursor, range.end);
        match input.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b']') => return Ok(()),
            _ => bail!("Expected ',' or ']' at byte {cursor}"),
        }
    }
}

fn skip_json_value(input: &[u8], start: usize, end: usize) -> Result<usize> {
    let start = skip_whitespace(input, start, end);
    match input.get(start).copied() {
        Some(b'"') => skip_json_string(input, start, end),
        Some(b'{') | Some(b'[') => skip_json_composite(input, start, end),
        Some(_) => {
            let mut cursor = start;
            while cursor < end
                && !input[cursor].is_ascii_whitespace()
                && !matches!(input[cursor], b',' | b']' | b'}')
            {
                cursor += 1;
            }
            if cursor == start {
                bail!("Expected JSON value at byte {start}");
            }
            Ok(cursor)
        }
        None => bail!("Expected JSON value at byte {start}"),
    }
}

fn skip_json_composite(input: &[u8], start: usize, end: usize) -> Result<usize> {
    let mut cursor = start;
    let mut depth = 0usize;
    while cursor < end {
        match input[cursor] {
            b'"' => cursor = skip_json_string(input, cursor, end)?,
            b'{' | b'[' => {
                depth += 1;
                cursor += 1;
            }
            b'}' | b']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| anyhow!("Unexpected JSON delimiter at byte {cursor}"))?;
                cursor += 1;
                if depth == 0 {
                    return Ok(cursor);
                }
            }
            _ => cursor += 1,
        }
    }
    bail!("Unterminated JSON value at byte {start}")
}

fn skip_json_string(input: &[u8], start: usize, end: usize) -> Result<usize> {
    let mut cursor = start + 1;
    while cursor < end {
        match input[cursor] {
            b'\\' => {
                cursor = cursor.saturating_add(2);
            }
            b'"' => return Ok(cursor + 1),
            _ => cursor += 1,
        }
    }
    bail!("Unterminated JSON string at byte {start}")
}

fn skip_whitespace(input: &[u8], mut cursor: usize, end: usize) -> usize {
    while cursor < end && input[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn first_non_whitespace(input: &[u8], range: Range<usize>) -> Option<u8> {
    let cursor = skip_whitespace(input, range.start, range.end);
    input.get(cursor).copied()
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
    deep: bool,
    streams: &BTreeMap<String, StreamDiagnostics>,
    sessions: &SessionCompactionDiagnostics,
    receipts: &ReceiptPayloadDiagnostics,
    legacy_archive: &LegacyArchiveDiagnostics,
    maintenance_cache: &MaintenanceCacheDiagnostics,
) -> Vec<Value> {
    let mut recommendations = Vec::new();
    if deep && sessions.recursive_session_records > 0 {
        recommendations.push(json!({
            "kind": "compact_sessions",
            "command": "jig state compact sessions --dry-run",
            "reason": format!(
                "{} session records contain nested summaries; projected recovery is {} bytes",
                sessions.recursive_session_records,
                sessions.estimated_reclaimable_bytes,
            ),
        }));
    }
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
    let receipt_stream = streams.get("receipts");
    let receipt_payload_bytes = receipts.total_top_level_value_bytes;
    let receipt_stream_bytes = receipt_stream.map_or(0, |stream| stream.bytes);
    if deep
        && receipt_payload_bytes.max(receipt_stream_bytes) >= RECEIPT_RETENTION_RECOMMENDATION_BYTES
    {
        recommendations.push(json!({
            "kind": "archive_receipts",
            "command": "jig state archive --before <YYYY-MM-DD> --dry-run",
            "alternative_command": "jig state export receipts --before <YYYY-MM-DD> --output receipts.jsonl.gz",
            "reason": format!(
                "Receipt state uses {} bytes ({} bytes of analyzed top-level payloads); preview a local compressed archive or create a non-mutating export.",
                receipt_stream_bytes,
                receipt_payload_bytes,
            ),
        }));
    }
    if deep && receipt_stream.is_some_and(|stream| stream.deep_analysis_error_count > 0) {
        let error_count = receipt_stream.map_or(0, |stream| stream.deep_analysis_error_count);
        recommendations.push(json!({
            "kind": "export_receipts_before_repair",
            "command": "jig state export receipts --before <YYYY-MM-DD> --output receipts-before-repair.jsonl.gz",
            "alternative_command": "jig state archive --before <YYYY-MM-DD> --dry-run",
            "reason": format!(
                "Deep receipt analysis failed for {error_count} records; preserve a non-mutating compressed export and resolve the reported records before archiving."
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
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;
    use crate::test_env::TestRepoBuilder;

    fn fixture_context(root: &Path) -> RepoContext {
        TestRepoBuilder::new(root).write();
        RepoContext::load_from_root(root.to_path_buf()).unwrap()
    }

    #[test]
    fn diagnose_missing_state_is_strictly_read_only() {
        let temp = tempdir().unwrap();
        let ctx = fixture_context(temp.path());
        let before = fixture_paths(temp.path());

        let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: true }).unwrap();

        assert_eq!(output["state_dir_exists"], false);
        assert_eq!(output["totals"]["stream_bytes"], 0);
        assert_eq!(output["sessions"]["projected_shallow_bytes"], 0);
        assert_eq!(before, fixture_paths(temp.path()));
        assert!(!ctx.state_dir().exists());
        assert!(!temp.path().join(".git").exists());
    }

    #[test]
    fn diagnose_reports_exact_stream_and_deep_storage_facts() {
        let temp = tempdir().unwrap();
        let ctx = fixture_context(temp.path());
        fs::create_dir_all(ctx.state_dir().join("archive/nested")).unwrap();

        let nested_summary = r#"{"recent_sessions":[{"summary":"braces } ] in a string"}]}"#;
        let recursive = [
            r#"{"id":"outer","session_id":"s2","event":"start","timestamp_ms":2,"summary":{"recent_sessions":[{"id":"inner","session_id":"s1","event":"start","timestamp_ms":1,"summary":"#,
            nested_summary,
            r#"}]}}"#,
        ]
        .concat();
        let ordinary = r#"{"id":"end","session_id":"s1","event":"end","timestamp_ms":3}"#;
        let sessions = format!("{recursive}\n{ordinary}\n");
        fs::write(ctx.state_file("sessions.jsonl"), sessions.as_bytes()).unwrap();

        let malformed_plans = b"{}\n{\"broken\":";
        fs::write(ctx.state_file("plans.jsonl"), malformed_plans).unwrap();

        let args = r#"{"command":"x"}"#;
        let stdout = r#""out""#;
        let stderr = r#""err""#;
        let evidence = r#"{"ok":true}"#;
        let paths = r#"["a","b"]"#;
        let diff = r#"{"files":2,"insertions":1,"deletions":0}"#;
        let receipt = [
            r#"{"id":"r","args":"#,
            args,
            r#","stdout_preview":"#,
            stdout,
            r#","stderr_preview":"#,
            stderr,
            r#","evidence":"#,
            evidence,
            r#","changed_paths":"#,
            paths,
            r#","diff_stat":"#,
            diff,
            "}",
        ]
        .concat();
        fs::write(
            ctx.state_file("receipts.jsonl"),
            format!("{receipt}\n").as_bytes(),
        )
        .unwrap();
        fs::write(ctx.state_dir().join("archive/old.jsonl"), b"abc").unwrap();
        fs::write(ctx.state_dir().join("archive/nested/older.jsonl"), b"12345").unwrap();

        let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: true }).unwrap();

        assert_eq!(
            output["streams"]["sessions"]["bytes"],
            sessions.len() as u64
        );
        assert_eq!(output["streams"]["sessions"]["records"], 2);
        assert_eq!(
            output["streams"]["sessions"]["max_line_bytes"],
            recursive.len() as u64 + 1
        );
        assert_eq!(
            output["streams"]["sessions"]["max_record_bytes"],
            recursive.len() as u64
        );
        assert_eq!(output["streams"]["sessions"]["max_record_line"], 1);
        assert_eq!(output["streams"]["plans"]["records"], 2);
        assert_eq!(output["streams"]["plans"]["malformed_records"], 1);
        assert_eq!(
            output["streams"]["plans"]["malformed_record_samples"][0]["line"],
            2
        );
        assert_eq!(output["streams"]["plans"]["torn_tail"], true);

        let compacted_recursive = recursive.replace(
            &format!(r#""summary":{nested_summary}"#),
            r#""summary":null"#,
        );
        let expected_projection = compacted_recursive.len() + 1 + ordinary.len() + 1;
        assert_eq!(output["sessions"]["recursive_session_records"], 1);
        assert_eq!(output["sessions"]["recursive_summary_values"], 1);
        assert_eq!(
            output["sessions"]["projected_shallow_bytes"],
            expected_projection as u64
        );
        assert_eq!(
            output["sessions"]["estimated_reclaimable_bytes"],
            (sessions.len() - expected_projection) as u64
        );

        assert_eq!(output["receipts"]["analyzed_records"], 1);
        assert_eq!(output["receipts"]["args_bytes"], args.len() as u64);
        assert_eq!(
            output["receipts"]["stdout_preview_bytes"],
            stdout.len() as u64
        );
        assert_eq!(
            output["receipts"]["stderr_preview_bytes"],
            stderr.len() as u64
        );
        assert_eq!(
            output["receipts"]["output_preview_bytes"],
            (stdout.len() + stderr.len()) as u64
        );
        assert_eq!(output["receipts"]["evidence_bytes"], evidence.len() as u64);
        assert_eq!(
            output["receipts"]["changed_paths_bytes"],
            paths.len() as u64
        );
        assert_eq!(output["receipts"]["diff_stat_bytes"], diff.len() as u64);
        assert_eq!(output["legacy_archive"]["files"], 2);
        assert_eq!(output["legacy_archive"]["bytes"], 8);
        assert_eq!(output["totals"]["legacy_archive_bytes"], 8);
        assert_eq!(
            output["recommendations"][0]["command"],
            "jig state compact sessions --dry-run"
        );
    }

    #[test]
    fn diagnose_reports_tracking_ignore_and_union_merge_facts() {
        let temp = tempdir().unwrap();
        let ctx = fixture_context(temp.path());
        fs::create_dir_all(ctx.state_dir()).unwrap();
        fs::write(ctx.state_file("sessions.jsonl"), b"{}\n").unwrap();
        fs::write(
            temp.path().join(".gitattributes"),
            b".agent/state/*.jsonl merge=union\n",
        )
        .unwrap();
        fs::write(
            temp.path().join(".gitignore"),
            b".agent/state/decisions.jsonl\n",
        )
        .unwrap();
        git(temp.path(), &["init", "-q"]);
        git(
            temp.path(),
            &["add", ".gitattributes", ".agent/state/sessions.jsonl"],
        );

        let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: false }).unwrap();

        assert_eq!(output["git"]["repository"], true);
        assert_eq!(output["git"]["paths"]["sessions"]["tracked"], true);
        assert_eq!(output["git"]["paths"]["sessions"]["ignored"], false);
        assert_eq!(
            output["git"]["paths"]["sessions"]["merge_attribute"],
            "union"
        );
        assert_eq!(output["git"]["paths"]["decisions"]["tracked"], false);
        assert_eq!(output["git"]["paths"]["decisions"]["ignored"], true);
        assert_eq!(
            output["git"]["paths"]["decisions"]["merge_attribute"],
            "union"
        );
    }

    #[test]
    fn diagnose_includes_local_maintenance_cache_usage() {
        let temp = tempdir().unwrap();
        let ctx = fixture_context(temp.path());
        let backups = temp.path().join(".agent/.cache/state-backups/session-1");
        let archives = temp.path().join(".agent/.cache/state-archives");
        fs::create_dir_all(&backups).unwrap();
        fs::create_dir_all(&archives).unwrap();
        fs::write(backups.join("sessions.jsonl.gz"), b"backup").unwrap();
        fs::write(backups.join("manifest.json"), b"manifest").unwrap();
        fs::write(archives.join("receipts.jsonl.gz"), b"archive").unwrap();

        let output = state_diagnose(&ctx, StateDiagnoseRequest { deep: false }).unwrap();

        assert_eq!(output["maintenance_cache"]["exists"], true);
        assert_eq!(output["maintenance_cache"]["files"], 3);
        assert_eq!(output["maintenance_cache"]["bytes"], 21);
        assert_eq!(output["maintenance_cache"]["state_backups"]["bytes"], 14);
        assert_eq!(output["maintenance_cache"]["state_archives"]["bytes"], 7);
        assert_eq!(output["totals"]["maintenance_cache_bytes"], 21);
        assert_eq!(
            output["totals"]["local_disk_bytes"],
            output["totals"]["checkout_state_bytes"].as_u64().unwrap() + 21
        );
        assert!(output["recommendations"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["kind"] == "review_maintenance_cache")
        }));
    }

    #[test]
    fn diagnose_recommends_receipt_retention_and_export_before_repair() {
        let mut streams = BTreeMap::new();
        streams.insert(
            "receipts".into(),
            StreamDiagnostics {
                bytes: RECEIPT_RETENTION_RECOMMENDATION_BYTES,
                deep_analysis_error_count: 2,
                ..StreamDiagnostics::default()
            },
        );
        let receipts = ReceiptPayloadDiagnostics {
            total_top_level_value_bytes: RECEIPT_RETENTION_RECOMMENDATION_BYTES,
            ..ReceiptPayloadDiagnostics::default()
        };

        let recommendations = recommendations(
            true,
            &streams,
            &SessionCompactionDiagnostics::default(),
            &receipts,
            &LegacyArchiveDiagnostics::default(),
            &MaintenanceCacheDiagnostics::default(),
        );

        assert!(recommendations.iter().any(|recommendation| {
            recommendation["kind"] == "archive_receipts"
                && recommendation["command"]
                    .as_str()
                    .is_some_and(|command| command.contains("state archive"))
                && recommendation["alternative_command"]
                    .as_str()
                    .is_some_and(|command| command.contains("state export receipts"))
        }));
        assert!(recommendations.iter().any(|recommendation| {
            recommendation["kind"] == "export_receipts_before_repair"
                && recommendation["command"]
                    .as_str()
                    .is_some_and(|command| command.contains("state export receipts"))
        }));
    }

    #[test]
    fn raw_session_analysis_handles_escaped_keys_and_shallow_nulls() {
        let record = br#"{
            "summ\u0061ry": {
                "recent_sessions": [
                    {"summary": null},
                    {"summary": {"value": "escaped \" } ]"}}
                ]
            }
        }"#;
        serde_json::from_slice::<IgnoredAny>(record).unwrap();

        let projection = analyze_session_record(record).unwrap();

        assert_eq!(projection.recursive_summary_values, 1);
        assert_eq!(
            projection.projected_record_bytes,
            record.len() as u64 - br#"{"value": "escaped \" } ]"}"#.len() as u64 + 4
        );
    }

    fn fixture_paths(root: &Path) -> BTreeSet<PathBuf> {
        let mut paths = BTreeSet::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                paths.insert(path.strip_prefix(root).unwrap().to_path_buf());
                if entry.file_type().unwrap().is_dir() {
                    pending.push(path);
                }
            }
        }
        paths
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }
}
