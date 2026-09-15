use std::ops::Range;

use anyhow::{Context, Result, bail};

use super::super::json_scan::{skip_json_string, skip_json_value, skip_whitespace};

#[derive(Debug, Default, serde::Serialize)]
pub(super) struct SessionCompactionDiagnostics {
    pub(super) source_bytes: u64,
    pub(super) analyzed_records: u64,
    pub(super) recursive_session_records: u64,
    pub(super) recursive_summary_values: u64,
    pub(super) projected_shallow_bytes: u64,
    pub(super) estimated_reclaimable_bytes: u64,
    pub(super) growth_bytes: u64,
}

#[derive(Debug, Default)]
pub(super) struct SessionRecordProjection {
    pub(super) recursive_summary_values: u64,
    pub(super) projected_record_bytes: u64,
    pub(super) reclaimable_bytes: u64,
    pub(super) growth_bytes: u64,
}

pub(super) fn analyze_session_record(record: &[u8]) -> Result<SessionRecordProjection> {
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
pub(super) struct ReceiptPayloadDiagnostics {
    pub(super) analyzed_records: u64,
    pub(super) args_bytes: u64,
    pub(super) stdout_preview_bytes: u64,
    pub(super) stderr_preview_bytes: u64,
    pub(super) output_preview_bytes: u64,
    pub(super) evidence_bytes: u64,
    pub(super) changed_paths_bytes: u64,
    pub(super) diff_stat_bytes: u64,
    pub(super) other_top_level_value_bytes: u64,
    pub(super) total_top_level_value_bytes: u64,
}

pub(super) fn analyze_receipt_record(
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

fn first_non_whitespace(input: &[u8], range: Range<usize>) -> Option<u8> {
    let cursor = skip_whitespace(input, range.start, range.end);
    input.get(cursor).copied()
}
