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
};

use super::tools::validate_check_tool;

mod target_evidence;

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
            Self::Unsupported(gate) => format!("{} (kind: {})", gate.id, gate.kind),
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
            Self::Unsupported(gate) => json!({
                "id": gate.id,
                "kind": gate.kind,
                "required": gate.required,
                "status": GateOutcome::Unsupported.as_str(),
            }),
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
    let latest = latest_passing_gates(&report);
    let mut status = report.to_value();
    let object = status
        .as_object_mut()
        .ok_or_else(|| anyhow!("work gate status was not a JSON object"))?;
    object.insert("command".into(), json!("work evidence"));
    object.insert("latest_passing_gates".into(), json!(latest));
    Ok(status)
}

pub(super) fn ensure_required_gates_passed(ctx: &RepoContext, plan_id: &str) -> Result<()> {
    let report = gate_report(ctx, plan_id)?;
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
    let repository = work_gates
        .iter()
        .any(|gate| matches!(gate, WorkGate::Evidence(_)))
        .then(|| RepositoryCatalog::from_context(ctx))
        .transpose()?;
    let mut check_tools = BTreeSet::new();
    let mut review_gate_ids = BTreeSet::new();
    let mut evidence_targets = BTreeMap::new();
    for gate in &work_gates {
        collection.ensure_active()?;
        match gate {
            WorkGate::Check(gate) => {
                validate_check_tool(ctx, &gate.tool, "Work gate")?;
                check_tools.insert(gate.tool.clone());
            }
            WorkGate::Evidence(gate) => {
                let catalog = repository
                    .as_ref()
                    .expect("evidence gates initialize the repository catalog");
                evidence_targets.insert(
                    gate.id.clone(),
                    resolve_evidence_targets(catalog, &gate.selector)?,
                );
            }
            WorkGate::CodexReview(gate) => {
                review_gate_ids.insert(gate.id.clone());
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

    let mut gates = Vec::new();
    let mut required_failures = RequiredGateFailures::default();

    for gate in work_gates {
        collection.ensure_active()?;
        let status = evaluate_gate(
            &gate,
            repository.as_ref(),
            &current_fingerprint,
            &receipt_index,
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
        current_worktree_fingerprint: current_fingerprint.fingerprint,
        current_worktree_fingerprint_error: current_fingerprint.error,
        gates,
        required_failures,
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
    gate: &WorkGate,
    repository: Option<&RepositoryCatalog>,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    receipt_index: &WorkGateReceiptIndex,
    collection: GateCollection<'_>,
) -> Result<GateEvaluation> {
    collection.ensure_active()?;
    match gate {
        WorkGate::Check(gate) => {
            let tool_name = gate.tool.as_str();
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

            Ok(GateEvaluation::Check(CheckGateEvaluation {
                id: gate.id.clone(),
                required: gate.required,
                tool: tool_name.to_string(),
                outcome: outcome.with_freshness(evaluated_receipt.freshness),
                receipt: evaluated_receipt,
            }))
        }
        WorkGate::Evidence(gate) => {
            let catalog = repository
                .ok_or_else(|| anyhow!("repository catalog was not loaded for an evidence gate"))?;
            Ok(GateEvaluation::Evidence(EvidenceGateEvaluation::evaluate(
                gate,
                catalog,
                current_fingerprint,
                receipt_index,
                collection,
            )?))
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
            Ok(GateEvaluation::CodexReview(ReviewGateEvaluation {
                id: gate.id.clone(),
                required: gate.required,
                skill: skill.to_string(),
                outcome: outcome.with_freshness(evaluated_receipt.freshness),
                receipt: evaluated_receipt,
                evidence: evidence.cloned(),
            }))
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
mod tests;
