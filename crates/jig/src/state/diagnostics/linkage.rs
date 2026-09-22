//! Read-only run-linkage diagnosis.
//!
//! Receipts may reference a durable run through their top-level `run_id`, and
//! supported work-check batch evidence references child receipts and their
//! runs. This module joins those references to the active run journal and to
//! local run archives and state backups. It never reconciles a run, creates a
//! cache or lease, or rewrites a stream: a lifecycle that cannot be verified is
//! reported as uncertain rather than treated as healthy or as proof of loss.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

use super::{StreamDiagnostics, display_repo_path};
use crate::state::records::RunEventRecord;
use crate::state::runs::active_run_lease_ids;
use crate::state::runs::lifecycle::{RunLifecycleValidator, is_recognized_run_event};

mod receipt_sources;
mod references;
mod report;
mod sources;

use receipt_sources::scan_receipt_history_sources;
pub(super) use references::analyze_receipt_linkage;
use references::{BatchReference, batch_receipt_ids, collect_references};
use report::{RunJournalFacts, RunLinkageCounts, guidance};
pub(super) use report::{RunLinkageFinding, RunLinkageReport, recommendations};
use sources::{HistorySources, SourceKind, scan_local_history_sources};

const MAX_TRACKED_REFERENCES: usize = 250_000;
const MAX_FINDINGS: usize = 100;
const MAX_FINDING_IDS: usize = 50;
const MAX_ANOMALY_SAMPLES: usize = 5;
const RUN_LEASE_DIR: &str = ".agent/.cache/run-leases";

pub(super) const NOT_CHECKED_REASON: &str = "Run linkage is analyzed only with --deep; receipt run references were not compared with run history.";

/// Streams collect receipt run references and journal lifecycles here one
/// physical record at a time; nothing is resolved until every stream is read.
#[derive(Default)]
pub(super) struct RunLinkageCollector {
    receipt_ids: BTreeSet<String>,
    receipt_runs: BTreeMap<String, BTreeSet<String>>,
    conflicting_receipt_runs: BTreeSet<String>,
    batches: Vec<BatchReference>,
    receipts_with_run_id: u64,
    batch_receipts: u64,
    batch_links: u64,
    tracked_references: usize,
    reference_budget_exceeded: bool,
    tracked_lifecycles: usize,
    lifecycle_budget_exceeded: bool,
    journal: BTreeMap<String, LifecycleObservation>,
    journal_events: u64,
    journal_unrecognized_events: u64,
    journal_unrecognized_records: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecycleStatus {
    /// At least one event kind is not recognized; not a verified lifecycle.
    Unrecognized,
    Inconsistent,
    Active,
    Completed,
}

#[derive(Debug, Default)]
pub(super) struct LifecycleObservation {
    validator: RunLifecycleValidator,
    known_events: u64,
    unknown_events: u64,
    anomalies: Vec<String>,
    anomaly_count: u64,
}

impl LifecycleObservation {
    pub(super) fn observe(&mut self, event: &RunEventRecord) {
        if !is_recognized_run_event(&event.event) {
            self.unknown_events += 1;
            return;
        }
        self.known_events += 1;
        if let Err(error) = self.validator.observe(event) {
            self.anomaly(error.to_string());
        }
    }

    fn anomaly(&mut self, message: String) {
        self.anomaly_count += 1;
        if self.anomalies.len() < MAX_ANOMALY_SAMPLES {
            self.anomalies.push(message);
        }
    }

    fn status(&self) -> LifecycleStatus {
        if self.unknown_events > 0 {
            LifecycleStatus::Unrecognized
        } else if !self.validator.queued() || self.anomaly_count > 0 {
            LifecycleStatus::Inconsistent
        } else if self.validator.completed() {
            LifecycleStatus::Completed
        } else {
            LifecycleStatus::Active
        }
    }

    fn events(&self) -> u64 {
        self.known_events.saturating_add(self.unknown_events)
    }
}

impl RunLinkageCollector {
    fn track_references(&mut self, references: usize) -> bool {
        if self.reference_budget_exceeded
            || self
                .tracked_references
                .checked_add(references)
                .is_none_or(|total| total > MAX_TRACKED_REFERENCES)
        {
            self.reference_budget_exceeded = true;
            return false;
        }
        self.tracked_references += references;
        true
    }

