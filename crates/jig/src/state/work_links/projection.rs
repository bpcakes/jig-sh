use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::jsonl::{
    JsonlScanStats, JsonlWriteGuard, RawJsonlRecord, scan_jsonl_raw_bounded,
    scan_jsonl_raw_locked_bounded,
};
use super::{
    MAX_JOURNAL_DIAGNOSTIC_SAMPLES, MAX_WORK_LINK_RECORD_BYTES, PROVIDER_BEADS, SupportedWorkLink,
    WORK_LINK_SCHEMA_VERSION, WorkLinkDiagnostic, WorkLinkJournalAuthority,
    WorkLinkJournalDiagnosticSample, WorkLinkJournalDiagnostics, WorkLinkProjection,
    WorkLinkRecordV1, validate_event_id,
};

const EVENT_SEMANTICS_DOMAIN: &[u8] = b"jig-work-link-event-semantics-v1\0";
const LINK_IDENTITY_DOMAIN: &[u8] = b"jig-work-link-identity-v1\0";
pub(super) const MAX_WORK_LINK_UNIQUE_EVENTS: usize = 100_000;
pub(super) const MAX_WORK_LINK_KNOWN_PLANS: usize = 100_000;

#[derive(Clone, Copy)]
struct ProjectionLimits {
    unique_events: usize,
    known_plans: usize,
}

const PRODUCTION_LIMITS: ProjectionLimits = ProjectionLimits {
    unique_events: MAX_WORK_LINK_UNIQUE_EVENTS,
    known_plans: MAX_WORK_LINK_KNOWN_PLANS,
};

#[derive(Debug)]
pub(super) struct WorkLinkProjectionLimit {
    dimension: &'static str,
    limit: usize,
}

impl std::fmt::Display for WorkLinkProjectionLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "work-link journal exceeds the supported {} limit of {}",
            self.dimension, self.limit
        )
    }
}

impl std::error::Error for WorkLinkProjectionLimit {}

#[derive(Clone, Debug)]
struct SeenEvent {
    semantic_digest: [u8; 32],
    line_number: u64,
    plan_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct DiagnosticSummary {
    count: u64,
    sample: Option<Box<WorkLinkDiagnostic>>,
}

impl DiagnosticSummary {
    fn push(&mut self, mut detail: WorkLinkDiagnostic) {
        self.count = self.count.saturating_add(1);
        if self.sample.is_none() {
            detail.message = super::super::support::truncate(&detail.message);
            self.sample = Some(Box::new(detail));
        }
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn samples(&self) -> Vec<WorkLinkDiagnostic> {
        self.sample
            .iter()
            .map(|sample| sample.as_ref().clone())
            .collect()
    }
}

struct SelectedPlanState {
    canonical_record: WorkLinkRecordV1,
    event_ids: Vec<String>,
}

#[derive(Default)]
struct PlanState {
    link_identity_digest: Option<[u8; 32]>,
    selected: Option<Box<SelectedPlanState>>,
    replayed_records: u64,
    conflicts: DiagnosticSummary,
    unsupported: DiagnosticSummary,
    corruption: DiagnosticSummary,
}

pub(super) struct JournalProjection {
    selected_plan: Option<String>,
    limits: ProjectionLimits,
    seen_events: BTreeMap<String, SeenEvent>,
    plans: BTreeMap<String, PlanState>,
    supported_records: u64,
    replayed_records: u64,
    global_conflicts: DiagnosticSummary,
    global_corruption: DiagnosticSummary,
    torn_tail: bool,
}

impl JournalProjection {
    fn new(selected_plan: Option<&str>, limits: ProjectionLimits) -> Self {
        Self {
            selected_plan: selected_plan.map(str::to_owned),
            limits,
            seen_events: BTreeMap::new(),
            plans: BTreeMap::new(),
            supported_records: 0,
            replayed_records: 0,
            global_conflicts: DiagnosticSummary::default(),
            global_corruption: DiagnosticSummary::default(),
            torn_tail: false,
        }
    }

