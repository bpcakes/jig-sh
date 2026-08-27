// agentic-loc-exception: gate orchestration stays cohesive with dependency and receipt evaluation.
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::command::{WorkEvidenceRequest, WorkGatesRequest};
use crate::context::{RepoContext, WorkGate};
use crate::git_receipts::PlanChangeSnapshot;
use crate::repository::{RepositoryCatalog, resolve_evidence_targets};
use crate::state::{
    PlanBaseline, PlanStatus, ToolReceiptStatus, WorkCheckGateEvidence, WorkGateReceiptIndex,
    WorkReviewReceiptEvidence, WorkReviewReceiptStatus, current_worktree_fingerprint,
    current_worktree_fingerprint_with_cancellation, ensure_plan_exists,
    ensure_plan_exists_with_cancellation, open_plan_summaries,
    open_plan_summaries_with_cancellation, plan_baselines_with_cancellation, plan_status,
    plan_status_with_cancellation, work_gate_receipt_index,
    work_gate_receipt_index_with_cancellation, work_gate_receipt_indexes_with_cancellation,
};

use super::scope::{GateScopeEvaluation, PlanGateContext};
use super::tools::validate_check_tool;

use target_evidence::EvidenceGateEvaluation;

const MAX_GATE_CHANGED_PATHS: usize = 100;

#[derive(Clone, Copy)]
enum GateCollection<'a> {
    Blocking,
    Cancellable(&'a dyn Fn() -> bool),
}

impl GateCollection<'_> {
    fn ensure_active(self) -> Result<()> {
        match self {
            Self::Blocking => Ok(()),
            Self::Cancellable(cancelled) => ensure_gate_collection_active(cancelled),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GateOutcome {
    Passed,
    Reused,
    NotApplicable,
    Missing,
    Failed,
    InvalidOutput,
    Stale,
    Unknown,
    Unsupported,
}

impl GateOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Reused => "reused",
            Self::NotApplicable => "not_applicable",
            Self::Missing => "missing",
            Self::Failed => "failed",
            Self::InvalidOutput => "invalid_output",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
        }
    }

    const fn with_freshness(self, freshness: GateFreshness) -> Self {
        if matches!(self, Self::Passed | Self::Reused | Self::NotApplicable) {
            match freshness {
                GateFreshness::Fresh => self,
                _ => freshness.as_gate_outcome(),
            }
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GateFreshness {
    Fresh,
    Missing,
    Stale,
    Unknown,
}

impl GateFreshness {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Missing => "missing",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }

    const fn as_gate_outcome(self) -> GateOutcome {
        match self {
            Self::Fresh => GateOutcome::Passed,
            Self::Missing => GateOutcome::Missing,
            Self::Stale => GateOutcome::Stale,
            Self::Unknown => GateOutcome::Unknown,
        }
    }

    fn reason<T: GateReceiptView>(
        self,
        receipt: Option<&T>,
        current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    ) -> &'static str {
        match self {
            Self::Fresh => "receipt matches current worktree fingerprint",
            Self::Missing => "no receipt exists for this gate",
            Self::Stale => "receipt was recorded for a different worktree fingerprint",
            Self::Unknown
                if receipt
                    .and_then(GateReceiptView::worktree_fingerprint)
                    .is_none() =>
            {
                "receipt did not record a worktree fingerprint"
            }
            Self::Unknown if current_fingerprint.fingerprint.is_none() => {
                "current worktree fingerprint could not be collected"
            }
            Self::Unknown => "worktree freshness could not be determined",
        }
    }
}

#[derive(Clone, Debug)]
struct EvaluatedReceipt {
    receipt_id: Option<String>,
    freshness_receipt_id: Option<String>,
    exit_status: Option<i32>,
    ended_at_ms: Option<u64>,
    freshness: GateFreshness,
    freshness_reason: String,
    changed_paths: Vec<String>,
    changed_path_count: usize,
    changed_paths_truncated: bool,
    changed_paths_digest: Option<String>,
    diff_summary: Option<String>,
    receipt_worktree_fingerprint_error: Option<String>,
    current_worktree_fingerprint_error: Option<String>,
}

