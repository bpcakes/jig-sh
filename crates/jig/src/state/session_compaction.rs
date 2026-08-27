//! Bounded-memory validation and compaction of legacy recursive session summaries.
//!
//! Runtime session deserialization intentionally discards durable summaries.
//! Migration therefore uses a separate raw byte-span parser. It retains at
//! most one JSONL record plus small normalized projections, and rewrites only
//! direct `recent_sessions[*].summary` value spans.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::de::IgnoredAny;
use sha2::{Digest, Sha256};

use super::json_scan::{skip_json_string, skip_json_value, skip_whitespace};

const JSONL_READ_CHUNK: usize = 16 * 1024;
const MAX_EMBEDDED_VALIDATION_CACHE_ENTRIES: usize = 4096;

#[derive(Debug)]
pub(super) struct SessionCompactionAnalysis {
    roots: BTreeMap<String, CanonicalRoot>,
    logical_order: Vec<String>,
    pub(super) source_bytes: u64,
    pub(super) source_sha256: String,
    pub(super) physical_records: usize,
    pub(super) logical_records: usize,
    pub(super) duplicate_records: usize,
    pub(super) records_changed: usize,
    pub(super) recursive_references: usize,
    pub(super) compacted_bytes: u64,
}

impl SessionCompactionAnalysis {
    pub(super) fn empty() -> Self {
        Self {
            roots: BTreeMap::new(),
            logical_order: Vec::new(),
            source_bytes: 0,
            source_sha256: digest_hex(&Sha256::digest(b"")),
            physical_records: 0,
            logical_records: 0,
            duplicate_records: 0,
            records_changed: 0,
            recursive_references: 0,
            compacted_bytes: 0,
        }
    }

    pub(super) fn bytes_reclaimable(&self) -> u64 {
        self.source_bytes.saturating_sub(self.compacted_bytes)
    }

    pub(super) fn needs_rewrite(&self) -> bool {
        self.records_changed != 0 || self.duplicate_records != 0
    }