    fn observe(&mut self, raw: RawJsonlRecord<'_>) -> Result<()> {
        if !raw.terminated {
            // Strict readers never give an unterminated record authority. The
            // scan statistics add one journal-level diagnostic after the scan.
            return Ok(());
        }
        if raw.bytes.len() > MAX_WORK_LINK_RECORD_BYTES {
            self.global_corruption.push(diagnostic(
                Some(raw.line_number),
                None,
                format!("work-link record exceeds the {MAX_WORK_LINK_RECORD_BYTES}-byte limit"),
            ));
            return Ok(());
        }

        let value = match crate::strict_json::from_slice(raw.bytes) {
            Ok(value) => value,
            Err(error) => {
                self.global_corruption.push(diagnostic(
                    Some(raw.line_number),
                    None,
                    format!("invalid strict JSON: {error}"),
                ));
                return Ok(());
            }
        };
        let plan_id = valid_plan_id_from_value(&value);
        let event_id = valid_event_id_from_value(&value);
        let Some(event_id) = event_id else {
            self.push_corrupt(
                plan_id.as_deref(),
                diagnostic(
                    Some(raw.line_number),
                    None,
                    "missing or invalid work-link event id",
                ),
            )?;
            return Ok(());
        };

        if let Some(plan_id) = plan_id.as_deref() {
            self.ensure_plan(plan_id)?;
        }
        let semantic_digest = event_semantics_digest(&value)?;
        if let Some(previous) = self.seen_events.get(&event_id).cloned() {
            if previous.semantic_digest == semantic_digest {
                self.replayed_records = self.replayed_records.saturating_add(1);
                if let Some(plan_id) = previous.plan_id {
                    let state = self.plan_mut(&plan_id)?;
                    state.replayed_records = state.replayed_records.saturating_add(1);
                }
                return Ok(());
            }
            let detail = diagnostic(
                Some(raw.line_number),
                Some(event_id.clone()),
                format!(
                    "event id has different complete JSON semantics than line {}",
                    previous.line_number
                ),
            );
            match (&previous.plan_id, &plan_id) {
                (Some(left), Some(right)) => {
                    self.plan_mut(left)?.conflicts.push(detail.clone());
                    if right != left {
                        self.plan_mut(right)?.conflicts.push(detail);
                    }
                }
                (Some(plan_id), None) | (None, Some(plan_id)) => {
                    self.plan_mut(plan_id)?.conflicts.push(detail.clone());
                    self.global_conflicts.push(detail);
                }
                (None, None) => self.global_conflicts.push(detail),
            }
            return Ok(());
        }
        if self.seen_events.len() >= self.limits.unique_events {
            return Err(WorkLinkProjectionLimit {
                dimension: "unique-event",
                limit: self.limits.unique_events,
            }
            .into());
        }
        self.seen_events.insert(
            event_id.clone(),
            SeenEvent {
                semantic_digest,
                line_number: raw.line_number,
                plan_id: plan_id.clone(),
            },
        );

        let Some(plan_id) = plan_id else {
            self.global_corruption.push(diagnostic(
                Some(raw.line_number),
                Some(event_id),
                "missing or invalid plan_id",
            ));
            return Ok(());
        };
        let schema_version = value.get("schema_version").and_then(Value::as_u64);
        match schema_version {
            Some(version) if version > u64::from(WORK_LINK_SCHEMA_VERSION) => {
                self.plan_mut(&plan_id)?.unsupported.push(diagnostic(
                    Some(raw.line_number),
                    Some(event_id),
                    format!("unsupported work-link schema version {version}"),
                ));
                return Ok(());
            }
            Some(version) if version == u64::from(WORK_LINK_SCHEMA_VERSION) => {}
            _ => {
                self.push_corrupt(
                    Some(&plan_id),
                    diagnostic(
                        Some(raw.line_number),
                        Some(event_id),
                        "missing or invalid work-link schema_version",
                    ),
                )?;
                return Ok(());
            }
        }

        match value
            .get("issue")
            .and_then(|issue| issue.get("provider"))
            .and_then(Value::as_str)
        {
            Some(PROVIDER_BEADS) => {}
            Some(provider) => {
                self.plan_mut(&plan_id)?.unsupported.push(diagnostic(
                    Some(raw.line_number),
                    Some(event_id),
                    format!("unsupported work-link provider {provider:?}"),
                ));
                return Ok(());
            }
            None => {
                self.push_corrupt(
                    Some(&plan_id),
                    diagnostic(
                        Some(raw.line_number),
                        Some(event_id),
                        "missing or invalid work-link provider",
                    ),
                )?;
                return Ok(());
            }
        }

        let record: WorkLinkRecordV1 = match serde_json::from_value(value) {
            Ok(record) => record,
            Err(error) => {
                self.push_corrupt(
                    Some(&plan_id),
                    diagnostic(
                        Some(raw.line_number),
                        Some(event_id),
                        format!("invalid work-link v1 record: {error}"),
                    ),
                )?;
                return Ok(());
            }
        };
        if let Err(error) = record.validate() {
            self.push_corrupt(
                Some(&plan_id),
                diagnostic(
                    Some(raw.line_number),
                    Some(record.id),
                    format!("invalid work-link v1 record: {error:#}"),
                ),
            )?;
            return Ok(());
        }

        self.supported_records = self.supported_records.saturating_add(1);
        let identity_digest = link_identity_digest(&record);
        let selected = self.selected_plan.as_deref() == Some(plan_id.as_str());
        let state = self.plan_mut(&plan_id)?;
        if let Some(previous) = state.link_identity_digest {
            if previous != identity_digest {
                state.conflicts.push(diagnostic(
                    Some(raw.line_number),
                    Some(record.id.clone()),
                    format!("plan {plan_id} has multiple distinct immutable work links"),
                ));
            }
        } else {
            state.link_identity_digest = Some(identity_digest);
        }
        if selected {
            if let Some(selected) = state.selected.as_mut() {
                selected.event_ids.push(record.id.clone());
                if record.id < selected.canonical_record.id {
                    selected.canonical_record = record;
                }
            } else {
                state.selected = Some(Box::new(SelectedPlanState {
                    event_ids: vec![record.id.clone()],
                    canonical_record: record,
                }));
            }
        }
        Ok(())
    }

