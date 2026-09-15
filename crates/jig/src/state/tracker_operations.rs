//! Durable tracker side-effect intents and reconciliation facts.
//!
//! The journal is authoritative for deciding whether a tracker operation is
//! pending, so readers deliberately fail closed on corrupt, unsupported, or
//! unterminated records. Every record is an immutable fact. Exact retries are
//! idempotent; conflicting reuse of an event or operation identity is not.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::{RepoContext, TRACKER_JOURNAL_CONTRACT_VERSION, supports_tracker_journals};
use crate::strict_json;

use super::jsonl::{
    JsonlWriteGuard, RawJsonlRecord, append_jsonl_durable_locked, confirm_jsonl_durable_locked,
    scan_jsonl_raw_bounded, scan_jsonl_raw_locked_bounded, with_jsonl_write_lock,
};
use super::support::new_id;

use self::validation::{validate_append_transition, validate_fact, validate_merged_history};

mod validation;

pub(crate) const TRACKER_OPERATIONS_FILE: &str = "tracker-operations.jsonl";
pub(crate) const TRACKER_OPERATION_SCHEMA_VERSION: u32 = 1;

pub(crate) const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_CORRELATION_BYTES: usize = 512;
const MAX_DETAIL_BYTES: usize = 4 * 1024;
const MAX_REFERENCES_PER_EVENT: usize = 64;
const MAX_ATTEMPT: u32 = 10_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerOperationKind {
    Backlink,
    Claim,
    CompleteIssue,
    Export,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerOperationPhase {
    Intent,
    Attempt,
    Observation,
    Acknowledgement,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerOperationOutcome {
    Applied,
    NoEffect,
}

/// A repository-portable tracker reference. The provider database location is
/// intentionally absent; `tracker_root` is the repository-relative public
/// root and is currently required to be `.beads`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PortableTrackerIssueRef {
    pub(crate) provider: String,
    pub(crate) workspace_id: String,
    pub(crate) issue_id: String,
    pub(crate) tracker_root: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct TrackerOperationCorrelation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) provider_operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reconciliation_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) attempt: Option<u32>,
}

/// Version-one immutable fact. Unknown fields remain part of the raw semantic
/// record used for duplicate detection, even though typed consumers do not
/// need to understand them yet.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct TrackerOperationEventV1 {
    pub(crate) schema_version: u32,
    pub(crate) event_id: String,
    pub(crate) operation_id: String,
    pub(crate) plan_id: String,
    pub(crate) issue: PortableTrackerIssueRef,
    pub(crate) kind: TrackerOperationKind,
    pub(crate) phase: TrackerOperationPhase,
    pub(crate) timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) outcome: Option<TrackerOperationOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) correlation: Option<TrackerOperationCorrelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) receipt_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) run_ids: Vec<String>,
    /// Nonterminal fact IDs whose uncertain effects this acknowledgement
    /// resolves. A merged branch fact absent from this closure keeps the
    /// operation pending even when another branch contains an acknowledgement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) resolves_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