impl EvaluatedReceipt {
    fn new<T: GateReceiptView>(
        receipt: Option<&T>,
        freshness_receipt: Option<&T>,
        current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    ) -> Self {
        let freshness = gate_freshness(freshness_receipt, current_fingerprint);
        let freshness_reason = freshness
            .reason(freshness_receipt, current_fingerprint)
            .to_string();
        Self::with_freshness(
            receipt,
            freshness_receipt,
            current_fingerprint,
            freshness,
            freshness_reason,
        )
    }

    fn with_freshness<T: GateReceiptView>(
        receipt: Option<&T>,
        freshness_receipt: Option<&T>,
        current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
        freshness: GateFreshness,
        freshness_reason: String,
    ) -> Self {
        let (changed_paths, changed_path_count, changed_paths_truncated, changed_paths_digest) =
            gate_changed_paths(freshness_receipt);
        Self {
            receipt_id: receipt.map(|receipt| receipt.receipt_id().to_string()),
            freshness_receipt_id: freshness_receipt.map(|receipt| receipt.receipt_id().to_string()),
            exit_status: receipt.map(GateReceiptView::exit_status),
            ended_at_ms: receipt.map(GateReceiptView::ended_at_ms),
            freshness,
            freshness_reason,
            changed_paths,
            changed_path_count,
            changed_paths_truncated,
            changed_paths_digest,
            diff_summary: freshness_receipt.map(|receipt| receipt.diff_summary().to_string()),
            receipt_worktree_fingerprint_error: freshness_receipt
                .and_then(GateReceiptView::worktree_fingerprint_error)
                .map(str::to_string),
            current_worktree_fingerprint_error: current_fingerprint.error.clone(),
        }
    }

    fn scoped(
        status: &crate::state::WorkCheckGateReceiptStatus,
        current: &GateScopeEvaluation,
    ) -> Self {
        let evidence = &status.evidence;
        let (freshness, freshness_reason) =
            if let Some(error) = status.batch.worktree_fingerprint_error.as_deref() {
                (
                    GateFreshness::Unknown,
                    format!("work-check batch could not prove a stable worktree: {error}"),
                )
            } else if let Some(error) = current.error() {
                (
                    GateFreshness::Unknown,
                    format!("current gate scope could not be collected: {error}"),
                )
            } else if evidence.gate_signature != current.gate_signature() {
                (
                    GateFreshness::Stale,
                    "gate policy or execution definition changed since the receipt".into(),
                )
            } else {
                match (
                    evidence.scope_fingerprint.as_deref(),
                    current.scope_fingerprint(),
                ) {
                    (Some(receipt), Some(current)) if receipt == current => (
                        GateFreshness::Fresh,
                        "receipt matches the current gate-scoped fingerprint".into(),
                    ),
                    (Some(_), Some(_)) => (
                        GateFreshness::Stale,
                        "receipt was recorded for a different gate-scoped fingerprint".into(),
                    ),
                    _ => (
                        GateFreshness::Unknown,
                        "gate-scoped freshness could not be determined".into(),
                    ),
                }
            };
        Self {
            receipt_id: evidence
                .tool_receipt_id
                .clone()
                .or_else(|| Some(status.batch.receipt_id.clone())),
            freshness_receipt_id: Some(status.batch.receipt_id.clone()),
            exit_status: match evidence.status.as_str() {
                "executed" | "failed" | "cancelled" => evidence.exit_status,
                "reused" | "not_applicable" => Some(0),
                "unknown" => None,
                _ => None,
            },
            ended_at_ms: Some(status.batch.ended_at_ms),
            freshness,
            freshness_reason,
            changed_paths: evidence.changed_paths.clone(),
            changed_path_count: evidence.changed_path_count,
            changed_paths_truncated: evidence.changed_paths_truncated,
            changed_paths_digest: evidence.changed_paths_digest.clone(),
            diff_summary: Some(status.batch.diff_summary.clone()),
            receipt_worktree_fingerprint_error: status
                .batch
                .worktree_fingerprint_error
                .clone()
                .or_else(|| evidence.scope_error.clone()),
            current_worktree_fingerprint_error: current.error().map(str::to_string),
        }
    }
}

#[derive(Clone, Debug)]
struct CheckGateEvaluation {
    id: String,
    required: bool,
    tool: String,
    paths: Option<Vec<String>>,
    paths_ignore: Vec<String>,
    reuse: bool,
    outcome: GateOutcome,
    receipt: EvaluatedReceipt,
    evidence: Option<WorkCheckGateEvidence>,
    current_scope: GateScopeEvaluation,
}