    fn track_lifecycle(&mut self) -> bool {
        if self.lifecycle_budget_exceeded
            || self
                .tracked_lifecycles
                .checked_add(1)
                .is_none_or(|total| total > MAX_TRACKED_REFERENCES)
        {
            self.lifecycle_budget_exceeded = true;
            return false;
        }
        self.tracked_lifecycles += 1;
        true
    }

    pub(super) fn observe_run_event(&mut self, record: &[u8]) {
        let Ok(event) = serde_json::from_slice::<RunEventRecord>(record) else {
            self.journal_unrecognized_records += 1;
            return;
        };
        self.journal_events += 1;
        if !self.journal.contains_key(&event.run_id) && !self.track_lifecycle() {
            return;
        }
        let lifecycle = self.journal.entry(event.run_id.clone()).or_default();
        let unknown_before = lifecycle.unknown_events;
        lifecycle.observe(&event);
        if lifecycle.unknown_events > unknown_before {
            self.journal_unrecognized_events += 1;
        }
    }
}

fn journal_facts(
    root: &Path,
    collector: &RunLinkageCollector,
    runs: Option<&StreamDiagnostics>,
) -> RunJournalFacts {
    let inconsistent_lifecycles = collector
        .journal
        .values()
        .filter(|lifecycle| lifecycle.status() == LifecycleStatus::Inconsistent)
        .count() as u64;
    let mut facts = RunJournalFacts {
        path: runs.map_or_else(
            || display_repo_path(root, &root.join(".agent/state/runs.jsonl")),
            |stream| stream.path.clone(),
        ),
        events: collector.journal_events,
        lifecycles: collector.journal.len() as u64,
        unrecognized_events: collector.journal_unrecognized_events,
        unrecognized_records: collector.journal_unrecognized_records,
        inconsistent_lifecycles,
        ..RunJournalFacts::default()
    };
    if let Some(stream) = runs {
        facts.exists = stream.exists;
        facts.malformed_records = stream.malformed_records;
        facts.torn_tail = stream.torn_tail;
        facts.scan_error.clone_from(&stream.scan_error);
    }
    facts.authority = if facts.scan_error.is_some() {
        "unreadable"
    } else if !facts.exists {
        "absent"
    } else if facts.malformed_records > 0
        || facts.torn_tail
        || facts.unrecognized_records > 0
        || facts.unrecognized_events > 0
        || facts.inconsistent_lifecycles > 0
    {
        "damaged"
    } else {
        "verified"
    }
    .into();
    facts
}

fn incomplete_reasons(
    collector: &RunLinkageCollector,
    receipts: Option<&StreamDiagnostics>,
    journal: &RunJournalFacts,
) -> Vec<String> {
    let mut reasons = Vec::new();
    match receipts {
        Some(stream) => {
            if let Some(error) = &stream.scan_error {
                reasons.push(format!("receipt stream scan failed: {error}"));
            }
            if stream.malformed_records > 0 {
                reasons.push(format!(
                    "receipt stream contains {} malformed record(s)",
                    stream.malformed_records
                ));
            }
            if stream.torn_tail {
                reasons.push("receipt stream has a torn final record".into());
            }
            if stream.deep_analysis_error_count > 0 {
                reasons.push(format!(
                    "{} receipt records could not be analyzed for run references",
                    stream.deep_analysis_error_count
                ));
            }
        }
        None => reasons.push("receipt stream was not scanned".into()),
    }
    if !collector.conflicting_receipt_runs.is_empty() {
        reasons.push(format!(
            "{} receipt ID(s) reference conflicting runs",
            collector.conflicting_receipt_runs.len()
        ));
    }
    if let Some(error) = &journal.scan_error {
        reasons.push(format!("run journal scan failed: {error}"));
    }
    if journal.malformed_records > 0 {
        reasons.push(format!(
            "run journal contains {} malformed record(s)",
            journal.malformed_records
        ));
    }
    if journal.torn_tail {
        reasons.push("run journal has a torn final record".into());
    }
    if journal.unrecognized_records > 0 {
        reasons.push(format!(
            "run journal contains {} structurally unrecognized record(s)",
            journal.unrecognized_records
        ));
    }
    if journal.unrecognized_events > 0 {
        reasons.push(format!(
            "run journal contains {} unrecognized event(s)",
            journal.unrecognized_events
        ));
    }
    if journal.inconsistent_lifecycles > 0 {
        reasons.push(format!(
            "run journal contains {} lifecycle(s) rejected by authoritative validation",
            journal.inconsistent_lifecycles
        ));
    }
    if collector.reference_budget_exceeded {
        reasons.push(format!(
            "more than {MAX_TRACKED_REFERENCES} receipt identities and batch-child references exist; later references were not tracked"
        ));
    }
    if collector.lifecycle_budget_exceeded {
        reasons.push(format!(
            "more than {MAX_TRACKED_REFERENCES} run lifecycles exist; later journal lifecycles were not observed"
        ));
    }
    reasons
}

struct Classification {
    status: &'static str,
    detail: String,
    history_sources: Vec<Value>,
    recovery: Option<Value>,
}

fn classify_missing_run(
    run_id: &str,
    journal: &RunJournalFacts,
    sources: &HistorySources,
    lifecycle_budget_exceeded: bool,
    linkage_complete: bool,
    current_nonterminal_runs: u64,
    active_worker_lease_ids: &[String],
) -> Option<Classification> {
    let found = sources.found.get(run_id);
    let history_sources = found
        .map(|entries| {
            entries
                .iter()
                .map(sources::SourceLifecycle::to_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if lifecycle_budget_exceeded {
        return Some(Classification {
            status: "unverifiable",
            detail: "No tracked active-journal lifecycle was available for this run, but the lifecycle observation budget was exhausted. The run may be present in the unobserved portion of the journal, so it cannot be classified as missing.".into(),
            history_sources,
            recovery: None,
        });
    }
    let archived_complete = found.is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry.kind == SourceKind::Archive
                && entry.observation.status() == LifecycleStatus::Completed
        })
    });
    if archived_complete {
        return None;
    }
    if matches!(journal.authority.as_str(), "damaged" | "unreadable") {
        return Some(Classification {
            status: "unverifiable",
            detail: format!(
                "No lifecycle for this run was found, but the active run journal is {} ({} malformed records, {} unrecognized events, {} inconsistent lifecycles, torn tail: {}), so its events may be unreadable rather than absent. Back up and repair the journal before drawing an integrity conclusion.",
                journal.authority,
                journal.malformed_records,
                journal.unrecognized_events,
                journal.inconsistent_lifecycles,
                journal.torn_tail
            ),
            history_sources,
            recovery: None,
        });
    }
    let unverifiable_sources = sources.error_count.saturating_add(sources.symlinks_skipped);
    if unverifiable_sources > 0 {
        return Some(Classification {
            status: "unverifiable",
            detail: format!(
                "No lifecycle for this run was found in the active run journal, but {} local run history source(s) could not be verified. Archived history or a safe recovery source therefore cannot be confirmed.",
                unverifiable_sources
            ),
            history_sources,
            recovery: None,
        });
    }
    if sources.budget_exhausted {
        return Some(Classification {
            status: "unverifiable",
            detail: "No lifecycle for this run was found before aggregate history scanning exhausted its decompression budget, so archived history or a safe recovery source cannot be confirmed.".into(),
            history_sources,
            recovery: None,
        });
    }
    if let Some(backup) = found.and_then(|entries| {
        entries
            .iter()
            .filter(|entry| {
                entry.kind == SourceKind::Backup
                    && entry.observation.status() == LifecycleStatus::Completed
            })
            .max_by(|left, right| {
                let left_created = left.backup.as_ref().map_or(0, |facts| facts.created_at_ms);
                let right_created = right.backup.as_ref().map_or(0, |facts| facts.created_at_ms);
                left_created
                    .cmp(&right_created)
                    .then_with(|| left.path.cmp(&right.path))
            })
    }) {
        if !linkage_complete {
            return Some(Classification {
                status: "unverifiable",
                detail: "An exact backup contains this run, but the linkage scan is incomplete, so backup availability cannot be presented as verified recovery guidance until the reported scan problems are resolved.".into(),
                history_sources,
                recovery: None,
            });
        }
        let mut recovery = backup.recovery_value();
        if let Value::Object(fields) = &mut recovery {
            let mut restore_blockers = Vec::new();
            if current_nonterminal_runs > 0 {
                restore_blockers.push("nonterminal_runs");
            }
            if !active_worker_lease_ids.is_empty() {
                restore_blockers.push("active_worker_leases");
            }
            fields.insert(
                "restore_eligibility".into(),
                Value::String(
                    if restore_blockers.is_empty() {
                        "manual_preflight_required"
                    } else {
                        "blocked_destination_activity"
                    }
                    .into(),
                ),
            );
            fields.insert("restore_command_available".into(), Value::Bool(false));
            fields.insert("restore_blockers".into(), json!(restore_blockers));
            fields.insert("current_journal_events".into(), json!(journal.events));
            fields.insert(
                "current_journal_lifecycles".into(),
                json!(journal.lifecycles),
            );
            fields.insert(
                "current_nonterminal_runs".into(),
                json!(current_nonterminal_runs),
            );
            fields.insert(
                "current_active_worker_leases".into(),
                json!(active_worker_lease_ids.len()),
            );
            fields.insert(
                "current_active_worker_lease_ids".into(),
                json!(
                    active_worker_lease_ids
                        .iter()
                        .take(MAX_FINDING_IDS)
                        .collect::<Vec<_>>()
                ),
            );
            fields.insert(
                "current_active_worker_lease_ids_truncated".into(),
                Value::Bool(active_worker_lease_ids.len() > MAX_FINDING_IDS),
            );
            fields.insert(
                "current_journal_comparison_required".into(),
                Value::Bool(journal.events > 0),
            );
        }
        return Some(Classification {
            status: "recoverable_from_backup",
            detail: format!(
                "The active run journal has no lifecycle for this run, but the exact state backup {} contains {} events for it. This proves source availability, not restore eligibility. The current journal contains {} event(s) across {} lifecycle(s), including {} nonterminal run(s), and {} active worker lease(s) currently block stream replacement; compare both journals and complete restore preflight before any manual restore, because restoring replaces the entire stream.",
                backup.path,
                backup.observation.events(),
                journal.events,
                journal.lifecycles,
                current_nonterminal_runs,
                active_worker_lease_ids.len()
            ),
            history_sources,
            recovery: Some(recovery),
        });
    }
    if let Some(partial) = found.and_then(|entries| entries.first()) {
        return Some(Classification {
            status: "unverifiable",
            detail: format!(
                "The active run journal has no lifecycle for this run. {} contains {} events for it, but they do not form a verified complete lifecycle ({}). Treat this history as uncertain rather than healthy or lost.",
                partial.path,
                partial.observation.events(),
                partial
                    .observation
                    .anomalies
                    .first()
                    .map_or_else(|| "nonterminal".to_string(), Clone::clone)
            ),
            history_sources,
            recovery: None,
        });
    }
    Some(Classification {
        status: "missing",
        detail: format!(
            "No lifecycle events for this run exist in the active run journal, and none of the {} run archive(s) and {} run backup(s) in this checkout contain them. Absence here does not prove the events were deleted; another checkout, archive, or backup may still hold them, and receipts alone cannot recover event identities or order.",
            sources.archives_scanned, sources.backups_scanned
        ),
        history_sources,
        recovery: None,
    })
}