    fn ensure_plan(&mut self, plan_id: &str) -> Result<()> {
        if !self.plans.contains_key(plan_id) {
            if self.plans.len() >= self.limits.known_plans {
                return Err(WorkLinkProjectionLimit {
                    dimension: "known-plan",
                    limit: self.limits.known_plans,
                }
                .into());
            }
            self.plans.insert(plan_id.to_owned(), PlanState::default());
        }
        Ok(())
    }

    fn plan_mut(&mut self, plan_id: &str) -> Result<&mut PlanState> {
        self.ensure_plan(plan_id)?;
        Ok(self
            .plans
            .get_mut(plan_id)
            .expect("ensured work-link plan state exists"))
    }

    fn finish(&mut self, stats: JsonlScanStats) {
        if stats.unterminated_final_record {
            self.torn_tail = true;
            self.global_corruption.push(diagnostic(
                stats.max_line_number,
                None,
                "final work-link JSONL record is not newline-terminated",
            ));
        }
    }

    fn push_corrupt(&mut self, plan_id: Option<&str>, detail: WorkLinkDiagnostic) -> Result<()> {
        if let Some(plan_id) = plan_id {
            self.plan_mut(plan_id)?.corruption.push(detail);
        } else {
            self.global_corruption.push(detail);
        }
        Ok(())
    }