#[derive(Clone, Debug)]
struct ReviewGateEvaluation {
    id: String,
    required: bool,
    skill: String,
    outcome: GateOutcome,
    receipt: EvaluatedReceipt,
    evidence: Option<WorkReviewReceiptEvidence>,
}

#[derive(Clone, Debug)]
struct UnsupportedGateEvaluation {
    id: String,
    required: bool,
    kind: String,
    reason: Option<String>,
}

#[derive(Clone, Debug)]
enum GateEvaluation {
    Check(Box<CheckGateEvaluation>),
    Evidence(EvidenceGateEvaluation),
    CodexReview(Box<ReviewGateEvaluation>),
    Unsupported(UnsupportedGateEvaluation),
}

impl GateEvaluation {
    fn id(&self) -> &str {
        match self {
            Self::Check(gate) => &gate.id,
            Self::Evidence(gate) => gate.id(),
            Self::CodexReview(gate) => &gate.id,
            Self::Unsupported(gate) => &gate.id,
        }
    }

    const fn required(&self) -> bool {
        match self {
            Self::Check(gate) => gate.required,
            Self::Evidence(gate) => gate.required(),
            Self::CodexReview(gate) => gate.required,
            Self::Unsupported(gate) => gate.required,
        }
    }

    const fn outcome(&self) -> GateOutcome {
        match self {
            Self::Check(gate) => gate.outcome,
            Self::Evidence(gate) => gate.outcome(),
            Self::CodexReview(gate) => gate.outcome,
            Self::Unsupported(_) => GateOutcome::Unsupported,
        }
    }

    fn unsupported_label(&self) -> String {
        match self {
            Self::Unsupported(gate) => gate.reason.as_ref().map_or_else(
                || format!("{} (kind: {})", gate.id, gate.kind),
                |reason| format!("{} (kind: {}; {reason})", gate.id, gate.kind),
            ),
            _ => self.id().to_string(),
        }
    }

    fn receipt(&self) -> Option<&EvaluatedReceipt> {
        match self {
            Self::Check(gate) => Some(&gate.receipt),
            Self::Evidence(gate) => gate.receipt(),
            Self::CodexReview(gate) => Some(&gate.receipt),
            Self::Unsupported(_) => None,
        }
    }

