//! Serialized run-linkage report types and the recommendations derived from
//! them. Recommendations describe read-only preservation or verified
//! exact-source recovery; none of them mutates state.

use serde_json::{Value, json};

use super::NOT_CHECKED_REASON;
use super::receipt_sources::ReceiptHistorySources;
use super::sources::HistorySources;

#[derive(Debug, Default, serde::Serialize)]
pub(in crate::state::diagnostics) struct RunLinkageReport {
    pub(in crate::state::diagnostics) checked: bool,
    pub(in crate::state::diagnostics) verdict: String,
    pub(in crate::state::diagnostics) reason: Option<String>,
    pub(in crate::state::diagnostics) complete: bool,
    pub(in crate::state::diagnostics) incomplete_reasons: Vec<String>,
    pub(in crate::state::diagnostics) receipts_with_run_id: u64,
    pub(in crate::state::diagnostics) batch_receipts: u64,
    pub(in crate::state::diagnostics) batch_links: u64,
    pub(in crate::state::diagnostics) unresolved_batch_links: u64,
    pub(in crate::state::diagnostics) conflicting_batch_links: u64,
    pub(in crate::state::diagnostics) referenced_runs: u64,
    pub(in crate::state::diagnostics) tracked_references: u64,
    pub(in crate::state::diagnostics) reference_budget_exceeded: bool,
    pub(in crate::state::diagnostics) tracked_lifecycles: u64,
    pub(in crate::state::diagnostics) lifecycle_budget_exceeded: bool,
    pub(in crate::state::diagnostics) runs: RunLinkageCounts,
    pub(in crate::state::diagnostics) journal: RunJournalFacts,
    pub(in crate::state::diagnostics) sources: HistorySources,
    pub(in crate::state::diagnostics) receipt_history: ReceiptHistorySources,
    pub(in crate::state::diagnostics) findings: Vec<RunLinkageFinding>,
    pub(in crate::state::diagnostics) finding_count: u64,
    pub(in crate::state::diagnostics) findings_truncated: bool,
    pub(in crate::state::diagnostics) guidance: Option<Value>,
}

#[derive(Debug, Default, serde::Serialize)]
pub(in crate::state::diagnostics) struct RunLinkageCounts {
    pub(in crate::state::diagnostics) active: u64,
    pub(in crate::state::diagnostics) completed: u64,
    pub(in crate::state::diagnostics) archived_verified: u64,
    pub(in crate::state::diagnostics) recoverable_from_backup: u64,
    pub(in crate::state::diagnostics) missing: u64,
    pub(in crate::state::diagnostics) unverifiable: u64,
    pub(in crate::state::diagnostics) inconsistent: u64,
}

#[derive(Debug, Default, serde::Serialize)]
pub(in crate::state::diagnostics) struct RunJournalFacts {
    pub(in crate::state::diagnostics) path: String,
    pub(in crate::state::diagnostics) exists: bool,
    /// `verified`, `damaged`, `unreadable`, or `absent`.
    pub(in crate::state::diagnostics) authority: String,
    pub(in crate::state::diagnostics) events: u64,
    pub(in crate::state::diagnostics) lifecycles: u64,
    pub(in crate::state::diagnostics) unrecognized_events: u64,
    pub(in crate::state::diagnostics) unrecognized_records: u64,
    pub(in crate::state::diagnostics) inconsistent_lifecycles: u64,
    pub(in crate::state::diagnostics) malformed_records: u64,
    pub(in crate::state::diagnostics) torn_tail: bool,
    pub(in crate::state::diagnostics) scan_error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(in crate::state::diagnostics) struct RunLinkageFinding {
    pub(in crate::state::diagnostics) run_id: String,
    pub(in crate::state::diagnostics) status: String,
    pub(in crate::state::diagnostics) detail: String,
    pub(in crate::state::diagnostics) receipt_ids: Vec<String>,
    pub(in crate::state::diagnostics) receipt_count: u64,
    pub(in crate::state::diagnostics) receipt_ids_truncated: bool,
    pub(in crate::state::diagnostics) batch_receipt_ids: Vec<String>,
    pub(in crate::state::diagnostics) batch_receipt_count: u64,
    pub(in crate::state::diagnostics) batch_receipt_ids_truncated: bool,
    pub(in crate::state::diagnostics) journal_events: u64,
    pub(in crate::state::diagnostics) journal_anomalies: Vec<String>,
    pub(in crate::state::diagnostics) lease_file_present: Option<bool>,
    pub(in crate::state::diagnostics) history_sources: Vec<Value>,
    pub(in crate::state::diagnostics) recovery: Option<Value>,
}

impl RunLinkageReport {
    pub(in crate::state::diagnostics) fn not_checked() -> Self {
        Self {
            checked: false,
            verdict: "not_checked".into(),
            reason: Some(NOT_CHECKED_REASON.into()),
            complete: false,
            ..Self::default()
        }
    }