    pub(super) fn ensure_authoritative_write_safe(&self) -> Result<()> {
        let diagnostics = self.diagnostics();
        if matches!(
            diagnostics.authority,
            WorkLinkJournalAuthority::Empty | WorkLinkJournalAuthority::Supported
        ) {
            return Ok(());
        }
        let detail = diagnostics
            .errors
            .first()
            .map(|sample| sample.message.as_str())
            .unwrap_or("no diagnostic details");
        bail!(
            "Refusing authoritative work-link write because journal authority is {:?}: {detail}",
            diagnostics.authority
        )
    }

    pub(super) fn for_plan(&self, plan_id: &str) -> WorkLinkProjection {
        if !self.global_corruption.is_empty() {
            return WorkLinkProjection::Corrupt(self.global_corruption.samples());
        }
        let Some(state) = self.plans.get(plan_id) else {
            if !self.global_conflicts.is_empty() {
                return WorkLinkProjection::Conflict(self.global_conflicts.samples());
            }
            return WorkLinkProjection::Unlinked;
        };
        if !state.corruption.is_empty() {
            return WorkLinkProjection::Corrupt(state.corruption.samples());
        }
        if !self.global_conflicts.is_empty() || !state.conflicts.is_empty() {
            let mut conflicts = self.global_conflicts.samples();
            conflicts.extend(state.conflicts.samples());
            return WorkLinkProjection::Conflict(conflicts);
        }
        if !state.unsupported.is_empty() {
            return WorkLinkProjection::Unsupported(state.unsupported.samples());
        }
        if state.link_identity_digest.is_none() {
            return WorkLinkProjection::Unlinked;
        }
        let Some(selected) = state.selected.as_ref() else {
            return WorkLinkProjection::Corrupt(vec![diagnostic(
                None,
                None,
                "selected work-link projection did not retain its canonical record",
            )]);
        };
        let mut event_ids = selected.event_ids.clone();
        event_ids.sort();
        WorkLinkProjection::Supported(Box::new(SupportedWorkLink {
            record: selected.canonical_record.clone(),
            event_ids,
            replayed_records: state.replayed_records,
        }))
    }