fn classify_journal_run(lifecycle: &LifecycleObservation) -> Option<Classification> {
    match lifecycle.status() {
        LifecycleStatus::Active | LifecycleStatus::Completed => None,
        LifecycleStatus::Unrecognized => Some(Classification {
            status: "unverifiable",
            detail: format!(
                "The active run journal contains {} unrecognized event(s) among {} record(s) for this run, so the complete lifecycle cannot be verified by this runtime.",
                lifecycle.unknown_events,
                lifecycle.events()
            ),
            history_sources: Vec::new(),
            recovery: None,
        }),
        LifecycleStatus::Inconsistent => Some(Classification {
            status: "inconsistent",
            detail: format!(
                "The active run journal contains events for this run, but they do not form a consistent lifecycle ({}). Do not treat this history as verified or as proof of deletion.",
                lifecycle
                    .anomalies
                    .first()
                    .map_or("no queued event", String::as_str)
            ),
            history_sources: Vec::new(),
            recovery: None,
        }),
    }
}

fn lease_file_present(root: &Path, run_id: &str) -> Option<bool> {
    let safe = !run_id.is_empty()
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    safe.then(|| {
        root.join(RUN_LEASE_DIR)
            .join(format!("{run_id}.lock"))
            .is_file()
    })
}

fn capped_ids(ids: &BTreeSet<String>) -> (Vec<String>, u64, bool) {
    (
        ids.iter().take(MAX_FINDING_IDS).cloned().collect(),
        ids.len() as u64,
        ids.len() > MAX_FINDING_IDS,
    )
}

