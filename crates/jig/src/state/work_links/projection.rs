use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

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

#[derive(Clone, Debug)]
struct SeenEvent {
    value: Value,
    line_number: u64,
    plan_id: Option<String>,
}

#[derive(Default)]
pub(super) struct JournalProjection {
    seen_events: BTreeMap<String, SeenEvent>,
    records_by_plan: BTreeMap<String, Vec<WorkLinkRecordV1>>,
    replayed_by_event: BTreeMap<String, u64>,
    conflicts_by_plan: BTreeMap<String, Vec<WorkLinkDiagnostic>>,
    unsupported_by_plan: BTreeMap<String, Vec<WorkLinkDiagnostic>>,
    corrupt_by_plan: BTreeMap<String, Vec<WorkLinkDiagnostic>>,
    global_conflicts: Vec<WorkLinkDiagnostic>,
    global_corruption: Vec<WorkLinkDiagnostic>,
    torn_tail: bool,
}

impl JournalProjection {
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
            );
            return Ok(());
        };

        if let Some(previous) = self.seen_events.get(&event_id).cloned() {
            if previous.value == value {
                *self.replayed_by_event.entry(event_id).or_default() += 1;
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
                    self.conflicts_by_plan
                        .entry(left.clone())
                        .or_default()
                        .push(detail.clone());
                    if right != left {
                        self.conflicts_by_plan
                            .entry(right.clone())
                            .or_default()
                            .push(detail);
                    }
                }
                (Some(plan_id), None) | (None, Some(plan_id)) => {
                    self.conflicts_by_plan
                        .entry(plan_id.clone())
                        .or_default()
                        .push(detail.clone());
                    self.global_conflicts.push(detail);
                }
                (None, None) => self.global_conflicts.push(detail),
            }
            return Ok(());
        }
        self.seen_events.insert(
            event_id.clone(),
            SeenEvent {
                value: value.clone(),
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
                self.unsupported_by_plan
                    .entry(plan_id)
                    .or_default()
                    .push(diagnostic(
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
                );
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
                self.unsupported_by_plan
                    .entry(plan_id)
                    .or_default()
                    .push(diagnostic(
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
                );
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
                );
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
            );
            return Ok(());
        }
        self.records_by_plan
            .entry(plan_id)
            .or_default()
            .push(record);
        Ok(())
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

    fn push_corrupt(&mut self, plan_id: Option<&str>, detail: WorkLinkDiagnostic) {
        if let Some(plan_id) = plan_id {
            self.corrupt_by_plan
                .entry(plan_id.into())
                .or_default()
                .push(detail);
        } else {
            self.global_corruption.push(detail);
        }
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
        let mut corruption = self.global_corruption.clone();
        corruption.extend(
            self.corrupt_by_plan
                .get(plan_id)
                .cloned()
                .unwrap_or_default(),
        );
        if !corruption.is_empty() {
            return WorkLinkProjection::Corrupt(corruption);
        }

        let mut conflicts = self.global_conflicts.clone();
        conflicts.extend(
            self.conflicts_by_plan
                .get(plan_id)
                .cloned()
                .unwrap_or_default(),
        );
        let records = self
            .records_by_plan
            .get(plan_id)
            .cloned()
            .unwrap_or_default();
        if records
            .windows(2)
            .any(|pair| !pair[0].same_link_record(&pair[1]))
        {
            conflicts.push(diagnostic(
                None,
                None,
                format!("plan {plan_id} has multiple distinct immutable work links"),
            ));
        }
        if !conflicts.is_empty() {
            return WorkLinkProjection::Conflict(conflicts);
        }

        if let Some(unsupported) = self.unsupported_by_plan.get(plan_id)
            && !unsupported.is_empty()
        {
            return WorkLinkProjection::Unsupported(unsupported.clone());
        }
        let Some(mut record) = records.into_iter().next() else {
            return WorkLinkProjection::Unlinked;
        };
        let mut event_ids = self
            .records_by_plan
            .get(plan_id)
            .into_iter()
            .flatten()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        event_ids.sort();
        if let Some(first) = event_ids.first()
            && first != &record.id
            && let Some(first_record) = self
                .records_by_plan
                .get(plan_id)
                .and_then(|records| records.iter().find(|candidate| &candidate.id == first))
        {
            record = first_record.clone();
        }
        let replayed_records = event_ids
            .iter()
            .filter_map(|event_id| self.replayed_by_event.get(event_id))
            .copied()
            .sum();
        WorkLinkProjection::Supported(Box::new(SupportedWorkLink {
            record,
            event_ids,
            replayed_records,
        }))
    }

    pub(super) fn diagnostics(&self) -> WorkLinkJournalDiagnostics {
        let mut plan_ids = BTreeSet::new();
        plan_ids.extend(self.records_by_plan.keys().cloned());
        plan_ids.extend(self.conflicts_by_plan.keys().cloned());
        plan_ids.extend(self.unsupported_by_plan.keys().cloned());
        plan_ids.extend(self.corrupt_by_plan.keys().cloned());
        plan_ids.extend(
            self.seen_events
                .values()
                .filter_map(|event| event.plan_id.clone()),
        );

        let mut report = WorkLinkJournalDiagnostics {
            known_plans: plan_ids.len() as u64,
            supported_records: self
                .records_by_plan
                .values()
                .map(|records| records.len() as u64)
                .sum(),
            replayed_records: self.replayed_by_event.values().copied().sum(),
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
            for plan_id in &plan_ids {
                match self.for_plan(plan_id) {
                    WorkLinkProjection::Unlinked => {}
                    WorkLinkProjection::Supported(_) => report.supported_plans += 1,
                    WorkLinkProjection::Unsupported(errors) => {
                        report.unsupported_plans += 1;
                        append_journal_diagnostics(&mut report, Some(plan_id.as_str()), &errors);
                    }
                    WorkLinkProjection::Conflict(errors) => {
                        report.conflicting_plans += 1;
                        append_journal_diagnostics(&mut report, Some(plan_id.as_str()), &errors);
                    }
                    WorkLinkProjection::Corrupt(errors) => {
                        report.corrupt_plans += 1;
                        append_journal_diagnostics(&mut report, Some(plan_id.as_str()), &errors);
                    }
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
    diagnostics: &[WorkLinkDiagnostic],
) {
    report.error_count = report.error_count.saturating_add(diagnostics.len() as u64);
    for diagnostic in diagnostics {
        if report.errors.len() >= MAX_JOURNAL_DIAGNOSTIC_SAMPLES {
            break;
        }
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

pub(super) fn scan_journal(path: &Path) -> Result<JournalProjection> {
    let mut journal = JournalProjection::default();
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
) -> Result<JournalProjection> {
    let mut journal = JournalProjection::default();
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