    pub(super) fn diagnostics(&self) -> WorkLinkJournalDiagnostics {
        let mut report = WorkLinkJournalDiagnostics {
            known_plans: self.plans.len() as u64,
            supported_records: self.supported_records,
            replayed_records: self.replayed_records,
            torn_tail: self.torn_tail,
            ..WorkLinkJournalDiagnostics::default()
        };
        if !self.global_corruption.is_empty() {
            report.corrupt_plans = report.known_plans;
            append_journal_diagnostics(&mut report, None, &self.global_corruption);
        } else if !self.global_conflicts.is_empty() {
            report.conflicting_plans = report.known_plans;
            append_journal_diagnostics(&mut report, None, &self.global_conflicts);
        } else {
            for (plan_id, state) in &self.plans {
                if !state.corruption.is_empty() {
                    report.corrupt_plans += 1;
                    append_journal_diagnostics(
                        &mut report,
                        Some(plan_id.as_str()),
                        &state.corruption,
                    );
                } else if !state.conflicts.is_empty() {
                    report.conflicting_plans += 1;
                    append_journal_diagnostics(
                        &mut report,
                        Some(plan_id.as_str()),
                        &state.conflicts,
                    );
                } else if !state.unsupported.is_empty() {
                    report.unsupported_plans += 1;
                    append_journal_diagnostics(
                        &mut report,
                        Some(plan_id.as_str()),
                        &state.unsupported,
                    );
                } else if state.link_identity_digest.is_some() {
                    report.supported_plans += 1;
                }
            }
        }
        report.authority = if self.torn_tail {
            WorkLinkJournalAuthority::Torn
        } else if !self.global_corruption.is_empty() || report.corrupt_plans > 0 {
            WorkLinkJournalAuthority::Corrupt
        } else if !self.global_conflicts.is_empty() || report.conflicting_plans > 0 {
            WorkLinkJournalAuthority::Conflicting
        } else if report.unsupported_plans > 0 {
            WorkLinkJournalAuthority::Unsupported
        } else if report.supported_plans > 0 {
            WorkLinkJournalAuthority::Supported
        } else {
            WorkLinkJournalAuthority::Empty
        };
        report.errors_truncated = report.error_count as usize > report.errors.len();
        report
    }
}

fn append_journal_diagnostics(
    report: &mut WorkLinkJournalDiagnostics,
    plan_id: Option<&str>,
    diagnostics: &DiagnosticSummary,
) {
    report.error_count = report.error_count.saturating_add(diagnostics.count);
    if report.errors.len() >= MAX_JOURNAL_DIAGNOSTIC_SAMPLES {
        return;
    }
    if let Some(diagnostic) = diagnostics.sample.as_deref() {
        report.errors.push(WorkLinkJournalDiagnosticSample {
            plan_id: plan_id.map(str::to_owned),
            line_number: diagnostic.line_number,
            event_id: diagnostic.event_id.clone(),
            message: super::super::support::truncate(&diagnostic.message),
        });
    }
}

fn diagnostic(
    line_number: Option<u64>,
    event_id: Option<String>,
    message: impl Into<String>,
) -> WorkLinkDiagnostic {
    WorkLinkDiagnostic {
        line_number,
        event_id,
        message: message.into(),
    }
}

fn event_semantics_digest(value: &Value) -> Result<[u8; 32]> {
    let encoded = serde_json::to_vec(value).context("Failed to canonicalize work-link JSON")?;
    let mut digest = Sha256::new();
    digest.update(EVENT_SEMANTICS_DOMAIN);
    digest.update((encoded.len() as u64).to_be_bytes());
    digest.update(encoded);
    Ok(digest.finalize().into())
}

fn link_identity_digest(record: &WorkLinkRecordV1) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(LINK_IDENTITY_DOMAIN);
    digest.update(record.schema_version.to_be_bytes());
    for field in [
        record.plan_id.as_str(),
        record.issue.provider.as_str(),
        record.issue.workspace_id.as_str(),
        record.issue.issue_id.as_str(),
        record.issue.tracker_root.as_str(),
    ] {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field.as_bytes());
    }
    digest.finalize().into()
}

fn valid_event_id_from_value(value: &Value) -> Option<String> {
    let id = value.get("id")?.as_str()?;
    validate_event_id(id).is_ok().then(|| id.to_string())
}

fn valid_plan_id_from_value(value: &Value) -> Option<String> {
    let plan_id = value.get("plan_id")?.as_str()?;
    super::super::plan_files::validate_plan_id(plan_id)
        .is_ok()
        .then(|| plan_id.to_string())
}

pub(super) fn scan_journal(path: &Path, selected_plan: Option<&str>) -> Result<JournalProjection> {
    let mut journal = JournalProjection::new(selected_plan, PRODUCTION_LIMITS);
    let stats = scan_jsonl_raw_bounded(path, &|| false, super::MAX_WORK_LINK_RECORD_BYTES, |raw| {
        journal.observe(raw)
    })
    .with_context(|| format!("Failed to scan work-link journal {}", path.display()))?;
    journal.finish(stats);
    Ok(journal)
}

pub(super) fn scan_journal_locked(
    guard: &JsonlWriteGuard,
    path: &Path,
    selected_plan: Option<&str>,
) -> Result<JournalProjection> {
    let mut journal = JournalProjection::new(selected_plan, PRODUCTION_LIMITS);
    let stats = scan_jsonl_raw_locked_bounded(
        guard,
        path,
        &|| false,
        super::MAX_WORK_LINK_RECORD_BYTES,
        |raw| journal.observe(raw),
    )
    .with_context(|| format!("Failed to scan work-link journal {}", path.display()))?;
    journal.finish(stats);
    Ok(journal)
}

#[cfg(test)]
mod tests;
