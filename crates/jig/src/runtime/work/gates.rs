use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cancellation::ensure_status_collection_active;
use crate::command::{WorkEvidenceRequest, WorkGatesRequest};
use crate::context::{RepoContext, WorkGate};
use crate::repository::{RepositoryCatalog, resolve_evidence_targets};
use crate::state::{
    PlanStatus, ToolReceiptStatus, WorkGateReceiptIndex, WorkReviewReceiptEvidence,
    WorkReviewReceiptStatus, current_worktree_fingerprint,
    current_worktree_fingerprint_with_cancellation, ensure_plan_exists,
    ensure_plan_exists_with_cancellation, open_plan_summaries,
    open_plan_summaries_with_cancellation, plan_status, plan_status_with_cancellation,
    work_gate_receipt_index, work_gate_receipt_index_with_cancellation,
    work_gate_receipt_indexes_with_cancellation,
};

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
            Self::Missing => "missing",
            Self::Failed => "failed",
            Self::InvalidOutput => "invalid_output",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
        }
    }

    const fn with_freshness(self, freshness: GateFreshness) -> Self {
        if matches!(self, Self::Passed) {
            freshness.as_gate_outcome()
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
            .to_owned();
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
}

#[derive(Clone, Debug)]
struct CheckGateEvaluation {
    id: String,
    required: bool,
    tool: String,
    outcome: GateOutcome,
    receipt: EvaluatedReceipt,
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
    Check(CheckGateEvaluation),
    Evidence(EvidenceGateEvaluation),
    CodexReview(ReviewGateEvaluation),
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
            Self::Check(gate) => Some(format!("tool:{}", gate.tool)),
            Self::Evidence(gate) => Some(gate.evidence_key()),
            Self::CodexReview(gate) => Some(format!("gate:{}", gate.id)),
            Self::Unsupported(_) => None,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Check(gate) => {
                let receipt = &gate.receipt;
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
        if let Self::Evidence(gate) = self {
            return gate.to_latest_evidence();
        }

        let receipt = self.receipt()?;
        if receipt.exit_status != Some(0) {
            return None;
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
        Some(json!({
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
        }))
    }
}

struct GateReport {
    plan_id: String,
    plan_state: &'static str,
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
            GateOutcome::Passed => {}
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

#[derive(Default)]
struct WorkGateDependencies {
    check_tools: BTreeSet<String>,
    review_gate_ids: BTreeSet<String>,
    evidence_targets: BTreeMap<String, BTreeSet<jig_contract::TargetId>>,
    resolution_errors: BTreeMap<String, String>,
}

struct GateEvaluationInputs<'a> {
    repository: Option<&'a RepositoryCatalog>,
    receipt_index: &'a WorkGateReceiptIndex,
    resolution_errors: &'a BTreeMap<String, String>,
}

fn collect_work_gate_dependencies(
    ctx: &RepoContext,
    work_gates: &[WorkGate],
    repository: Option<&RepositoryCatalog>,
    repository_error: Option<&str>,
    collection: GateCollection<'_>,
) -> Result<WorkGateDependencies> {
    let mut dependencies = WorkGateDependencies::default();
    for gate in work_gates {
        collection.ensure_active()?;
        match gate {
            WorkGate::Check(gate) => match validate_check_tool(ctx, &gate.tool, "Work gate") {
                Ok(()) => {
                    dependencies.check_tools.insert(gate.tool.clone());
                }
                Err(error) => {
                    dependencies
                        .resolution_errors
                        .insert(gate.id.clone(), format!("{error:#}"));
                }
            },
            WorkGate::Evidence(gate) => {
                if let Some(error) = repository_error {
                    dependencies
                        .resolution_errors
                        .insert(gate.id.clone(), error.to_owned());
                    continue;
                }
                let catalog = repository.expect("evidence gates initialize the repository catalog");
                match resolve_evidence_targets(catalog, &gate.selector) {
                    Ok(targets) => {
                        dependencies
                            .evidence_targets
                            .insert(gate.id.clone(), targets);
                    }
                    Err(error) => {
                        // A renamed target or profile is a broken gate, not a
                        // reason for read-only status inspection to disappear.
                        // Execution paths still reject the selector before
                        // running any target.
                        dependencies
                            .resolution_errors
                            .insert(gate.id.clone(), format!("{error:#}"));
                    }
                }
            }
            WorkGate::CodexReview(gate) => {
                dependencies.review_gate_ids.insert(gate.id.clone());
            }
            WorkGate::Unsupported(_) => {}
        }
    }
    Ok(dependencies)
}

fn repository_for_gate_status(
    ctx: &RepoContext,
    work_gates: &[WorkGate],
) -> (Option<RepositoryCatalog>, Option<String>) {
    if !work_gates
        .iter()
        .any(|gate| matches!(gate, WorkGate::Evidence(_)))
    {
        return (None, None);
    }
    match RepositoryCatalog::from_context(ctx) {
        Ok(catalog) => (Some(catalog), None),
        Err(error) => (
            None,
            Some(format!("repository catalog is invalid: {error:#}")),
        ),
    }
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
    let (repository, repository_error) = repository_for_gate_status(ctx, &work_gates);
    let dependencies = collect_work_gate_dependencies(
        ctx,
        &work_gates,
        repository.as_ref(),
        repository_error.as_deref(),
        collection,
    )?;
    collection.ensure_active()?;
    let receipt_index = match collection {
        GateCollection::Blocking => work_gate_receipt_index(
            ctx,
            plan_id,
            &dependencies.check_tools,
            &dependencies.review_gate_ids,
            &dependencies.evidence_targets,
        )?,
        GateCollection::Cancellable(cancelled) => work_gate_receipt_index_with_cancellation(
            ctx,
            plan_id,
            &dependencies.check_tools,
            &dependencies.review_gate_ids,
            &dependencies.evidence_targets,
            cancelled,
        )?,
    };
    collection.ensure_active()?;

    evaluate_gate_report_from_index(
        plan_id,
        plan_state,
        current_fingerprint,
        work_gates,
        GateEvaluationInputs {
            repository: repository.as_ref(),
            receipt_index: &receipt_index,
            resolution_errors: &dependencies.resolution_errors,
        },
        collection,
    )
}

fn evaluate_gate_report_from_index(
    plan_id: &str,
    plan_state: &'static str,
    current_fingerprint: crate::state::CurrentWorktreeFingerprint,
    work_gates: Vec<WorkGate>,
    inputs: GateEvaluationInputs<'_>,
    collection: GateCollection<'_>,
) -> Result<GateReport> {
    let mut gates = Vec::new();
    let mut required_failures = RequiredGateFailures::default();

    for gate in work_gates {
        collection.ensure_active()?;
        let status = evaluate_gate(&gate, &current_fingerprint, &inputs, collection)?;
        collection.ensure_active()?;
        required_failures.observe(&status);
        gates.push(status);
    }
    collection.ensure_active()?;

    Ok(GateReport {
        plan_id: plan_id.to_string(),
        plan_state,
        current_worktree_fingerprint: current_fingerprint.fingerprint,
        current_worktree_fingerprint_error: current_fingerprint.error,
        gates,
        required_failures,
    })
}

mod resolution;
pub(super) use resolution::open_plan_snapshots_with_cancellation;
use resolution::*;

mod target_evidence;
#[cfg(test)]
mod tests;