    fn evidence_key(&self) -> Option<String> {
        match self {
            Self::Check(gate) => Some(format!("gate:{}", gate.id)),
            Self::Evidence(gate) => Some(gate.evidence_key()),
            Self::CodexReview(gate) => Some(format!("gate:{}", gate.id)),
            Self::Unsupported(_) => None,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Check(gate) => {
                let receipt = &gate.receipt;
                let evidence = gate.evidence.as_ref();
                let current = &gate.current_scope;
                json!({
                    "id": gate.id,
                    "kind": "check",
                    "required": gate.required,
                    "tool": gate.tool,
                    "status": gate.outcome.as_str(),
                    "receipt_id": receipt.receipt_id,
                    "freshness_receipt_id": receipt.freshness_receipt_id,
                    "exit_status": receipt.exit_status,
                    "ended_at_ms": receipt.ended_at_ms,
                    "freshness": receipt.freshness.as_str(),
                    "freshness_reason": receipt.freshness_reason,
                    "changed_paths": receipt.changed_paths,
                    "changed_path_count": receipt.changed_path_count,
                    "changed_paths_truncated": receipt.changed_paths_truncated,
                    "changed_paths_digest": receipt.changed_paths_digest,
                    "diff_summary": receipt.diff_summary,
                    "receipt_worktree_fingerprint_error": receipt.receipt_worktree_fingerprint_error,
                    "current_worktree_fingerprint_error": receipt.current_worktree_fingerprint_error,
                    "evidence_status": evidence.map(|evidence| evidence.status.as_str()),
                    "receipt_applicability": evidence.map(|evidence| evidence.applicability.as_str()),
                    "applicability": current.applicability().map(crate::git_receipts::GateApplicability::as_str),
                    "applicability_reason": current.reason(),
                    "applicability_error": current.error(),
                    "paths": gate.paths,
                    "paths_ignore": gate.paths_ignore,
                    "reuse": gate.reuse,
                    "forced": evidence.map(|evidence| evidence.forced),
                    "baseline_oid": current.baseline_oid(),
                    "receipt_baseline_oid": evidence.and_then(|evidence| evidence.baseline_oid.as_deref()),
                    "gate_signature": current.gate_signature(),
                    "receipt_gate_signature": evidence.map(|evidence| evidence.gate_signature.as_str()),
                    "scope_fingerprint": current.scope_fingerprint(),
                    "receipt_scope_fingerprint": evidence.and_then(|evidence| evidence.scope_fingerprint.as_deref()),
                    "matching_paths": current.matching_paths(),
                    "matching_path_count": current.matching_path_count(),
                    "matching_paths_truncated": current.matching_paths_truncated(),
                    "matching_paths_digest": current.matching_paths_digest(),
                    "source_plan_id": evidence.and_then(|evidence| evidence.source_plan_id.as_deref()),
                    "source_batch_receipt_id": evidence.and_then(|evidence| evidence.source_batch_receipt_id.as_deref()),
                    "source_tool_receipt_id": evidence.and_then(|evidence| evidence.source_tool_receipt_id.as_deref()),
                })
            }
            Self::Evidence(gate) => gate.to_value(),
            Self::CodexReview(gate) => {
                let receipt = &gate.receipt;
                let evidence = gate.evidence.as_ref();
                json!({
                    "id": gate.id,
                    "kind": "codex_review",
                    "required": gate.required,
                    "skill": gate.skill,
                    "status": gate.outcome.as_str(),
                    "receipt_id": receipt.receipt_id,
                    "exit_status": receipt.exit_status,
                    "ended_at_ms": receipt.ended_at_ms,
                    "freshness": receipt.freshness.as_str(),
                    "freshness_reason": receipt.freshness_reason,
                    "changed_paths": receipt.changed_paths,
                    "changed_path_count": receipt.changed_path_count,
                    "changed_paths_truncated": receipt.changed_paths_truncated,
                    "changed_paths_digest": receipt.changed_paths_digest,
                    "diff_summary": receipt.diff_summary,
                    "finding_count": evidence.and_then(|evidence| evidence.finding_count),
                    "actionable_count": evidence.and_then(|evidence| evidence.actionable_count),
                    "retained_finding_count": evidence.and_then(|evidence| evidence.retained_finding_count),
                    "retained_actionable_count": evidence.and_then(|evidence| evidence.retained_actionable_count),
                    "findings_truncated": evidence.and_then(|evidence| evidence.findings_truncated),
                    "actionable_findings_truncated": evidence.and_then(|evidence| evidence.actionable_findings_truncated),
                    "threshold": evidence.and_then(|evidence| evidence.threshold.as_deref()),
                    "parse_error": evidence.and_then(WorkReviewReceiptEvidence::parse_error),
                    "receipt_worktree_fingerprint_error": receipt.receipt_worktree_fingerprint_error,
                    "current_worktree_fingerprint_error": receipt.current_worktree_fingerprint_error,
                })
            }
            Self::Unsupported(gate) => {
                let mut value = json!({
                    "id": gate.id,
                    "kind": gate.kind,
                    "required": gate.required,
                    "status": GateOutcome::Unsupported.as_str(),
                });
                if let Some(reason) = &gate.reason {
                    value["reason"] = Value::String(reason.clone());
                }
                value
            }
        }
    }