    /// A report that was not produced carries no counts that could read as
    /// verified facts; only the disclosure that the check did not run.
    pub(in crate::state::diagnostics) fn to_value(&self) -> Value {
        if self.checked {
            serde_json::to_value(self).unwrap_or_else(|error| {
                json!({"checked": true, "verdict": "incomplete", "serialization_error": error.to_string()})
            })
        } else {
            json!({
                "checked": false,
                "verdict": self.verdict,
                "complete": false,
                "reason": self.reason,
                "finding_count": 0,
                "findings": [],
            })
        }
    }
}

pub(in crate::state::diagnostics) fn guidance() -> Value {
    json!({
        "derived_caches": "Run leases, indexes, and other artifacts under .agent/.cache are derived from the journals. Rebuilding them never restores missing run events, event identities, or their order.",
        "authoritative_recovery": "Exact recovery needs an authoritative copy of the original run journal: a manifested state backup, a copy from another checkout, or committed Git history. Only such a source can restore the original event IDs and order, and restoring it replaces the whole stream, so preserve any newer appends first.",
        "preservation": "Keep existing receipts and journals unchanged. Export the affected receipts for retention and append a decision that records the affected run and receipt IDs and states that their run history is unavailable.",
        "new_evidence": "Rerunning checks produces new receipts and new run events. It is new evidence and never reconstructs the missing history.",
        "never": [
            "Do not append fabricated queued, target_completed, or completed events for the affected runs.",
            "Do not restore a whole run stream over a journal that has newer appends without preserving them.",
        ],
    })
}

fn preview_ids<'a>(ids: impl IntoIterator<Item = &'a String>, total: u64) -> String {
    let shown = ids.into_iter().take(3).cloned().collect::<Vec<_>>();
    let omitted = total.saturating_sub(shown.len() as u64);
    if omitted == 0 {
        shown.join(", ")
    } else {
        format!("{} (+{omitted} more)", shown.join(", "))
    }
}

/// Recommendations derived from the linkage report; they never mutate state.
pub(in crate::state::diagnostics) fn recommendations(report: &RunLinkageReport) -> Vec<Value> {
    let mut recommendations = Vec::new();
    if !report.checked {
        return recommendations;
    }
    let unavailable = report
        .findings
        .iter()
        .filter(|finding| finding.status != "recoverable_from_backup")
        .collect::<Vec<_>>();
    let unavailable_count = report
        .runs
        .missing
        .saturating_add(report.runs.unverifiable)
        .saturating_add(report.runs.inconsistent);
    if unavailable_count > 0 {
        let coverage_truncated = report.findings_truncated || report.reference_budget_exceeded;
        let receipt_total = unavailable
            .iter()
            .map(|finding| finding.receipt_count)
            .sum::<u64>();
        let receipt_total = if coverage_truncated {
            format!("At least {receipt_total} retained receipt(s)")
        } else {
            format!("{receipt_total} receipt(s)")
        };
        let unavailable_total = if coverage_truncated {
            format!("at least {unavailable_count} retained run(s)")
        } else {
            format!("{unavailable_count} run(s)")
        };
        let run_ids = unavailable.iter().map(|finding| &finding.run_id);
        recommendations.push(json!({
            "kind": "preserve_unlinked_receipt_evidence",
            "command": "jig state export receipts --before <YYYY-MM-DD> --output receipts-preserved.jsonl.gz",
            "alternative_command": "jig work decide --title \"Run history unavailable\" --selected-option \"Preserve receipts; record affected IDs\" --rationale \"<affected run and receipt IDs; run history unavailable>\"",
            "affected_run_ids": unavailable.iter().map(|finding| finding.run_id.clone()).collect::<Vec<_>>(),
            "affected_run_ids_truncated": coverage_truncated,
            "reason": format!(
                "{receipt_total} reference {unavailable_total} whose lifecycle is unavailable or unverifiable in this checkout ({}). Existing receipts remain valid evidence: keep them, export them for retention, and append a decision naming the affected run and receipt IDs and this limitation. Rebuilding derived caches does not restore run events, a new check run is new evidence rather than restored history, and no queued, target_completed, or completed events may be fabricated.",
                preview_ids(run_ids, unavailable_count)
            ),
        }));
    }
    for finding in report
        .findings
        .iter()
        .filter(|finding| finding.status == "recoverable_from_backup")
    {
        let backup_path = finding
            .recovery
            .as_ref()
            .and_then(|recovery| recovery["backup_path"].as_str())
            .unwrap_or("<backup-directory>");
        recommendations.push(json!({
            "kind": "recover_run_history_from_backup",
            "command": null,
            "affected_run_ids": [finding.run_id],
            "backup": finding.recovery,
            "reason": format!(
                "Exact state backup {backup_path} contains lifecycle events for run {}, but diagnosis cannot establish restore eligibility from a racing read-only snapshot. Compare both journals, preserve newer events, and run restore preflight manually; restore replaces the entire run journal and refuses while runs or worker leases are active.",
                finding.run_id
            ),
        }));
    }
    if !report.complete {
        recommendations.push(json!({
            "kind": "complete_run_linkage_check",
            "command": null,
            "reason": format!(
                "Run linkage could not be fully verified: {}. Resolve the reported problems and rerun `jig state diagnose --deep` before treating receipt-to-run linkage as clean.",
                report.incomplete_reasons.join("; ")
            ),
        }));
    }
    recommendations
}