    pub(super) fn same_logical_state(&self, other: &Self) -> bool {
        self.roots == other.roots && self.logical_order == other.logical_order
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CanonicalRoot {
    event: EventProjection,
    summary: SummaryFieldProjection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EventProjection {
    envelope: EventEnvelope,
    extra: BTreeMap<String, ValueDigest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EventEnvelope {
    id: String,
    session_id: String,
    event: String,
    timestamp_ms: u64,
    outcome: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SummaryFieldProjection {
    Missing,
    Null,
    Object(SummaryProjection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SummaryProjection {
    fields: BTreeMap<String, ValueDigest>,
    recent_sessions: RecentSessionsProjection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RecentSessionsProjection {
    Missing,
    Null,
    Array(Vec<EventProjection>),
}

type ValueDigest = [u8; 32];

#[derive(Debug)]
struct ParsedRoot {
    id: String,
    canonical: CanonicalRoot,
    references: Vec<ParsedReference>,
}

#[derive(Debug)]
struct ParsedReference {
    event: EventProjection,
    summary: RawSummaryField,
}

#[derive(Clone, Debug)]
enum RawSummaryField {
    Missing,
    Null,
    Object(Range<usize>),
}

struct ParsedEvent {
    event: EventProjection,
    summary: RawSummaryField,
}

struct ParsedSummary {
    projection: SummaryProjection,
    references: Vec<ParsedReference>,
}

#[derive(Debug)]
struct FileScanReport {
    bytes: u64,
    sha256: String,
}

#[derive(Default)]
struct EmbeddedValidationCache {
    by_event_id: HashMap<String, HashSet<ValueDigest>>,
    entries: usize,
}

impl EmbeddedValidationCache {
    fn contains(&self, event_id: &str, digest: &ValueDigest) -> bool {
        self.by_event_id
            .get(event_id)
            .is_some_and(|entries| entries.contains(digest))
    }

    fn insert(&mut self, event_id: &str, digest: ValueDigest) {
        if self.entries >= MAX_EMBEDDED_VALIDATION_CACHE_ENTRIES {
            return;
        }
        let inserted = self
            .by_event_id
            .entry(event_id.to_string())
            .or_default()
            .insert(digest);
        if inserted {
            self.entries = self.entries.saturating_add(1);
        }
    }
}

pub(super) fn analyze_session_compaction(path: &Path) -> Result<SessionCompactionAnalysis> {
    let mut roots = BTreeMap::<String, CanonicalRoot>::new();
    let mut logical_order = Vec::new();
    let mut physical_records = 0usize;
    let mut line_buffer = Vec::new();

    let first_scan = scan_file(path, &mut line_buffer, |line_number, raw, blank| {
        if blank {
            return Ok(());
        }
        physical_records = physical_records.saturating_add(1);
        let parsed = parse_root_record(raw, line_number, path, true)?;
        match roots.get(&parsed.id) {
            Some(existing) if existing == &parsed.canonical => {}
            Some(_) => {
                bail!(
                    "Session event id {} has divergent canonical records in {}",
                    parsed.id,
                    path.display()
                );
            }
            None => {
                logical_order.push(parsed.id.clone());
                roots.insert(parsed.id, parsed.canonical);
            }
        }
        Ok(())
    })?;

    let logical_records = roots.len();
    let duplicate_records = physical_records.saturating_sub(logical_records);
    let mut records_changed = 0usize;
    let mut recursive_references = 0usize;
    let mut compacted_bytes = 0u64;
    let mut emitted = HashSet::new();
    let mut embedded_validation_cache = EmbeddedValidationCache::default();

    let second_scan = scan_file(path, &mut line_buffer, |line_number, raw, blank| {
        if blank {
            compacted_bytes = compacted_bytes.saturating_add(raw.len() as u64 + 1);
            return Ok(());
        }
        let parsed = parse_root_record(raw, line_number, path, false)?;
        recursive_references = recursive_references.saturating_add(validate_direct_references(
            raw,
            &parsed,
            &roots,
            line_number,
            path,
            &mut embedded_validation_cache,
        )?);
        if !emitted.insert(parsed.id) {
            return Ok(());
        }
        let edits = replacement_ranges(&parsed.references);
        if !edits.is_empty() {
            records_changed = records_changed.saturating_add(1);
        }
        compacted_bytes = compacted_bytes
            .saturating_add(compacted_record_len(raw.len(), &edits, line_number, path)? as u64 + 1);
        Ok(())
    })?;
    if first_scan.bytes != second_scan.bytes || first_scan.sha256 != second_scan.sha256 {
        bail!(
            "Session state changed during compaction analysis; rerun {}",
            path.display()
        );
    }

    Ok(SessionCompactionAnalysis {
        roots,
        logical_order,
        source_bytes: first_scan.bytes,
        source_sha256: first_scan.sha256,
        physical_records,
        logical_records,
        duplicate_records,
        records_changed,
        recursive_references,
        compacted_bytes,
    })
}

pub(super) fn write_compacted_sessions(
    path: &Path,
    analysis: &SessionCompactionAnalysis,
    writer: &mut dyn Write,
) -> Result<()> {
    let mut emitted = HashSet::new();
    let mut emitted_order = Vec::new();
    let mut output_bytes = 0u64;
    let mut line_buffer = Vec::new();

    let scan = scan_file(path, &mut line_buffer, |line_number, raw, blank| {
        if blank {
            writer.write_all(raw)?;
            writer.write_all(b"\n")?;
            output_bytes = output_bytes.saturating_add(raw.len() as u64 + 1);
            return Ok(());
        }
        let parsed = parse_root_record(raw, line_number, path, false)?;
        if !emitted.insert(parsed.id.clone()) {
            return Ok(());
        }
        emitted_order.push(parsed.id);
        let edits = replacement_ranges(&parsed.references);
        output_bytes = output_bytes.saturating_add(
            write_compacted_record(writer, raw, &edits, line_number, path)? as u64 + 1,
        );
        writer.write_all(b"\n")?;
        Ok(())
    })?;

    if scan.bytes != analysis.source_bytes || scan.sha256 != analysis.source_sha256 {
        bail!(
            "Session state changed after compaction analysis; rerun instead of rewriting {}",
            path.display()
        );
    }
    if emitted_order != analysis.logical_order || output_bytes != analysis.compacted_bytes {
        bail!(
            "Compacted session output diverged from its validated projection for {}",
            path.display()
        );
    }
    Ok(())
}

fn parse_root_record(
    raw: &[u8],
    line_number: usize,
    path: &Path,
    validate_full_json: bool,
) -> Result<ParsedRoot> {
    if validate_full_json {
        serde_json::from_slice::<IgnoredAny>(raw).with_context(|| {
            format!(
                "Failed to parse session JSONL record {line_number} in {}",
                path.display()
            )
        })?;
    }
    let parsed = parse_event(raw, 0..raw.len(), line_number, path)?;
    let id = parsed.event.envelope.id.clone();
    let (summary, references) = match &parsed.summary {
        RawSummaryField::Missing => (SummaryFieldProjection::Missing, Vec::new()),
        RawSummaryField::Null => (SummaryFieldProjection::Null, Vec::new()),
        RawSummaryField::Object(range) => {
            let parsed_summary = parse_summary(raw, range.clone(), line_number, path)?;
            (
                SummaryFieldProjection::Object(parsed_summary.projection),
                parsed_summary.references,
            )
        }
    };
    Ok(ParsedRoot {
        id,
        canonical: CanonicalRoot {
            event: parsed.event,
            summary,
        },
        references,
    })
}

fn parse_event(
    input: &[u8],
    range: Range<usize>,
    line_number: usize,
    path: &Path,
) -> Result<ParsedEvent> {
    let mut members = object_member_map(input, range, line_number, path, "session event")?;
    let id = parse_required_string(input, members.remove("id"), "id", line_number, path)?;
    let session_id = parse_required_string(
        input,
        members.remove("session_id"),
        "session_id",
        line_number,
        path,
    )?;
    let event = parse_required_string(input, members.remove("event"), "event", line_number, path)?;
    let timestamp_ms = parse_required_u64(
        input,
        members.remove("timestamp_ms"),
        "timestamp_ms",
        line_number,
        path,
    )?;
    let outcome = match members.remove("outcome") {
        None => None,
        Some(range) if value_is_null(input, &range) => None,
        Some(range) => Some(parse_string(input, &range, "outcome", line_number, path)?),
    };
    let summary = parse_summary_field(input, members.remove("summary"), line_number, path)?;
    let extra = members
        .into_iter()
        .map(|(key, range)| {
            semantic_value_digest(input, range, line_number, path).map(|digest| (key, digest))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    Ok(ParsedEvent {
        event: EventProjection {
            envelope: EventEnvelope {
                id,
                session_id,
                event,
                timestamp_ms,
                outcome,
            },
            extra,
        },
        summary,
    })
}

fn parse_summary(
    input: &[u8],
    range: Range<usize>,
    line_number: usize,
    path: &Path,
) -> Result<ParsedSummary> {
    let mut members = object_member_map(input, range, line_number, path, "session summary")?;
    let (recent_sessions, references) = match members.remove("recent_sessions") {
        None => (RecentSessionsProjection::Missing, Vec::new()),
        Some(range) if value_is_null(input, &range) => (RecentSessionsProjection::Null, Vec::new()),
        Some(range) if first_non_whitespace(input, &range) == Some(b'[') => {
            let values = array_value_ranges(input, range, line_number, path)?;
            let mut projections = Vec::with_capacity(values.len());
            let mut references = Vec::with_capacity(values.len());
            for value in values {
                let parsed = parse_event(input, value, line_number, path).with_context(|| {
                    format!(
                        "Failed to parse recent session reference in record {line_number} of {}",
                        path.display()
                    )
                })?;
                projections.push(parsed.event.clone());
                references.push(ParsedReference {
                    event: parsed.event,
                    summary: parsed.summary,
                });
            }
            (RecentSessionsProjection::Array(projections), references)
        }
        Some(_) => {
            bail!(
                "recent_sessions in record {line_number} of {} must be an array or null",
                path.display()
            );
        }
    };
    let fields = members
        .into_iter()
        .map(|(key, range)| {
            semantic_value_digest(input, range, line_number, path).map(|digest| (key, digest))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(ParsedSummary {
        projection: SummaryProjection {
            fields,
            recent_sessions,
        },
        references,
    })
}

fn parse_summary_field(
    input: &[u8],
    range: Option<Range<usize>>,
    line_number: usize,
    path: &Path,
) -> Result<RawSummaryField> {
    let Some(range) = range else {
        return Ok(RawSummaryField::Missing);
    };
    let range = trim_range(input, range);
    if value_is_null(input, &range) {
        return Ok(RawSummaryField::Null);
    }
    if first_non_whitespace(input, &range) == Some(b'{') {
        return Ok(RawSummaryField::Object(range));
    }
    bail!(
        "summary in record {line_number} of {} must be an object or null",
        path.display()
    )
}

fn validate_direct_references(
    input: &[u8],
    parsed: &ParsedRoot,
    roots: &BTreeMap<String, CanonicalRoot>,
    line_number: usize,
    path: &Path,
    cache: &mut EmbeddedValidationCache,
) -> Result<usize> {
    let recursive_references = parsed
        .references
        .iter()
        .filter(|reference| matches!(reference.summary, RawSummaryField::Object(_)))
        .count();
    validate_references(
        input,
        &parsed.id,
        &parsed.references,
        roots,
        line_number,
        path,
        cache,
    )?;
    Ok(recursive_references)
}

fn validate_references(
    input: &[u8],
    owner_id: &str,
    references: &[ParsedReference],
    roots: &BTreeMap<String, CanonicalRoot>,
    line_number: usize,
    path: &Path,
    cache: &mut EmbeddedValidationCache,
) -> Result<()> {
    for reference in references {
        let id = &reference.event.envelope.id;
        let Some(root) = roots.get(id) else {
            bail!(
                "Session event {} references orphan event id {} in record {line_number} of {}",
                owner_id,
                id,
                path.display()
            );
        };
        if root.event != reference.event {
            bail!(
                "Session event {} references divergent event fields for event id {} in record {line_number} of {}",
                owner_id,
                id,
                path.display()
            );
        }
        if let RawSummaryField::Object(range) = &reference.summary {
            let digest: ValueDigest = Sha256::digest(&input[range.clone()]).into();
            if cache.contains(id, &digest) {
                continue;
            }
            let embedded = parse_summary(input, range.clone(), line_number, path)?;
            if root.summary != SummaryFieldProjection::Object(embedded.projection) {
                bail!(
                    "Session event {} embeds a non-canonical summary for event id {} in record {line_number} of {}",
                    owner_id,
                    id,
                    path.display()
                );
            }
            validate_references(
                input,
                id,
                &embedded.references,
                roots,
                line_number,
                path,
                cache,
            )?;
            cache.insert(id, digest);
        }
    }
    Ok(())
}

fn replacement_ranges(references: &[ParsedReference]) -> Vec<Range<usize>> {
    references
        .iter()
        .filter_map(|reference| match &reference.summary {
            RawSummaryField::Object(range) => Some(range.clone()),
            RawSummaryField::Missing | RawSummaryField::Null => None,
        })
        .collect()
}

fn compacted_record_len(
    source_len: usize,
    edits: &[Range<usize>],
    line_number: usize,
    path: &Path,
) -> Result<usize> {
    validate_edits(source_len, edits, line_number, path)?;
    let mut length = source_len;
    for edit in edits {
        length = length
            .checked_sub(edit.len())
            .and_then(|value| value.checked_add(4))
            .ok_or_else(|| anyhow!("Compacted record length overflow"))?;
    }
    Ok(length)
}

fn write_compacted_record(
    writer: &mut dyn Write,
    raw: &[u8],
    edits: &[Range<usize>],
    line_number: usize,
    path: &Path,
) -> Result<usize> {
    validate_edits(raw.len(), edits, line_number, path)?;
    let mut cursor = 0usize;
    for edit in edits {
        writer.write_all(&raw[cursor..edit.start])?;
        writer.write_all(b"null")?;
        cursor = edit.end;
    }
    writer.write_all(&raw[cursor..])?;
    compacted_record_len(raw.len(), edits, line_number, path)
}

fn validate_edits(
    source_len: usize,
    edits: &[Range<usize>],
    line_number: usize,
    path: &Path,
) -> Result<()> {
    let mut end = 0usize;
    for edit in edits {
        if edit.start < end || edit.start >= edit.end || edit.end > source_len {
            bail!(
                "Invalid or overlapping compaction span in record {line_number} of {}",
                path.display()
            );
        }
        end = edit.end;
    }
    Ok(())
}

fn object_member_map(
    input: &[u8],
    range: Range<usize>,
    line_number: usize,
    path: &Path,
    label: &str,
) -> Result<BTreeMap<String, Range<usize>>> {
    let members = object_member_ranges(input, range).with_context(|| {
        format!(
            "Failed to inspect {label} in record {line_number} of {}",
            path.display()
        )
    })?;
    let mut map = BTreeMap::new();
    for (key, value) in members {
        if map.insert(key.clone(), value).is_some() {
            bail!(
                "Duplicate key {key:?} in {label} at record {line_number} of {}",
                path.display()
            );
        }
    }
    Ok(map)
}

fn parse_required_string(
    input: &[u8],
    range: Option<Range<usize>>,
    field: &str,
    line_number: usize,
    path: &Path,
) -> Result<String> {
    let range = range.with_context(|| {
        format!(
            "Session record {line_number} in {} is missing {field}",
            path.display()
        )
    })?;
    parse_string(input, &range, field, line_number, path)
}

fn parse_string(
    input: &[u8],
    range: &Range<usize>,
    field: &str,
    line_number: usize,
    path: &Path,
) -> Result<String> {
    serde_json::from_slice(&input[range.clone()]).with_context(|| {
        format!(
            "Session record {line_number} in {} has invalid {field}",
            path.display()
        )
    })
}

fn parse_required_u64(
    input: &[u8],
    range: Option<Range<usize>>,
    field: &str,
    line_number: usize,
    path: &Path,
) -> Result<u64> {
    let range = range.with_context(|| {
        format!(
            "Session record {line_number} in {} is missing {field}",
            path.display()
        )
    })?;
    serde_json::from_slice(&input[range]).with_context(|| {
        format!(
            "Session record {line_number} in {} has invalid {field}",
            path.display()
        )
    })
}

fn semantic_value_digest(
    input: &[u8],
    range: Range<usize>,
    line_number: usize,
    path: &Path,
) -> Result<ValueDigest> {
    let range = trim_range(input, range);
    let mut hasher = Sha256::new();
    match first_non_whitespace(input, &range) {
        Some(b'n') if value_is_null(input, &range) => hasher.update(b"null"),
        Some(b't') if &input[range.clone()] == b"true" => hasher.update(b"true"),
        Some(b'f') if &input[range.clone()] == b"false" => hasher.update(b"false"),
        Some(b'"') => {
            hasher.update(b"string\0");
            hash_decoded_json_string(&mut hasher, input, &range).with_context(|| {
                format!(
                    "Invalid JSON string in record {line_number} of {}",
                    path.display()
                )
            })?;
        }
        Some(b'[') => {
            hasher.update(b"array\0");
            let mut value_count = 0u64;
            visit_array_value_ranges(input, range, line_number, path, |value| {
                hasher.update(semantic_value_digest(input, value, line_number, path)?);
                value_count = value_count.saturating_add(1);
                Ok(())
            })?;
            hasher.update(value_count.to_be_bytes());
        }
        Some(b'{') => {
            hasher.update(b"object\0");
            let members = object_member_map(input, range, line_number, path, "JSON object")?;
            hasher.update((members.len() as u64).to_be_bytes());
            for (key, value) in members {
                hash_component(&mut hasher, key.as_bytes());
                hasher.update(semantic_value_digest(input, value, line_number, path)?);
            }
        }
        Some(_) => {
            hasher.update(b"number\0");
            let number: serde_json::Number =
                serde_json::from_slice(&input[range]).with_context(|| {
                    format!(
                        "Invalid JSON scalar in record {line_number} of {}",
                        path.display()
                    )
                })?;
            hash_component(&mut hasher, number.to_string().as_bytes());
        }
        None => bail!(
            "Empty JSON value in record {line_number} of {}",
            path.display()
        ),
    }
    Ok(hasher.finalize().into())
}

fn hash_component(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn hash_decoded_json_string(hasher: &mut Sha256, input: &[u8], range: &Range<usize>) -> Result<()> {
    if range.len() < 2
        || input.get(range.start) != Some(&b'"')
        || input.get(range.end - 1) != Some(&b'"')
    {
        bail!("Expected a quoted JSON string");
    }
    let mut cursor = range.start + 1;
    let end = range.end - 1;
    let mut plain_start = cursor;
    while cursor < end {
        match input[cursor] {
            b'\\' => {
                hasher.update(&input[plain_start..cursor]);
                let escape = *input
                    .get(cursor + 1)
                    .context("Truncated JSON string escape")?;
                match escape {
                    b'"' | b'\\' | b'/' => {
                        hasher.update([escape]);
                        cursor += 2;
                    }
                    b'b' => {
                        hasher.update([0x08]);
                        cursor += 2;
                    }
                    b'f' => {
                        hasher.update([0x0c]);
                        cursor += 2;
                    }
                    b'n' => {
                        hasher.update(b"\n");
                        cursor += 2;
                    }
                    b'r' => {
                        hasher.update(b"\r");
                        cursor += 2;
                    }
                    b't' => {
                        hasher.update(b"\t");
                        cursor += 2;
                    }
                    b'u' => {
                        let high = parse_json_hex_quad(input, cursor + 2, end)?;
                        cursor += 6;
                        let scalar = if (0xd800..=0xdbff).contains(&high) {
                            if input.get(cursor..cursor + 2) != Some(b"\\u") {
                                bail!("High JSON surrogate is not followed by a low surrogate");
                            }
                            let low = parse_json_hex_quad(input, cursor + 2, end)?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                bail!(
                                    "High JSON surrogate is followed by an invalid low surrogate"
                                );
                            }
                            cursor += 6;
                            0x1_0000 + (((high - 0xd800) as u32) << 10) + (low - 0xdc00) as u32
                        } else if (0xdc00..=0xdfff).contains(&high) {
                            bail!("Unpaired low JSON surrogate");
                        } else {
                            high as u32
                        };
                        let character =
                            char::from_u32(scalar).context("Invalid JSON Unicode scalar")?;
                        let mut encoded = [0u8; 4];
                        hasher.update(character.encode_utf8(&mut encoded).as_bytes());
                    }
                    _ => bail!("Unsupported JSON string escape"),
                }
                plain_start = cursor;
            }
            b'"' => bail!("Unexpected unescaped quote inside JSON string"),
            _ => cursor += 1,
        }
    }
    hasher.update(&input[plain_start..end]);
    Ok(())
}

fn parse_json_hex_quad(input: &[u8], start: usize, end: usize) -> Result<u16> {
    let bytes = input
        .get(start..start + 4)
        .filter(|_| start + 4 <= end)
        .context("Truncated JSON Unicode escape")?;
    let mut value = 0u16;
    for byte in bytes {
        let digit = match byte {
            b'0'..=b'9' => (byte - b'0') as u16,
            b'a'..=b'f' => (byte - b'a' + 10) as u16,
            b'A'..=b'F' => (byte - b'A' + 10) as u16,
            _ => bail!("Invalid hex digit in JSON Unicode escape"),
        };
        value = (value << 4) | digit;
    }
    Ok(value)
}

fn object_member_ranges(input: &[u8], range: Range<usize>) -> Result<Vec<(String, Range<usize>)>> {
    let range = trim_range(input, range);
    let mut cursor = range.start;
    if input.get(cursor) != Some(&b'{') {
        bail!("Expected JSON object at byte {cursor}");
    }
    cursor += 1;
    let mut members = Vec::new();
    loop {
        cursor = skip_whitespace(input, cursor, range.end);
        match input.get(cursor) {
            Some(b'}') => return Ok(members),
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
        members.push((key, value_start..cursor));
        cursor = skip_whitespace(input, cursor, range.end);
        match input.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b'}') => return Ok(members),
            _ => bail!("Expected ',' or '}}' at byte {cursor}"),
        }
    }
}

fn array_value_ranges(
    input: &[u8],
    range: Range<usize>,
    line_number: usize,
    path: &Path,
) -> Result<Vec<Range<usize>>> {
    let range = trim_range(input, range);
    let mut cursor = range.start;
    if input.get(cursor) != Some(&b'[') {
        bail!(
            "Expected JSON array in record {line_number} of {}",
            path.display()
        );
    }
    cursor += 1;
    let mut values = Vec::new();
    loop {
        cursor = skip_whitespace(input, cursor, range.end);
        if input.get(cursor) == Some(&b']') {
            return Ok(values);
        }
        let value_start = cursor;
        cursor = skip_json_value(input, cursor, range.end)?;
        values.push(value_start..cursor);
        cursor = skip_whitespace(input, cursor, range.end);
        match input.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b']') => return Ok(values),
            _ => bail!(
                "Expected ',' or ']' in record {line_number} of {}",
                path.display()
            ),
        }
    }
}

fn visit_array_value_ranges(
    input: &[u8],
    range: Range<usize>,
    line_number: usize,
    path: &Path,
    mut visitor: impl FnMut(Range<usize>) -> Result<()>,
) -> Result<()> {
    let range = trim_range(input, range);
    let mut cursor = range.start;
    if input.get(cursor) != Some(&b'[') {
        bail!(
            "Expected JSON array in record {line_number} of {}",
            path.display()
        );
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
            _ => {
                bail!(
                    "Expected ',' or ']' in record {line_number} of {}",
                    path.display()
                );
            }
        }
    }
}

fn trim_range(input: &[u8], range: Range<usize>) -> Range<usize> {
    let start = skip_whitespace(input, range.start, range.end);
    let mut end = range.end;
    while end > start && input[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    start..end
}

fn first_non_whitespace(input: &[u8], range: &Range<usize>) -> Option<u8> {
    let cursor = skip_whitespace(input, range.start, range.end);
    input.get(cursor).copied()
}

fn value_is_null(input: &[u8], range: &Range<usize>) -> bool {
    input[trim_range(input, range.clone())] == *b"null"
}

fn scan_file(
    path: &Path,
    line: &mut Vec<u8>,
    mut visitor: impl FnMut(usize, &[u8], bool) -> Result<()>,
) -> Result<FileScanReport> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileScanReport {
                bytes: 0,
                sha256: digest_hex(&Sha256::digest(b"")),
            });
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to open {}", path.display()));
        }
    };
    let mut reader = BufReader::with_capacity(JSONL_READ_CHUNK, file);
    let mut line_number = 0usize;
    let mut bytes = 0u64;
    let mut hasher = Sha256::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', line)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        line_number = line_number.saturating_add(1);
        bytes = bytes.saturating_add(read as u64);
        hasher.update(line.as_slice());
        if line.last() != Some(&b'\n') {
            bail!(
                "Refusing to compact unterminated JSONL line {line_number} in {}",
                path.display()
            );
        }
        line.pop();
        let blank = line.iter().all(u8::is_ascii_whitespace);
        visitor(line_number, line.as_slice(), blank)?;
    }
    Ok(FileScanReport {
        bytes,
        sha256: digest_hex(&hasher.finalize()),
    })
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    fn start(id: &str, timestamp_ms: u64, recent: Vec<Value>) -> Value {
        json!({
            "id": id,
            "session_id": id.replace("event", "session"),
            "event": "start",
            "timestamp_ms": timestamp_ms,
            "outcome": null,
            "summary": {
                "default_branch": "main",
                "open_plans": [],
                "recent_decisions": [],
                "recent_receipts": [],
                "recent_sessions": recent,
                "repo_name": "fixture",
                "source_commit": "abc",
                "source_path": "fixture"
            }
        })
    }

    #[test]
    fn recursive_history_compacts_only_nested_summary_spans() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let mut first = start("event-1", 1, Vec::new());
        first["future_root"] = json!({"key": "value"});
        first["future_reference"] = json!(["preserve", 1]);
        let mut second = start("event-2", 2, vec![first.clone()]);
        second["summary"]["source_commit"] = Value::Null;
        second["summary"]["future_nullable"] = Value::Null;
        second["future_root"] = json!({"preserve": true});
        let first_raw = serde_json::to_string(&first).unwrap();
        let second_raw = serde_json::to_string(&second).unwrap();
        let source = format!("\n{first_raw}\r\n{second_raw}\n");
        fs::write(&path, source.as_bytes()).unwrap();

        let analysis = analyze_session_compaction(&path).unwrap();
        assert_eq!(analysis.physical_records, 2);
        assert_eq!(analysis.logical_records, 2);
        assert_eq!(analysis.recursive_references, 1);
        assert_eq!(analysis.records_changed, 1);
        assert!(analysis.compacted_bytes < analysis.source_bytes);

        let mut output = Vec::new();
        write_compacted_sessions(&path, &analysis, &mut output).unwrap();
        assert!(output.starts_with(format!("\n{first_raw}\r\n").as_bytes()));
        let records = output
            .split(|byte| *byte == b'\n')
            .filter(|record| !record.iter().all(u8::is_ascii_whitespace))
            .map(|record| serde_json::from_slice::<Value>(record).unwrap())
            .collect::<Vec<_>>();
        let mut expected_second = second;
        expected_second["summary"]["recent_sessions"][0]["summary"] = Value::Null;
        assert_eq!(records[1], expected_second);
        fs::write(&path, &output).unwrap();
        let repeated = analyze_session_compaction(&path).unwrap();
        assert!(!repeated.needs_rewrite());
        assert!(analysis.same_logical_state(&repeated));
    }

    #[test]
    fn orphan_and_divergent_embedded_state_are_rejected() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let orphan = start("event-orphan", 1, Vec::new());
        let root = start("event-root", 2, vec![orphan]);
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&root).unwrap()),
        )
        .unwrap();
        let error = analyze_session_compaction(&path).unwrap_err().to_string();
        assert!(error.contains("orphan event id event-orphan"));