    fn to_latest_evidence(&self) -> Option<Value> {
        let receipt = self.receipt()?;
        match self {
            Self::Evidence(gate) => return gate.to_latest_evidence(),
            Self::Check(_) => {
                if !matches!(
                    self.outcome(),
                    GateOutcome::Passed | GateOutcome::Reused | GateOutcome::NotApplicable
                ) || receipt.freshness != GateFreshness::Fresh
                {
                    return None;
                }
            }
            Self::CodexReview(_) if receipt.exit_status != Some(0) => return None,
            Self::CodexReview(_) => {}
            Self::Unsupported(_) => return None,
        }
        let (tool, skill, freshness_receipt_id) = match self {
            Self::Check(gate) => (
                Some(gate.tool.as_str()),
                None,
                receipt.freshness_receipt_id.as_deref(),
            ),
            // Review gate JSON intentionally has no freshness_receipt_id field;
            // the legacy evidence projection therefore exposed null here.
            Self::CodexReview(gate) => (None, Some(gate.skill.as_str()), None),
            Self::Evidence(_) => unreachable!("evidence gates return above"),
            Self::Unsupported(_) => return None,
        };
        let mut value = json!({
            "tool": tool,
            "skill": skill,
            "gate_id": self.id(),
            "status": self.outcome().as_str(),
            "receipt_id": receipt.receipt_id,
            "freshness_receipt_id": freshness_receipt_id,
            "matches_current_worktree": receipt.freshness == GateFreshness::Fresh,
            "freshness": receipt.freshness.as_str(),
            "freshness_reason": receipt.freshness_reason,
            "changed_paths": receipt.changed_paths,
            "changed_path_count": receipt.changed_path_count,
            "changed_paths_truncated": receipt.changed_paths_truncated,
            "changed_paths_digest": receipt.changed_paths_digest,
            "diff_summary": receipt.diff_summary,
            "ended_at_ms": receipt.ended_at_ms.unwrap_or(0),
        });
        if let Self::Check(gate) = self {
            value["applicability"] = json!(
                gate.current_scope
                    .applicability()
                    .map(crate::git_receipts::GateApplicability::as_str)
            );
            value["applicability_reason"] = json!(gate.current_scope.reason());
            value["baseline_oid"] = json!(gate.current_scope.baseline_oid());
            value["matching_paths"] = json!(gate.current_scope.matching_paths());
            value["matching_path_count"] = json!(gate.current_scope.matching_path_count());
            value["matching_paths_truncated"] =
                json!(gate.current_scope.matching_paths_truncated());
            value["matching_paths_digest"] = json!(gate.current_scope.matching_paths_digest());
            value["scope_fingerprint"] = json!(gate.current_scope.scope_fingerprint());
            value["gate_signature"] = json!(gate.current_scope.gate_signature());
            value["evidence_status"] = json!(
                gate.evidence
                    .as_ref()
                    .map(|evidence| evidence.status.as_str())
            );
            value["source_plan_id"] = json!(
                gate.evidence
                    .as_ref()
                    .and_then(|evidence| evidence.source_plan_id.as_deref())
            );
            value["source_tool_receipt_id"] = json!(
                gate.evidence
                    .as_ref()
                    .and_then(|evidence| evidence.source_tool_receipt_id.as_deref())
            );
        }
        Some(value)
    }
}

struct GateReport {
    plan_id: String,
    plan_state: &'static str,
    plan_baseline: Option<PlanBaseline>,
    current_worktree_fingerprint: Option<String>,
    current_worktree_fingerprint_error: Option<String>,
    gates: Vec<GateEvaluation>,
    required_failures: RequiredGateFailures,
}

impl GateReport {
    fn gates_ok(&self) -> bool {
        self.required_failures.is_empty()
    }

    fn to_value(&self) -> Value {
        let gates_ok = self.gates_ok();
        json!({
            "ok": true,
            "gates_ok": gates_ok,
            "plan_id": self.plan_id,
            "plan_state": self.plan_state,
            "plan_baseline": self.plan_baseline,
            "overall": if gates_ok { "passed" } else { "blocked" },
            "current_worktree_fingerprint": self.current_worktree_fingerprint,
            "current_worktree_fingerprint_error": self.current_worktree_fingerprint_error,
            "gates": self.gates.iter().map(GateEvaluation::to_value).collect::<Vec<_>>(),
            "missing_required": self.required_failures.missing,
            "failed_required": self.required_failures.failed,
            "stale_required": self.required_failures.stale,
            "unknown_required": self.required_failures.unknown,
            "unsupported_required": self.required_failures.unsupported,
        })
    }

    fn fingerprint_errors(&self) -> Vec<String> {
        self.gates
            .iter()
            .filter_map(|gate| {
                let receipt = gate.receipt()?;
                match (
                    receipt.current_worktree_fingerprint_error.as_deref(),
                    receipt.receipt_worktree_fingerprint_error.as_deref(),
                ) {
                    (None, None) => None,
                    (Some(current), None) => {
                        Some(format!("{}: current={}", gate.id(), concise_error(current)))
                    }
                    (None, Some(receipt)) => {
                        Some(format!("{}: receipt={}", gate.id(), concise_error(receipt)))
                    }
                    (Some(current), Some(receipt)) => Some(format!(
                        "{}: current={}, receipt={}",
                        gate.id(),
                        concise_error(current),
                        concise_error(receipt)
                    )),
                }
            })
            .collect()
    }
}

