use std::process::Output;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::text::truncate_chars;

const MAX_EVIDENCE_PREVIEW_CHARS: usize = 500;
// Keep review receipts bounded while preserving raw counts separately so
// truncated findings cannot appear smaller than they were.
const MAX_REVIEW_FIELD_CHARS: usize = 2_000;
const MAX_REVIEW_FINDINGS: usize = 100;

pub(super) const REVIEW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub(super) struct CodexReviewOutput {
    pub(super) summary: String,
    pub(super) findings: Vec<CodexReviewFinding>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexReviewFinding {
    pub(super) severity: String,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) line: Option<u64>,
    pub(super) issue: String,
    #[serde(default)]
    pub(super) evidence: String,
    pub(super) recommendation: String,
}

pub(super) fn parse_review_output(output: &Output, stdout: &str) -> Result<CodexReviewOutput> {
    serde_json::from_str::<CodexReviewOutput>(stdout).with_context(|| {
        format!(
            "Codex review did not return valid structured JSON (status {}). stdout preview: {}",
            output.status.code().unwrap_or(1),
            stdout.chars().take(500).collect::<String>()
        )
    })
}

pub(super) fn normalize_findings(findings: Vec<CodexReviewFinding>, gate_id: &str) -> Vec<Value> {
    findings
        .into_iter()
        .take(MAX_REVIEW_FINDINGS)
        .map(|finding| {
            let CodexReviewFinding {
                severity,
                path,
                line,
                issue,
                evidence,
                recommendation,
            } = finding;
            let severity = normalize_severity(&severity);
            let fingerprint = finding_fingerprint(
                gate_id,
                severity,
                path.as_deref(),
                line,
                &issue,
                &evidence,
                &recommendation,
            );
            serde_json::json!({
                "fingerprint": fingerprint,
                "severity": severity,
                "path": path.map(|path| truncate_evidence_text(&path)),
                "line": line,
                "issue": truncate_evidence_text(&issue),
                "evidence": truncate_evidence_text(&evidence),
                "recommendation": truncate_evidence_text(&recommendation),
            })
        })
        .collect()
}

pub(super) fn actionable_findings(review_result: &Value) -> Result<Vec<Value>> {
    let reviews = review_result
        .get("reviews")
        .and_then(Value::as_array)
        .context("work review result is missing reviews array")?;
    let mut findings = Vec::new();
    for review in reviews {
        let actionable = review
            .get("actionable_findings")
            .and_then(Value::as_array)
            .context("work review result is missing actionable_findings array")?;
        findings.extend(actionable.iter().cloned());
    }
    Ok(findings)
}

pub(super) fn review_failed_gates(review_result: &Value) -> Result<Vec<Value>> {
    review_result
        .get("failed_gates")
        .and_then(Value::as_array)
        .cloned()
        .context("work review result is missing failed_gates array")
}

pub(super) fn checks_passed(check_result: &Value) -> Result<bool> {
    let gate_evidence_ok = check_result
        .get("gate_evidence")
        .and_then(Value::as_array)
        .is_none_or(|gates| {
            gates.iter().all(|gate| {
                matches!(
                    gate.get("status").and_then(Value::as_str),
                    Some("executed" | "reused" | "not_applicable")
                )
            })
        });
    let checks = check_result
        .get("checks")
        .and_then(Value::as_array)
        .context("work check result is missing checks array")?;
    Ok(gate_evidence_ok
        && checks.iter().all(|check| {
            check
                .get("result")
                .and_then(|result| result.get("exit_status"))
                .and_then(Value::as_i64)
                == Some(0)
        }))
}

pub(super) fn finding_meets_threshold(finding: &Value, threshold: &str) -> bool {
    let severity = finding["severity"].as_str().unwrap_or("warning");
    severity_rank(severity) >= severity_rank(threshold)
}

pub(super) fn normalize_severity(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "critical" | "high" => "critical",
        "suggestion" | "low" => "suggestion",
        _ => "warning",
    }
}

// Unknown severities deliberately receive the warning rank; the named warning
// arm remains explicit because this ordering is part of review policy.
#[allow(clippy::match_same_arms)]
pub(super) fn severity_rank(value: &str) -> u8 {
    match value {
        "critical" => 3,
        "warning" => 2,
        "suggestion" => 1,
        _ => 2,
    }
}

pub(super) fn evidence_preview(value: &str) -> String {
    truncate_chars(value, MAX_EVIDENCE_PREVIEW_CHARS)
}

pub(super) fn truncate_evidence_text(value: &str) -> String {
    truncate_chars(value, MAX_REVIEW_FIELD_CHARS)
}

pub(super) fn hash_json(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value)?;
    Ok(hash_bytes(&bytes))
}

pub(super) fn hash_text(value: &str) -> String {
    hash_bytes(value.as_bytes())
}

fn finding_fingerprint(
    gate_id: &str,
    severity: &str,
    path: Option<&str>,
    line: Option<u64>,
    issue: &str,
    evidence: &str,
    recommendation: &str,
) -> String {
    let value = serde_json::json!([
        gate_id,
        severity,
        path.unwrap_or_default(),
        line,
        issue,
        evidence,
        recommendation,
    ]);
    let bytes = serde_json::to_vec(&value).expect("finding fingerprint input is valid JSON");
    hash_bytes(&bytes)
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{normalize_severity, severity_rank};

    #[test]
    fn severity_normalization_handles_aliases_and_case() {
        assert_eq!(normalize_severity("HIGH"), "critical");
        assert_eq!(normalize_severity("medium"), "warning");
        assert_eq!(normalize_severity("Low"), "suggestion");
        assert_eq!(normalize_severity("unexpected"), "warning");
    }

    #[test]
    fn severity_rank_defaults_unknown_values_to_warning() {
        assert!(severity_rank("critical") > severity_rank("warning"));
        assert!(severity_rank("warning") > severity_rank("suggestion"));
        assert_eq!(severity_rank("unexpected"), severity_rank("warning"));
    }
}
