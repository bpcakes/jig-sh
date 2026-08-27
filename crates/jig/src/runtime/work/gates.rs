use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::command::{WorkEvidenceRequest, WorkGatesRequest};
use crate::context::{RepoContext, WorkGate};
use crate::git_receipts::PlanChangeSnapshot;
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
            } else if let Some(error) = current.error.as_deref() {
                (
                    GateFreshness::Unknown,
                    format!("current gate scope could not be collected: {error}"),
                )
            } else if evidence.gate_signature != current.gate_signature {
                (
                    GateFreshness::Stale,
                    "gate policy or execution definition changed since the receipt".into(),
                )
            } else {
                match (
                    evidence.scope_fingerprint.as_deref(),
                    current.scope_fingerprint.as_deref(),
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
            current_worktree_fingerprint_error: current.error.clone(),
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
}

#[derive(Clone, Debug)]
enum GateEvaluation {
    Check(Box<CheckGateEvaluation>),
    CodexReview(Box<ReviewGateEvaluation>),
    Unsupported(UnsupportedGateEvaluation),
}

impl GateEvaluation {
    fn id(&self) -> &str {
        match self {
            Self::Check(gate) => &gate.id,
            Self::CodexReview(gate) => &gate.id,
            Self::Unsupported(gate) => &gate.id,
        }
    }

    const fn required(&self) -> bool {
        match self {
            Self::Check(gate) => gate.required,
            Self::CodexReview(gate) => gate.required,
            Self::Unsupported(gate) => gate.required,
        }
    }

    const fn outcome(&self) -> GateOutcome {
        match self {
            Self::Check(gate) => gate.outcome,
            Self::CodexReview(gate) => gate.outcome,
            Self::Unsupported(_) => GateOutcome::Unsupported,
        }
    }

    fn unsupported_label(&self) -> String {
        match self {
            Self::Unsupported(gate) => format!("{} (kind: {})", gate.id, gate.kind),
            _ => self.id().to_string(),
        }
    }

    fn receipt(&self) -> Option<&EvaluatedReceipt> {
        match self {
            Self::Check(gate) => Some(&gate.receipt),
            Self::CodexReview(gate) => Some(&gate.receipt),
            Self::Unsupported(_) => None,
        }
    }

    fn evidence_key(&self) -> Option<String> {
        match self {
            Self::Check(gate) => Some(format!("gate:{}", gate.id)),
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
                    "applicability": current.applicability.map(crate::git_receipts::GateApplicability::as_str),
                    "applicability_reason": current.reason,
                    "applicability_error": current.error,
                    "paths": gate.paths,
                    "paths_ignore": gate.paths_ignore,
                    "reuse": gate.reuse,
                    "forced": evidence.map(|evidence| evidence.forced),
                    "baseline_oid": current.baseline_oid,
                    "receipt_baseline_oid": evidence.and_then(|evidence| evidence.baseline_oid.as_deref()),
                    "gate_signature": current.gate_signature,
                    "receipt_gate_signature": evidence.map(|evidence| evidence.gate_signature.as_str()),
                    "scope_fingerprint": current.scope_fingerprint,
                    "receipt_scope_fingerprint": evidence.and_then(|evidence| evidence.scope_fingerprint.as_deref()),
                    "matching_paths": current.matching_paths,
                    "matching_path_count": current.matching_path_count,
                    "matching_paths_truncated": current.matching_paths_truncated,
                    "matching_paths_digest": current.matching_paths_digest,
                    "source_plan_id": evidence.and_then(|evidence| evidence.source_plan_id.as_deref()),
                    "source_batch_receipt_id": evidence.and_then(|evidence| evidence.source_batch_receipt_id.as_deref()),
                    "source_tool_receipt_id": evidence.and_then(|evidence| evidence.source_tool_receipt_id.as_deref()),
                })
            }
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
            Self::Unsupported(gate) => json!({
                "id": gate.id,
                "kind": gate.kind,
                "required": gate.required,
                "status": GateOutcome::Unsupported.as_str(),
            }),
        }
    }

    fn to_latest_evidence(&self) -> Option<Value> {
        let receipt = self.receipt()?;
        match self {
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
                    .applicability
                    .map(crate::git_receipts::GateApplicability::as_str)
            );
            value["applicability_reason"] = json!(gate.current_scope.reason);
            value["baseline_oid"] = json!(gate.current_scope.baseline_oid);
            value["matching_paths"] = json!(gate.current_scope.matching_paths);
            value["matching_path_count"] = json!(gate.current_scope.matching_path_count);
            value["matching_paths_truncated"] = json!(gate.current_scope.matching_paths_truncated);
            value["matching_paths_digest"] = json!(gate.current_scope.matching_paths_digest);
            value["scope_fingerprint"] = json!(gate.current_scope.scope_fingerprint);
            value["gate_signature"] = json!(gate.current_scope.gate_signature);
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
) -> Result<()> {
    let report = gate_report_with_cancellation(ctx, plan_id, cancelled)?;
    if report.gates_ok() {
        return Ok(());
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
    for gate in &work_gates {
        collection.ensure_active()?;
        match gate {
            WorkGate::Check(gate) => {
                validate_check_tool(ctx, &gate.tool, "Work gate")?;
                check_tools.insert(gate.tool.clone());
            }
            WorkGate::CodexReview(gate) => {
                review_gate_ids.insert(gate.id.clone());
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    collection.ensure_active()?;
    let receipt_index = match collection {
        GateCollection::Blocking => {
            work_gate_receipt_index(ctx, plan_id, &check_tools, &review_gate_ids)?
        }
        GateCollection::Cancellable(cancelled) => work_gate_receipt_index_with_cancellation(
            ctx,
            plan_id,
            &check_tools,
            &review_gate_ids,
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
    for gate in &work_gates {
        ensure_gate_collection_active(cancelled)?;
        match gate {
            WorkGate::Check(gate) => {
                validate_check_tool(ctx, &gate.tool, "Work gate")?;
                check_tools.insert(gate.tool.clone());
            }
            WorkGate::CodexReview(gate) => {
                review_gate_ids.insert(gate.id.clone());
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

fn resolve_plan_state(ctx: &RepoContext, plan_id: &str) -> Result<&'static str> {
    Ok(match plan_status(ctx, plan_id)? {
        Some(PlanStatus::Open) => "open",
        Some(PlanStatus::Closed) => "closed",
        None => bail!("Plan not found: {plan_id}"),
    })
}

fn resolve_plan_state_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<&'static str> {
    Ok(
        match plan_status_with_cancellation(ctx, plan_id, cancelled)? {
            Some(PlanStatus::Open) => "open",
            Some(PlanStatus::Closed) => "closed",
            None => bail!("Plan not found: {plan_id}"),
        },
    )
}

fn resolve_work_plan_id(ctx: &RepoContext, requested: Option<String>) -> Result<String> {
    if let Some(plan_id) = requested {
        ensure_plan_exists(ctx, &plan_id)?;
        return Ok(plan_id);
    }

    let open_plans = open_plan_summaries(ctx)?;
    resolve_open_plan_id(&open_plans)
}

fn resolve_work_plan_id_with_cancellation(
    ctx: &RepoContext,
    requested: Option<String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    ensure_gate_collection_active(cancelled)?;
    if let Some(plan_id) = requested {
        ensure_plan_exists_with_cancellation(ctx, &plan_id, cancelled)?;
        ensure_gate_collection_active(cancelled)?;
        return Ok(plan_id);
    }

    let open_plans = open_plan_summaries_with_cancellation(ctx, cancelled)?;
    ensure_gate_collection_active(cancelled)?;
    resolve_open_plan_id(&open_plans)
}

fn resolve_open_plan_id(open_plans: &[Value]) -> Result<String> {
    match open_plans {
        [plan] => plan["plan_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Open plan summary did not include a plan id")),
        [] => bail!(
            "No open work plans. Run `scripts/jig work status` to find recent plan ids, then pass --plan-id to inspect a closed or specific plan."
        ),
        _ => bail!("Multiple open work plans. Pass --plan-id to choose which plan to inspect."),
    }
}

fn ensure_gate_collection_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

fn latest_passing_gates(report: &GateReport) -> Vec<Value> {
    let mut latest = BTreeMap::<String, (u64, Value)>::new();
    for gate in &report.gates {
        let Some(evidence_key) = gate.evidence_key() else {
            continue;
        };
        let Some(value) = gate.to_latest_evidence() else {
            continue;
        };
        let gate_id = gate.id();
        let ended_at_ms = gate
            .receipt()
            .and_then(|receipt| receipt.ended_at_ms)
            .unwrap_or(0);
        match latest.get(&evidence_key) {
            Some((existing_ended_at_ms, _)) if *existing_ended_at_ms > ended_at_ms => {}
            Some((existing_ended_at_ms, existing))
                if *existing_ended_at_ms == ended_at_ms
                    && existing["gate_id"].as_str().unwrap_or("") >= gate_id => {}
            // Replace when this receipt is newer, or when the timestamp ties
            // and the gate id sorts after the current winner.
            _ => {
                latest.insert(evidence_key, (ended_at_ms, value));
            }
        }
    }
    latest.into_values().map(|(_, value)| value).collect()
}

fn evaluate_gate(
    ctx: &RepoContext,
    plan_scope: &PlanGateContext,
    gate: &WorkGate,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    receipt_index: &WorkGateReceiptIndex,
    collection: GateCollection<'_>,
) -> Result<GateEvaluation> {
    collection.ensure_active()?;
    match gate {
        WorkGate::Check(gate) => {
            let tool_name = gate.tool.as_str();
            let current_scope = match collection {
                GateCollection::Blocking => plan_scope.evaluate(ctx, gate),
                GateCollection::Cancellable(cancelled) => {
                    plan_scope.evaluate_with_cancellation(ctx, gate, cancelled)
                }
            };
            if let Some(scoped_status) = receipt_index.check_gate_receipt(&gate.id).cloned() {
                let evaluated_receipt = EvaluatedReceipt::scoped(&scoped_status, &current_scope);
                let outcome = match scoped_status.evidence.status.as_str() {
                    "executed" => GateOutcome::Passed,
                    "reused" => GateOutcome::Reused,
                    "not_applicable" => GateOutcome::NotApplicable,
                    "unknown" => GateOutcome::Unknown,
                    _ => GateOutcome::Failed,
                }
                .with_freshness(evaluated_receipt.freshness);
                return Ok(GateEvaluation::Check(Box::new(CheckGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    tool: tool_name.to_string(),
                    paths: gate.paths.clone(),
                    paths_ignore: gate.paths_ignore.clone(),
                    reuse: gate.reuse,
                    outcome,
                    receipt: evaluated_receipt,
                    evidence: Some(scoped_status.evidence),
                    current_scope,
                })));
            }
            if gate.paths.is_some() || gate.reuse {
                let mut evaluated_receipt =
                    EvaluatedReceipt::new::<ToolReceiptStatus>(None, None, current_fingerprint);
                let outcome = if let Some(error) = current_scope.error.as_deref() {
                    evaluated_receipt.freshness = GateFreshness::Unknown;
                    evaluated_receipt.freshness_reason =
                        format!("gate applicability could not be determined: {error}");
                    evaluated_receipt.current_worktree_fingerprint_error = Some(error.to_string());
                    GateOutcome::Unknown
                } else {
                    GateOutcome::Missing
                };
                return Ok(GateEvaluation::Check(Box::new(CheckGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    tool: tool_name.to_string(),
                    paths: gate.paths.clone(),
                    paths_ignore: gate.paths_ignore.clone(),
                    reuse: gate.reuse,
                    outcome,
                    receipt: evaluated_receipt,
                    evidence: None,
                    current_scope,
                })));
            }
            collection.ensure_active()?;
            let receipt = receipt_index.tool_receipt(tool_name).cloned();
            collection.ensure_active()?;
            let freshness_receipt = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => {
                    // Freshness is anchored to the batch work-check receipt
                    // when available, since that receipt captures the
                    // before/after worktree fingerprint for the gate run.
                    let latest = receipt_index
                        .work_check_receipt(tool_name, &receipt.receipt_id)
                        .cloned();
                    collection.ensure_active()?;
                    latest.or_else(|| Some(receipt.clone()))
                }
                _ => receipt.clone(),
            };
            let evaluated_receipt = EvaluatedReceipt::new(
                receipt.as_ref(),
                freshness_receipt.as_ref(),
                current_fingerprint,
            );
            let outcome = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => GateOutcome::Passed,
                Some(_) => GateOutcome::Failed,
                None => GateOutcome::Missing,
            };

            Ok(GateEvaluation::Check(Box::new(CheckGateEvaluation {
                id: gate.id.clone(),
                required: gate.required,
                tool: tool_name.to_string(),
                paths: gate.paths.clone(),
                paths_ignore: gate.paths_ignore.clone(),
                reuse: gate.reuse,
                outcome: outcome.with_freshness(evaluated_receipt.freshness),
                receipt: evaluated_receipt,
                evidence: None,
                current_scope,
            })))
        }
        WorkGate::CodexReview(gate) => {
            let skill = gate.skill.as_str();
            collection.ensure_active()?;
            let receipt = receipt_index.review_receipt(&gate.id).cloned();
            collection.ensure_active()?;
            let evidence = receipt
                .as_ref()
                .and_then(|receipt| receipt.evidence.as_ref());
            let evidence_status = evidence.and_then(|evidence| evidence.status.as_deref());
            let outcome = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => GateOutcome::Passed,
                Some(_) if evidence_status == Some("invalid_output") => GateOutcome::InvalidOutput,
                Some(_) => GateOutcome::Failed,
                None => GateOutcome::Missing,
            };
            let evaluated_receipt =
                EvaluatedReceipt::new(receipt.as_ref(), receipt.as_ref(), current_fingerprint);
            Ok(GateEvaluation::CodexReview(Box::new(
                ReviewGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    skill: skill.to_string(),
                    outcome: outcome.with_freshness(evaluated_receipt.freshness),
                    receipt: evaluated_receipt,
                    evidence: evidence.cloned(),
                },
            )))
        }
        WorkGate::Unsupported(gate) => Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
            id: gate.id.clone(),
            required: gate.required,
            kind: gate.kind.clone(),
        })),
    }
}