#[derive(Default)]
struct RequiredGateFailures {
    missing: Vec<String>,
    failed: Vec<String>,
    stale: Vec<String>,
    unknown: Vec<String>,
    unsupported: Vec<String>,
}

impl RequiredGateFailures {
    fn is_empty(&self) -> bool {
        self.missing.is_empty()
            && self.failed.is_empty()
            && self.stale.is_empty()
            && self.unknown.is_empty()
            && self.unsupported.is_empty()
    }

    fn observe(&mut self, gate: &GateEvaluation) {
        if !gate.required() {
            return;
        }

        match gate.outcome() {
            GateOutcome::Passed | GateOutcome::Reused | GateOutcome::NotApplicable => {}
            GateOutcome::Missing => self.missing.push(gate.id().to_string()),
            GateOutcome::Failed | GateOutcome::InvalidOutput => {
                self.failed.push(gate.id().to_string());
            }
            GateOutcome::Stale => self.stale.push(gate.id().to_string()),
            GateOutcome::Unknown => self.unknown.push(gate.id().to_string()),
            GateOutcome::Unsupported => self.unsupported.push(gate.unsupported_label()),
        }
    }
}

pub(super) fn gates(ctx: &RepoContext, opts: WorkGatesRequest) -> Result<Value> {
    let plan_id = resolve_work_plan_id(ctx, opts.plan_id)?;
    Ok(gate_report(ctx, &plan_id)?.to_value())
}