fn count_status(counts: &mut RunLinkageCounts, status: &str) {
    match status {
        "missing" => counts.missing += 1,
        "unverifiable" => counts.unverifiable += 1,
        "inconsistent" => counts.inconsistent += 1,
        "recoverable_from_backup" => counts.recoverable_from_backup += 1,
        _ => {}
    }
}

/// Joins collected references to lifecycle history after every stream scan.
pub(super) fn resolve(
    root: &Path,
    mut collector: RunLinkageCollector,
    receipts: Option<&StreamDiagnostics>,
    runs: Option<&StreamDiagnostics>,
) -> RunLinkageReport {
    let journal = journal_facts(root, &collector, runs);
    let wanted_receipt_ids = batch_receipt_ids(&collector);
    let remaining_references = MAX_TRACKED_REFERENCES.saturating_sub(collector.tracked_references);
    let receipt_history = scan_receipt_history_sources(
        root,
        &wanted_receipt_ids,
        &collector.receipt_ids,
        &collector.receipt_runs,
        remaining_references,
    );
    receipt_history.merge_into(&mut collector);
    let mut incomplete_reasons = incomplete_reasons(&collector, receipts, &journal);
    if receipt_history.error_count > 0 {
        incomplete_reasons.push(format!(
            "{} local receipt history source(s) could not be fully verified",
            receipt_history.error_count
        ));
    }
    if receipt_history.symlinks_skipped > 0 {
        incomplete_reasons.push(format!(
            "{} symlinked local receipt history candidate(s) were skipped and could not be verified",
            receipt_history.symlinks_skipped
        ));
    }
    if receipt_history.budget_exhausted {
        incomplete_reasons
            .push("local receipt history scan exhausted its aggregate decompression budget".into());
    }
    if receipt_history.reference_budget_exhausted {
        incomplete_reasons
            .push("local receipt history scan exhausted the linkage reference budget".into());
    }
    let collected_references = collect_references(&collector);
    if collected_references.unresolved_batch_links > 0 {
        incomplete_reasons.push(format!(
            "{} supported batch child receipt link(s) reference missing receipt identities",
            collected_references.unresolved_batch_links
        ));
    }
    if collected_references.conflicting_batch_links > 0 {
        incomplete_reasons.push(format!(
            "{} supported batch child link(s) carry run IDs that conflict with their receipt histories",
            collected_references.conflicting_batch_links
        ));
    }
    let references = collected_references.runs;
    let missing = references
        .keys()
        .filter(|run_id| !collector.journal.contains_key(*run_id))
        .cloned()
        .collect::<BTreeSet<_>>();
    let sources = if missing.is_empty() {
        HistorySources::default()
    } else {
        scan_local_history_sources(root, &missing)
    };
    if sources.error_count > 0 {
        incomplete_reasons.push(format!(
            "{} local run history source(s) could not be fully verified",
            sources.error_count
        ));
    }
    if sources.symlinks_skipped > 0 {
        incomplete_reasons.push(format!(
            "{} symlinked local run history candidate(s) were skipped and could not be verified",
            sources.symlinks_skipped
        ));
    }
    if sources.budget_exhausted {
        incomplete_reasons
            .push("local run history scan exhausted its aggregate decompression budget".into());
    }
    let has_verified_backup = sources.found.values().flatten().any(|entry| {
        entry.kind == SourceKind::Backup && entry.observation.status() == LifecycleStatus::Completed
    });
    let active_worker_lease_ids = if has_verified_backup {
        match active_run_lease_ids(root, std::iter::empty()) {
            Ok(run_ids) => run_ids,
            Err(error) => {
                incomplete_reasons.push(format!(
                    "active worker lease preflight could not be completed: {error:#}"
                ));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let current_nonterminal_runs = collector
        .journal
        .values()
        .filter(|lifecycle| lifecycle.status() == LifecycleStatus::Active)
        .count() as u64;
    let linkage_complete = incomplete_reasons.is_empty();

    let mut counts = RunLinkageCounts::default();
    let mut findings = Vec::new();
    let mut finding_count = 0u64;
    for (run_id, reference) in &references {
        let (classification, journal_events, anomalies) = match collector.journal.get(run_id) {
            Some(lifecycle) => (
                classify_journal_run(lifecycle),
                lifecycle.events(),
                lifecycle.anomalies.clone(),
            ),
            None => (
                classify_missing_run(
                    run_id,
                    &journal,
                    &sources,
                    collector.lifecycle_budget_exceeded,
                    linkage_complete,
                    current_nonterminal_runs,
                    &active_worker_lease_ids,
                ),
                0,
                Vec::new(),
            ),
        };
        let Some(classification) = classification else {
            match collector
                .journal
                .get(run_id)
                .map(LifecycleObservation::status)
            {
                Some(LifecycleStatus::Completed) => counts.completed += 1,
                Some(_) => counts.active += 1,
                None => counts.archived_verified += 1,
            }
            continue;
        };
        count_status(&mut counts, classification.status);
        finding_count += 1;
        if findings.len() >= MAX_FINDINGS {
            continue;
        }
        let (receipt_ids, receipt_count, receipt_ids_truncated) =
            capped_ids(&reference.receipt_ids);
        let (batch_receipt_ids, batch_receipt_count, batch_receipt_ids_truncated) =
            capped_ids(&reference.batch_receipt_ids);
        findings.push(RunLinkageFinding {
            run_id: run_id.clone(),
            status: classification.status.into(),
            detail: classification.detail,
            receipt_ids,
            receipt_count,
            receipt_ids_truncated,
            batch_receipt_ids,
            batch_receipt_count,
            batch_receipt_ids_truncated,
            journal_events,
            journal_anomalies: anomalies,
            lease_file_present: lease_file_present(root, run_id),
            history_sources: classification.history_sources,
            recovery: classification.recovery,
        });
    }

    let complete = incomplete_reasons.is_empty();
    let verdict = if finding_count > 0 {
        "findings"
    } else if !complete {
        "incomplete"
    } else {
        "clean"
    };
    RunLinkageReport {
        checked: true,
        verdict: verdict.into(),
        reason: None,
        complete,
        incomplete_reasons,
        receipts_with_run_id: collector.receipts_with_run_id,
        batch_receipts: collector.batch_receipts,
        batch_links: collector.batch_links,
        unresolved_batch_links: collected_references.unresolved_batch_links,
        conflicting_batch_links: collected_references.conflicting_batch_links,
        referenced_runs: references.len() as u64,
        tracked_references: collector.tracked_references as u64,
        reference_budget_exceeded: collector.reference_budget_exceeded,
        tracked_lifecycles: collector.tracked_lifecycles as u64,
        lifecycle_budget_exceeded: collector.lifecycle_budget_exceeded,
        runs: counts,
        journal,
        sources,
        receipt_history,
        findings_truncated: finding_count as usize > findings.len(),
        findings,
        finding_count,
        guidance: (finding_count > 0).then(guidance),
    }
}

#[cfg(test)]
mod tests;