trait GateReceiptView {
    fn receipt_id(&self) -> &str;
    fn exit_status(&self) -> i32;
    fn ended_at_ms(&self) -> u64;
    fn changed_paths(&self) -> &[String];
    fn changed_path_count(&self) -> usize;
    fn changed_paths_truncated(&self) -> bool;
    fn changed_paths_digest(&self) -> Option<&str>;
    fn diff_summary(&self) -> &str;
    fn worktree_fingerprint(&self) -> Option<&str>;
    fn worktree_fingerprint_error(&self) -> Option<&str>;
}

impl GateReceiptView for ToolReceiptStatus {
    fn receipt_id(&self) -> &str {
        &self.receipt_id
    }

    fn exit_status(&self) -> i32 {
        self.exit_status
    }

    fn ended_at_ms(&self) -> u64 {
        self.ended_at_ms
    }

    fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }

    fn changed_path_count(&self) -> usize {
        self.changed_path_count
    }

    fn changed_paths_truncated(&self) -> bool {
        self.changed_paths_truncated
    }

    fn changed_paths_digest(&self) -> Option<&str> {
        self.changed_paths_digest.as_deref()
    }

    fn diff_summary(&self) -> &str {
        &self.diff_summary
    }

    fn worktree_fingerprint(&self) -> Option<&str> {
        self.worktree_fingerprint.as_deref()
    }

    fn worktree_fingerprint_error(&self) -> Option<&str> {
        self.worktree_fingerprint_error.as_deref()
    }
}