impl TrackerOperationEventV1 {
    pub(crate) fn new(
        operation_id: impl Into<String>,
        plan_id: impl Into<String>,
        issue: PortableTrackerIssueRef,
        kind: TrackerOperationKind,
        phase: TrackerOperationPhase,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            schema_version: TRACKER_OPERATION_SCHEMA_VERSION,
            event_id: new_tracker_operation_event_id(),
            operation_id: operation_id.into(),
            plan_id: plan_id.into(),
            issue,
            kind,
            phase,
            timestamp_ms,
            outcome: None,
            correlation: None,
            receipt_ids: Vec::new(),
            run_ids: Vec::new(),
            resolves_event_ids: Vec::new(),
            detail: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectedTrackerOperationEvent {
    pub(crate) fact: TrackerOperationEventV1,
    /// Complete semantic JSON, including fields unknown to this runtime.
    pub(crate) raw: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectedTrackerOperation {
    pub(crate) operation_id: String,
    pub(crate) plan_id: String,
    pub(crate) issue: PortableTrackerIssueRef,
    pub(crate) kind: TrackerOperationKind,
    pub(crate) events: Vec<ProjectedTrackerOperationEvent>,
    pub(crate) terminal_outcome: Option<TrackerOperationOutcome>,
}

impl ProjectedTrackerOperation {
    pub(crate) const fn is_pending(&self) -> bool {
        self.terminal_outcome.is_none()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PendingTrackerRetentionRoots {
    pub(crate) plan_ids: BTreeSet<String>,
    pub(crate) receipt_ids: BTreeSet<String>,
    pub(crate) run_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TrackerOperationProjection {
    pub(crate) operations: BTreeMap<String, ProjectedTrackerOperation>,
    pub(crate) pending_retention_roots: PendingTrackerRetentionRoots,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerOperationJournalAuthority {
    #[default]
    Empty,
    Supported,
    Unsupported,
    Conflicting,
    Corrupt,
    Torn,
    Unreadable,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct TrackerOperationJournalDiagnostics {
    pub(crate) authority: TrackerOperationJournalAuthority,
    pub(crate) operations: u64,
    pub(crate) events: u64,
    pub(crate) pending_operations: u64,
    pub(crate) terminal_operations: u64,
    pub(crate) applied_operations: u64,
    pub(crate) no_effect_operations: u64,
    pub(crate) pending_plan_roots: u64,
    pub(crate) pending_receipt_roots: u64,
    pub(crate) pending_run_roots: u64,
    pub(crate) error_count: u64,
    pub(crate) errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerOperationAppendOutcome {
    Appended,
    AlreadyPresent,
}

pub(crate) fn new_tracker_operation_id() -> String {
    new_id("tracker-operation")
}

pub(crate) fn new_tracker_operation_event_id() -> String {
    new_id("tracker-operation-event")
}

/// A separate helper keeps the epoch boundary usable by configuration and
/// launcher compatibility tests without requiring a journal write.
pub(crate) fn ensure_tracker_operations_write_contract_version(
    contract_version: u32,
) -> Result<()> {
    ensure!(
        supports_tracker_journals(contract_version),
        "tracker operation journal writes require contract version {}; repository contract is {}",
        TRACKER_JOURNAL_CONTRACT_VERSION,
        contract_version
    );
    Ok(())
}

pub(crate) fn ensure_tracker_operations_write_supported(ctx: &RepoContext) -> Result<()> {
    ensure_tracker_operations_write_contract_version(ctx.contract_version())
}

pub(crate) fn tracker_operation_projection(
    ctx: &RepoContext,
) -> Result<TrackerOperationProjection> {
    tracker_operation_projection_with_cancellation(ctx, &|| false)
}

pub(crate) fn tracker_operation_projection_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<TrackerOperationProjection> {
    tracker_operation_projection_from_path(&ctx.state_file(TRACKER_OPERATIONS_FILE), cancelled)
}

/// Path-based entry point for diagnostics and maintenance validation. Missing
/// journals return an empty projection without creating directories or locks.
pub(crate) fn tracker_operation_projection_from_path(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<TrackerOperationProjection> {
    let mut builder = ProjectionBuilder::default();
    let scan = scan_jsonl_raw_bounded(path, cancelled, MAX_RECORD_BYTES, |record| {
        builder.observe_raw(record, path)
    })?;
    ensure!(
        !scan.unterminated_final_record,
        "Refusing tracker operation authority because {} has an unterminated final record",
        path.display()
    );
    builder.finish()
}

/// Return a bounded semantic summary from the authoritative operation
/// projection. A missing journal produces an empty summary without creating
/// its parent directory or a lock file.
pub(crate) fn tracker_operation_journal_diagnostics_from_path(
    path: &Path,
) -> TrackerOperationJournalDiagnostics {
    match tracker_operation_projection_from_path(path, &|| false) {
        Ok(projection) => {
            let pending_operations = projection
                .operations
                .values()
                .filter(|operation| operation.is_pending())
                .count() as u64;
            let terminal_operations = projection.operations.len() as u64 - pending_operations;
            let applied_operations = projection
                .operations
                .values()
                .filter(|operation| {
                    operation.terminal_outcome == Some(TrackerOperationOutcome::Applied)
                })
                .count() as u64;
            let no_effect_operations = projection
                .operations
                .values()
                .filter(|operation| {
                    operation.terminal_outcome == Some(TrackerOperationOutcome::NoEffect)
                })
                .count() as u64;
            TrackerOperationJournalDiagnostics {
                authority: if projection.operations.is_empty() {
                    TrackerOperationJournalAuthority::Empty
                } else {
                    TrackerOperationJournalAuthority::Supported
                },
                operations: projection.operations.len() as u64,
                events: projection
                    .operations
                    .values()
                    .map(|operation| operation.events.len() as u64)
                    .sum(),
                pending_operations,
                terminal_operations,
                applied_operations,
                no_effect_operations,
                pending_plan_roots: projection.pending_retention_roots.plan_ids.len() as u64,
                pending_receipt_roots: projection.pending_retention_roots.receipt_ids.len() as u64,
                pending_run_roots: projection.pending_retention_roots.run_ids.len() as u64,
                ..TrackerOperationJournalDiagnostics::default()
            }
        }
        Err(error) => {
            let detail = format!("{error:#}");
            let authority = classify_tracker_operation_diagnostic(&detail, &error);
            TrackerOperationJournalDiagnostics {
                authority,
                error_count: 1,
                errors: vec![super::support::truncate(&detail)],
                ..TrackerOperationJournalDiagnostics::default()
            }
        }
    }
}

fn classify_tracker_operation_diagnostic(
    detail: &str,
    error: &anyhow::Error,
) -> TrackerOperationJournalAuthority {
    if detail.contains("unterminated final record") {
        TrackerOperationJournalAuthority::Torn
    } else if detail.contains("unsupported schema version")
        || detail.contains("unknown variant")
        || detail.contains("unsupported tracker provider")
    {
        TrackerOperationJournalAuthority::Unsupported
    } else if detail.contains("conflicting semantic records")
        || detail.contains("conflicting immutable identity")
        || detail.contains("conflicting terminal outcomes")
        || detail.contains("expected exactly one intent fact")
        || detail.contains("invalid tracker operation")
    {
        TrackerOperationJournalAuthority::Conflicting
    } else if error.downcast_ref::<std::io::Error>().is_some() {
        TrackerOperationJournalAuthority::Unreadable
    } else {
        TrackerOperationJournalAuthority::Corrupt
    }
}

/// Append one fact after projecting the journal under its writer lock. Passing
/// the same complete event again is an idempotent success; reusing its ID with
/// different semantic JSON is a conflict.
pub(crate) fn append_tracker_operation_event(
    ctx: &RepoContext,
    event: &TrackerOperationEventV1,
) -> Result<TrackerOperationAppendOutcome> {
    ensure_tracker_operations_write_supported(ctx)?;
    validate_fact(event)?;
    super::plans::ensure_plan_exists(ctx, &event.plan_id)?;
    match super::work_links::project_work_link(ctx, &event.plan_id)? {
        super::work_links::WorkLinkProjection::Supported(link)
            if link.record.issue.provider == event.issue.provider
                && link.record.issue.workspace_id == event.issue.workspace_id
                && link.record.issue.issue_id == event.issue.issue_id
                && link.record.issue.tracker_root == event.issue.tracker_root => {}
        super::work_links::WorkLinkProjection::Supported(_) => {
            anyhow::bail!(
                "tracker operation '{}' does not match the immutable work link for plan {}",
                event.operation_id,
                event.plan_id
            );
        }
        super::work_links::WorkLinkProjection::Unlinked => {
            anyhow::bail!(
                "tracker operation '{}' requires an immutable work link for plan {}",
                event.operation_id,
                event.plan_id
            );
        }
        super::work_links::WorkLinkProjection::Conflict(diagnostics)
        | super::work_links::WorkLinkProjection::Unsupported(diagnostics)
        | super::work_links::WorkLinkProjection::Corrupt(diagnostics) => {
            let detail = diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.as_str())
                .unwrap_or("no diagnostic details");
            anyhow::bail!(
                "tracker operation '{}' has ambiguous work-link authority for plan {}: {detail}",
                event.operation_id,
                event.plan_id
            );
        }
    }
    let path = ctx.state_file(TRACKER_OPERATIONS_FILE);
    append_tracker_operation_event_at_path(&path, event)
}

fn append_tracker_operation_event_at_path(
    path: &Path,
    event: &TrackerOperationEventV1,
) -> Result<TrackerOperationAppendOutcome> {
    validate_fact(event)?;
    let encoded =
        serde_json::to_vec(event).context("Failed to serialize tracker operation fact")?;
    ensure!(
        encoded.len() <= MAX_RECORD_BYTES,
        "tracker operation fact exceeds the {}-byte limit",
        MAX_RECORD_BYTES
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let raw = strict_json::from_slice(&encoded)
        .context("Failed to reparse serialized tracker operation fact")?;
    with_jsonl_write_lock(path, |guard| {
        append_tracker_operation_event_locked(guard, path, event, raw)
    })
}

fn append_tracker_operation_event_locked(
    guard: &JsonlWriteGuard,
    path: &Path,
    event: &TrackerOperationEventV1,
    raw: Value,
) -> Result<TrackerOperationAppendOutcome> {
    let mut builder = ProjectionBuilder::default();
    let scan = scan_jsonl_raw_locked_bounded(guard, path, &|| false, MAX_RECORD_BYTES, |record| {
        builder.observe_raw(record, path)
    })?;
    ensure!(
        !scan.unterminated_final_record,
        "Refusing to append to {} because its final record is not newline-terminated",
        path.display()
    );

    if let Some(existing) = builder.events_by_id.get(&event.event_id) {
        ensure!(
            existing == &raw,
            "tracker operation event ID '{}' has conflicting semantic records",
            event.event_id
        );
        // Finish the projection even on a retry: unrelated corruption or an
        // invalid later transition must never be bypassed by idempotency.
        builder.finish()?;
        // The prior attempt may have made the exact record visible before its
        // file or directory sync reported failure. Re-confirm both boundaries
        // before an idempotent result can authorize external work.
        confirm_jsonl_durable_locked(guard, path)?;
        return Ok(TrackerOperationAppendOutcome::AlreadyPresent);
    }
    // The lock-protected physical tail is the sequential writer authority.
    // `finish` canonicalizes a union-merged projection for readers, so doing
    // this check afterward would let caller-controlled timestamps reorder the
    // lifecycle used by the next append.
    validate_append_transition(builder.operations.get(&event.operation_id), event)?;
    builder.finish()?;
    append_jsonl_durable_locked(guard, path, event)?;
    Ok(TrackerOperationAppendOutcome::Appended)
}

/// Hold the tracker-operation writer lock while an archive uses one stable set
/// of pending roots. Intent writers use the same lock, so a new intent cannot
/// appear between retention projection and the archive callback.
pub(crate) fn with_tracker_operations_coordination_lock<T>(
    ctx: &RepoContext,
    operation: impl FnOnce(&PendingTrackerRetentionRoots) -> Result<T>,
) -> Result<T> {
    with_tracker_operations_coordination_lock_with_cancellation(ctx, &|| false, operation)
}

pub(crate) fn with_tracker_operations_coordination_lock_with_cancellation<T>(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
    operation: impl FnOnce(&PendingTrackerRetentionRoots) -> Result<T>,
) -> Result<T> {
    let path = ctx.state_file(TRACKER_OPERATIONS_FILE);
    with_jsonl_write_lock(&path, |guard| {
        let projection = tracker_operation_projection_locked(guard, &path, cancelled)?;
        operation(&projection.pending_retention_roots)
    })
}

fn tracker_operation_projection_locked(
    guard: &JsonlWriteGuard,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<TrackerOperationProjection> {
    let mut builder = ProjectionBuilder::default();
    let scan = scan_jsonl_raw_locked_bounded(guard, path, cancelled, MAX_RECORD_BYTES, |record| {
        builder.observe_raw(record, path)
    })?;
    ensure!(
        !scan.unterminated_final_record,
        "Refusing tracker operation authority because {} has an unterminated final record",
        path.display()
    );
    builder.finish()
}

#[derive(Default)]
struct ProjectionBuilder {
    events_by_id: BTreeMap<String, Value>,
    operations: BTreeMap<String, ProjectedTrackerOperation>,
}

impl ProjectionBuilder {
    fn observe_raw(&mut self, record: RawJsonlRecord<'_>, path: &Path) -> Result<()> {
        // An unterminated value is not committed. The completed scan reports it
        // and makes the entire authority projection fail closed.
        if !record.terminated {
            return Ok(());
        }
        ensure!(
            record.bytes.len() <= MAX_RECORD_BYTES,
            "tracker operation record {} in {} exceeds the {}-byte limit",
            record.line_number,
            path.display(),
            MAX_RECORD_BYTES
        );
        let raw = strict_json::from_slice(record.bytes).with_context(|| {
            format!(
                "Failed to parse tracker operation record {} in {}",
                record.line_number,
                path.display()
            )
        })?;
        self.observe_value(raw, Some(record.line_number), path)
    }

    fn observe_value(&mut self, raw: Value, line_number: Option<u64>, path: &Path) -> Result<()> {
        let location = line_number
            .map(|line| format!("record {line}"))
            .unwrap_or_else(|| "candidate record".to_string());
        let schema_version = raw
            .get("schema_version")
            .and_then(Value::as_u64)
            .with_context(|| {
                format!(
                    "Tracker operation {location} in {} has no integer schema_version",
                    path.display()
                )
            })?;
        ensure!(
            schema_version == u64::from(TRACKER_OPERATION_SCHEMA_VERSION),
            "Tracker operation {location} in {} uses unsupported schema version {}",
            path.display(),
            schema_version
        );
        let fact: TrackerOperationEventV1 =
            serde_json::from_value(raw.clone()).with_context(|| {
                format!(
                    "Failed to decode tracker operation {location} in {} as schema version {}",
                    path.display(),
                    TRACKER_OPERATION_SCHEMA_VERSION
                )
            })?;
        validate_fact(&fact).with_context(|| {
            format!("Invalid tracker operation {location} in {}", path.display())
        })?;

        if let Some(existing) = self.events_by_id.get(&fact.event_id) {
            ensure!(
                existing == &raw,
                "tracker operation event ID '{}' has conflicting semantic records",
                fact.event_id
            );
            return Ok(());
        }

        let operation = self
            .operations
            .entry(fact.operation_id.clone())
            .or_insert_with(|| ProjectedTrackerOperation {
                operation_id: fact.operation_id.clone(),
                plan_id: fact.plan_id.clone(),
                issue: fact.issue.clone(),
                kind: fact.kind,
                events: Vec::new(),
                terminal_outcome: None,
            });
        ensure!(
            operation.plan_id == fact.plan_id
                && operation.issue == fact.issue
                && operation.kind == fact.kind,
            "tracker operation '{}' has conflicting immutable identity",
            fact.operation_id
        );
        if fact.phase == TrackerOperationPhase::Acknowledgement {
            ensure!(
                operation.terminal_outcome.is_none() || operation.terminal_outcome == fact.outcome,
                "tracker operation '{}' has conflicting terminal outcomes",
                fact.operation_id
            );
            operation.terminal_outcome = fact.outcome;
        }
        operation.events.push(ProjectedTrackerOperationEvent {
            fact: fact.clone(),
            raw: raw.clone(),
        });
        self.events_by_id.insert(fact.event_id, raw);
        Ok(())
    }

    fn finish(mut self) -> Result<TrackerOperationProjection> {
        let mut pending_retention_roots = PendingTrackerRetentionRoots::default();
        for operation in self.operations.values_mut() {
            operation.events.sort_by(|left, right| {
                (left.fact.timestamp_ms, left.fact.event_id.as_str())
                    .cmp(&(right.fact.timestamp_ms, right.fact.event_id.as_str()))
            });
            operation.terminal_outcome = validate_merged_history(operation)?;
            if !operation.is_pending() {
                continue;
            }
            pending_retention_roots
                .plan_ids
                .insert(operation.plan_id.clone());
            for event in &operation.events {
                pending_retention_roots
                    .receipt_ids
                    .extend(event.fact.receipt_ids.iter().cloned());
                pending_retention_roots
                    .run_ids
                    .extend(event.fact.run_ids.iter().cloned());
            }
        }
        Ok(TrackerOperationProjection {
            operations: self.operations,
            pending_retention_roots,
        })
    }
}

#[cfg(test)]
mod tests;