pub(super) fn snapshot_with_cancellation(
    ctx: &RepoContext,
    plan_id: Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_gate_collection_active(cancelled)?;
    let plan_id = resolve_work_plan_id_with_cancellation(ctx, plan_id, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    Ok(gate_report_with_cancellation(ctx, &plan_id, cancelled)?.to_value())
}

pub(super) fn evidence(ctx: &RepoContext, opts: WorkEvidenceRequest) -> Result<Value> {
    let plan_id = resolve_work_plan_id(ctx, opts.plan_id)?;
    let report = gate_report(ctx, &plan_id)?;
    evidence_from_report(report)
}

pub(super) fn evidence_with_cancellation(
    ctx: &RepoContext,
    opts: WorkEvidenceRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value> {
    ensure_gate_collection_active(cancelled)?;
    let plan_id = resolve_work_plan_id_with_cancellation(ctx, opts.plan_id, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    let report = gate_report_with_cancellation(ctx, &plan_id, cancelled)?;
    evidence_from_report(report)
}

fn evidence_from_report(report: GateReport) -> Result<Value> {
    let latest = latest_passing_gates(&report);
    let mut status = report.to_value();
    let object = status
        .as_object_mut()
        .ok_or_else(|| anyhow!("work gate status was not a JSON object"))?;
    object.insert("command".into(), json!("work evidence"));
    object.insert("latest_passing_gates".into(), json!(latest));
    Ok(status)
}

pub(super) fn ensure_required_gates_passed_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    let report = gate_report_with_cancellation(ctx, plan_id, cancelled)?;
    if report.gates_ok() {
        return Ok(report
            .gates
            .iter()
            .any(GateEvaluation::required)
            .then(|| report.current_worktree_fingerprint.clone())
            .flatten());
    }

    let failures = &report.required_failures;
    let fingerprint_errors = report.fingerprint_errors();
    let fingerprint_error_details = if fingerprint_errors.is_empty() {
        String::new()
    } else {
        format!(" Fingerprint errors: [{}].", fingerprint_errors.join("; "))
    };

    bail!(
        "Required work gates are not satisfied for plan {plan_id}. Missing: [{}]. Failed: [{}]. Stale: [{}]. Unknown: [{}]. Unsupported: [{}].{} Run `scripts/jig work gates --plan-id {plan_id}` for details.",
        failures.missing.join(", "),
        failures.failed.join(", "),
        failures.stale.join(", "),
        failures.unknown.join(", "),
        failures.unsupported.join(", "),
        fingerprint_error_details,
    )
}

fn gate_report(ctx: &RepoContext, plan_id: &str) -> Result<GateReport> {
    let plan_state = resolve_plan_state(ctx, plan_id)?;
    evaluate_gate_report(
        ctx,
        plan_id,
        plan_state,
        current_worktree_fingerprint(ctx),
        GateCollection::Blocking,
    )
}

fn gate_report_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<GateReport> {
    ensure_gate_collection_active(cancelled)?;
    let plan_state = resolve_plan_state_with_cancellation(ctx, plan_id, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    let current_fingerprint = current_worktree_fingerprint_with_cancellation(ctx, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    evaluate_gate_report(
        ctx,
        plan_id,
        plan_state,
        current_fingerprint,
        GateCollection::Cancellable(cancelled),
    )
}

fn evaluate_gate_report(
    ctx: &RepoContext,
    plan_id: &str,
    plan_state: &'static str,
    current_fingerprint: crate::state::CurrentWorktreeFingerprint,
    collection: GateCollection<'_>,
) -> Result<GateReport> {
    collection.ensure_active()?;
    let work_gates = ctx.work_gates();
    let mut check_tools = BTreeSet::new();
    let mut review_gate_ids = BTreeSet::new();
    let mut evidence_targets = BTreeMap::new();
    let repository = repository_for_evidence_gates(ctx, &work_gates).ok();
    for gate in &work_gates {
        collection.ensure_active()?;
        match gate {
            WorkGate::Check(gate) => {
                if validate_check_tool(ctx, &gate.tool, "Work gate").is_ok() {
                    check_tools.insert(gate.tool.clone());
                }
            }
            WorkGate::CodexReview(gate) => {
                review_gate_ids.insert(gate.id.clone());
            }
            WorkGate::Evidence(gate) => {
                if let Some(repository) = &repository
                    && let Ok(targets) = resolve_evidence_targets(repository, &gate.selector)
                {
                    evidence_targets.insert(gate.id.clone(), targets);
                }
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    collection.ensure_active()?;
    let receipt_index = match collection {
        GateCollection::Blocking => work_gate_receipt_index(
            ctx,
            plan_id,
            &check_tools,
            &review_gate_ids,
            &evidence_targets,
        )?,
        GateCollection::Cancellable(cancelled) => work_gate_receipt_index_with_cancellation(
            ctx,
            plan_id,
            &check_tools,
            &review_gate_ids,
            &evidence_targets,
            cancelled,
        )?,
    };
    collection.ensure_active()?;

    evaluate_gate_report_from_index(
        ctx,
        GateReportPlanInput {
            plan_id,
            plan_state,
            prepared_scope: None,
        },
        current_fingerprint,
        work_gates,
        &receipt_index,
        collection,
    )
}

fn evaluate_gate_report_from_index(
    ctx: &RepoContext,
    plan: GateReportPlanInput<'_>,
    current_fingerprint: crate::state::CurrentWorktreeFingerprint,
    work_gates: Vec<WorkGate>,
    receipt_index: &WorkGateReceiptIndex,
    collection: GateCollection<'_>,
) -> Result<GateReport> {
    let GateReportPlanInput {
        plan_id,
        plan_state,
        prepared_scope,
    } = plan;
    let mut gates = Vec::new();
    let mut required_failures = RequiredGateFailures::default();
    let plan_scope = if let Some(plan_scope) = prepared_scope {
        plan_scope
    } else {
        match collection {
            GateCollection::Blocking => PlanGateContext::load(ctx, plan_id)?,
            GateCollection::Cancellable(cancelled) => {
                PlanGateContext::load_with_cancellation(ctx, plan_id, cancelled)?
            }
        }
    };
    plan_scope.seed_legacy_fingerprint(current_fingerprint.clone());

    for gate in work_gates {
        collection.ensure_active()?;
        let status = evaluate_gate(
            ctx,
            &plan_scope,
            &gate,
            &current_fingerprint,
            receipt_index,
            collection,
        )?;
        collection.ensure_active()?;
        required_failures.observe(&status);
        gates.push(status);
    }
    collection.ensure_active()?;

    Ok(GateReport {
        plan_id: plan_id.to_string(),
        plan_state,
        plan_baseline: plan_scope.baseline().cloned(),
        current_worktree_fingerprint: current_fingerprint.fingerprint,
        current_worktree_fingerprint_error: current_fingerprint.error,
        gates,
        required_failures,
    })
}

struct GateReportPlanInput<'a> {
    plan_id: &'a str,
    plan_state: &'static str,
    prepared_scope: Option<PlanGateContext>,
}

pub(super) fn open_plan_snapshots_with_cancellation(
    ctx: &RepoContext,
    plan_ids: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<BTreeMap<String, Value>> {
    ensure_gate_collection_active(cancelled)?;
    if plan_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let current_fingerprint = current_worktree_fingerprint_with_cancellation(ctx, cancelled)?;
    let work_gates = ctx.work_gates();
    let mut check_tools = BTreeSet::new();
    let mut review_gate_ids = BTreeSet::new();
    let mut evidence_targets = BTreeMap::new();
    let repository = repository_for_evidence_gates(ctx, &work_gates).ok();
    for gate in &work_gates {
        ensure_gate_collection_active(cancelled)?;
        match gate {
            WorkGate::Check(gate) => {
                if validate_check_tool(ctx, &gate.tool, "Work gate").is_ok() {
                    check_tools.insert(gate.tool.clone());
                }
            }
            WorkGate::CodexReview(gate) => {
                review_gate_ids.insert(gate.id.clone());
            }
            WorkGate::Evidence(gate) => {
                if let Some(repository) = &repository
                    && let Ok(targets) = resolve_evidence_targets(repository, &gate.selector)
                {
                    evidence_targets.insert(gate.id.clone(), targets);
                }
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    let plan_ids_set = plan_ids.iter().cloned().collect::<BTreeSet<_>>();
    let indexes = work_gate_receipt_indexes_with_cancellation(
        ctx,
        &plan_ids_set,
        &check_tools,
        &review_gate_ids,
        &evidence_targets,
        cancelled,
    )?;
    let baselines = plan_baselines_with_cancellation(ctx, &plan_ids_set, cancelled)?;
    let mut snapshots = BTreeMap::new();
    let mut plan_changes =
        BTreeMap::<String, Option<std::result::Result<Rc<PlanChangeSnapshot>, String>>>::new();
    for plan_id in plan_ids {
        ensure_gate_collection_active(cancelled)?;
        let index = indexes
            .get(plan_id)
            .expect("every requested open plan has a receipt index");
        let baseline = baselines.get(plan_id).cloned().flatten();
        let cache_key = baseline.as_ref().and_then(plan_change_cache_key);
        let prepared = if let Some(cache_key) = cache_key {
            if let Some(prepared) = plan_changes.get(&cache_key) {
                prepared.clone()
            } else {
                let prepared = PlanGateContext::prepare_plan_change_with_cancellation(
                    ctx, &baseline, cancelled,
                );
                plan_changes.insert(cache_key, prepared.clone());
                prepared
            }
        } else {
            None
        };
        let plan_scope = PlanGateContext::from_prepared(baseline, prepared);
        let report = evaluate_gate_report_from_index(
            ctx,
            GateReportPlanInput {
                plan_id,
                plan_state: "open",
                prepared_scope: Some(plan_scope),
            },
            current_fingerprint.clone(),
            work_gates.clone(),
            index,
            GateCollection::Cancellable(cancelled),
        )?;
        snapshots.insert(plan_id.clone(), report.to_value());
    }
    Ok(snapshots)
}

fn plan_change_cache_key(baseline: &PlanBaseline) -> Option<String> {
    baseline
        .commit_oid
        .as_deref()
        .map(|oid| format!("commit:{oid}"))
        .or_else(|| {
            baseline
                .empty_tree_oid
                .as_deref()
                .map(|oid| format!("empty-tree:{oid}"))
        })
}

include!("gates/tail.rs");

mod target_evidence;

fn repository_for_evidence_gates(
    ctx: &RepoContext,
    work_gates: &[WorkGate],
) -> Result<RepositoryCatalog> {
    if !work_gates
        .iter()
        .any(|gate| matches!(gate, WorkGate::Evidence(_)))
    {
        bail!("no evidence gates are configured");
    }
    RepositoryCatalog::from_context(ctx)
}