impl GateReceiptView for WorkReviewReceiptStatus {
    fn receipt_id(&self) -> &str {
        &self.receipt_id
    }

    fn exit_status(&self) -> i32 {
        self.exit_status
    }

    fn ended_at_ms(&self) -> u64 {
        self.ended_at_ms
    }

    fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }

    fn changed_path_count(&self) -> usize {
        self.changed_path_count
    }

    fn changed_paths_truncated(&self) -> bool {
        self.changed_paths_truncated
    }

    fn changed_paths_digest(&self) -> Option<&str> {
        self.changed_paths_digest.as_deref()
    }

    fn diff_summary(&self) -> &str {
        &self.diff_summary
    }

    fn worktree_fingerprint(&self) -> Option<&str> {
        self.worktree_fingerprint.as_deref()
    }

    fn worktree_fingerprint_error(&self) -> Option<&str> {
        self.worktree_fingerprint_error.as_deref()
    }
}

fn gate_changed_paths<T: GateReceiptView>(
    receipt: Option<&T>,
) -> (Vec<String>, usize, bool, Option<String>) {
    let Some(receipt) = receipt else {
        return (Vec::new(), 0, false, None);
    };
    let total = receipt.changed_path_count();
    let paths = receipt
        .changed_paths()
        .iter()
        .take(MAX_GATE_CHANGED_PATHS)
        .cloned()
        .collect::<Vec<_>>();
    (
        paths,
        total,
        receipt.changed_paths_truncated() || total > MAX_GATE_CHANGED_PATHS,
        receipt.changed_paths_digest().map(str::to_string),
    )
}