        let canonical = start("event-1", 1, Vec::new());
        let mut divergent = canonical.clone();
        divergent["summary"]["default_branch"] = json!("other");
        let root = start("event-2", 2, vec![divergent]);
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&canonical).unwrap(),
                serde_json::to_string(&root).unwrap()
            ),
        )
        .unwrap();
        let error = analyze_session_compaction(&path).unwrap_err().to_string();
        assert!(error.contains("non-canonical summary for event id event-1"));
    }

    #[test]
    fn divergent_third_level_embedded_summary_is_rejected() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let first = start("event-1", 1, Vec::new());
        let second = start("event-2", 2, vec![first.clone()]);
        let third = start("event-3", 3, vec![second.clone()]);
        let canonical_wrapper = start("event-4", 4, vec![third.clone()]);
        fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&second).unwrap(),
                serde_json::to_string(&third).unwrap(),
                serde_json::to_string(&canonical_wrapper).unwrap(),
            ),
        )
        .unwrap();
        let canonical = analyze_session_compaction(&path).unwrap();
        assert_eq!(canonical.recursive_references, 3);

        let mut divergent_third = third.clone();
        divergent_third["summary"]["recent_sessions"][0]["summary"]["recent_sessions"][0]["summary"]
            ["default_branch"] = json!("divergent");
        let wrapper = start("event-4", 4, vec![divergent_third]);
        fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&second).unwrap(),
                serde_json::to_string(&third).unwrap(),
                serde_json::to_string(&wrapper).unwrap(),
            ),
        )
        .unwrap();

        let error = analyze_session_compaction(&path).unwrap_err().to_string();

        assert!(error.contains("non-canonical summary for event id event-1"));
    }

    #[test]
    fn equivalent_duplicate_roots_collapse_but_unknown_field_divergence_fails() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let first = start("event-1", 1, Vec::new());
        let legacy = start("event-2", 2, vec![first.clone()]);
        let mut shallow = legacy.clone();
        shallow["summary"]["recent_sessions"][0]["summary"] = Value::Null;
        fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&legacy).unwrap(),
                serde_json::to_string(&shallow).unwrap()
            ),
        )
        .unwrap();
        let analysis = analyze_session_compaction(&path).unwrap();
        assert_eq!(analysis.duplicate_records, 1);

        let mut divergent = shallow;
        divergent["future_root"] = json!("different");
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&legacy).unwrap(),
                serde_json::to_string(&divergent).unwrap()
            ),
        )
        .unwrap();
        let error = analyze_session_compaction(&path).unwrap_err().to_string();
        assert!(error.contains("divergent canonical records"));
    }

    #[test]
    fn unknown_json_strings_hash_decoded_content_without_allocating_a_copy() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let literal = r#"{"id":"event-1","session_id":"session-1","event":"start","timestamp_ms":1,"summary":null,"future":"😀/a"}"#;
        let escaped = r#"{"id":"event-1","session_id":"session-1","event":"start","timestamp_ms":1,"summary":null,"future":"\ud83d\ude00\/\u0061"}"#;
        fs::write(&path, format!("{literal}\n{escaped}\n")).unwrap();

        let analysis = analyze_session_compaction(&path).unwrap();

        assert_eq!(analysis.logical_records, 1);
        assert_eq!(analysis.duplicate_records, 1);
    }

    #[test]
    fn malformed_duplicate_keys_and_torn_tails_fail_closed() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        for source in [
            "{\"id\":\"one\",\"id\":\"two\",\"session_id\":\"s\",\"event\":\"start\",\"timestamp_ms\":1,\"summary\":null}\n",
            "{\"id\":\"one\",\"session_id\":\"s\",\"event\":\"start\",\"timestamp_ms\":1,\"summary\":",
            "{\"id\":\"one\",\"session_id\":\"s\",\"event\":\"start\",\"timestamp_ms\":1,\"summary\":null}",
        ] {
            fs::write(&path, source).unwrap();
            assert!(analyze_session_compaction(&path).is_err());
        }
    }

    #[test]
    #[ignore = "allocates a >100 MB legacy record for manual stress validation"]
    fn compacts_record_larger_than_100_mb() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("sessions.jsonl");
        let mut first = start("event-1", 1, Vec::new());
        first["summary"]["future_payload"] = json!("x".repeat(51 * 1024 * 1024));
        let second = start("event-2", 2, vec![first.clone()]);
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&second).unwrap()
            ),
        )
        .unwrap();
        assert!(fs::metadata(&path).unwrap().len() > 100 * 1024 * 1024);
        let analysis = analyze_session_compaction(&path).unwrap();
        assert!(analysis.bytes_reclaimable() > 50 * 1024 * 1024);
    }
}