fn gate_freshness<T: GateReceiptView>(
    receipt: Option<&T>,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
) -> GateFreshness {
    let Some(receipt) = receipt else {
        return GateFreshness::Missing;
    };
    let Some(receipt_fingerprint) = receipt.worktree_fingerprint() else {
        return GateFreshness::Unknown;
    };
    let Some(current_fingerprint) = current_fingerprint.fingerprint.as_deref() else {
        return GateFreshness::Unknown;
    };
    if receipt_fingerprint == current_fingerprint {
        GateFreshness::Fresh
    } else {
        GateFreshness::Stale
    }
}

fn concise_error(error: &str) -> String {
    let one_line = error.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_ERROR_CHARS: usize = 240;
    if one_line.chars().count() <= MAX_ERROR_CHARS {
        return one_line;
    }

    let mut truncated = one_line
        .chars()
        .take(MAX_ERROR_CHARS.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::{
        CheckGateEvaluation, EvaluatedReceipt, GateEvaluation, GateFreshness, GateOutcome,
        GateReport, GateScopeEvaluation, RequiredGateFailures, concise_error, latest_passing_gates,
    };

    fn passing_check(id: &str, receipt_id: &str) -> GateEvaluation {
        GateEvaluation::Check(Box::new(CheckGateEvaluation {
            id: id.to_string(),
            required: true,
            tool: "jig.test".into(),
            paths: None,
            paths_ignore: Vec::new(),
            reuse: false,
            outcome: GateOutcome::Passed,
            evidence: None,
            receipt: EvaluatedReceipt {
                receipt_id: Some(receipt_id.to_string()),
                freshness_receipt_id: Some(receipt_id.to_string()),
                exit_status: Some(0),
                ended_at_ms: Some(42),
                freshness: GateFreshness::Fresh,
                freshness_reason: "receipt matches current worktree fingerprint".into(),
                changed_paths: Vec::new(),
                changed_path_count: 0,
                changed_paths_truncated: false,
                changed_paths_digest: None,
                diff_summary: None,
                receipt_worktree_fingerprint_error: None,
                current_worktree_fingerprint_error: None,
            },
            current_scope: GateScopeEvaluation {
                gate_signature: "signature".into(),
                baseline_oid: None,
                applicability: Some(crate::git_receipts::GateApplicability::Applicable),
                reason: "always applicable".into(),
                changed_paths: Vec::new(),
                changed_path_count: 0,
                changed_paths_truncated: false,
                changed_paths_digest: None,
                matching_paths: Vec::new(),
                matching_path_count: 0,
                matching_paths_truncated: false,
                matching_paths_digest: None,
                scope_fingerprint: Some("fingerprint".into()),
                error: None,
            },
        }))
    }

    #[test]
    fn concise_error_reserves_room_for_ellipsis() {
        let error = "x".repeat(300);
        let concise = concise_error(&error);

        assert_eq!(concise.chars().count(), 240);
        assert!(concise.ends_with("..."));
    }

    #[test]
    fn latest_passing_gates_keeps_distinct_gate_identities() {
        let report = GateReport {
            plan_id: "plan-test".into(),
            plan_state: "open",
            plan_baseline: None,
            current_worktree_fingerprint: Some("fingerprint".into()),
            current_worktree_fingerprint_error: None,
            gates: vec![
                passing_check("alpha", "receipt-alpha"),
                passing_check("zeta", "receipt-zeta"),
            ],
            required_failures: RequiredGateFailures::default(),
        };

        let latest = latest_passing_gates(&report);

        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0]["gate_id"], "alpha");
        assert_eq!(latest[0]["receipt_id"], "receipt-alpha");
        assert_eq!(latest[1]["gate_id"], "zeta");
        assert_eq!(latest[1]["receipt_id"], "receipt-zeta");
    }

    #[test]
    fn gate_outcome_uses_closed_freshness_mapping() {
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Fresh)
                .as_str(),
            "passed"
        );
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Missing)
                .as_str(),
            "missing"
        );
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Stale)
                .as_str(),
            "stale"
        );
        assert_eq!(
            GateOutcome::Passed
                .with_freshness(GateFreshness::Unknown)
                .as_str(),
            "unknown"
        );
        assert_eq!(
            GateOutcome::Failed
                .with_freshness(GateFreshness::Unknown)
                .as_str(),
            "failed"
        );
    }
}
