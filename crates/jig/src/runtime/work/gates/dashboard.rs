use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use anyhow::{Result as AnyResult, anyhow};

use super::PlanGateContext;
use super::{
    GateCollection, GateEvaluation, GateReport, PlanBaseline, ensure_gate_collection_active,
    evaluate_gate_report_from_index, plan_change_cache_key, repository_for_evidence_gates,
    validate_check_tool,
};
use crate::context::{RepoContext, WorkGate};
use crate::git_receipts::PlanChangeSnapshot;
use crate::repository::resolve_evidence_targets;
use crate::state::WorkReviewReceiptEvidence;
use crate::state::{
    WorkGateReceiptIndex, WorkGateReceiptIndexes, current_worktree_fingerprint_with_cancellation,
};

pub(super) struct GateReportPlanInput<'a> {
    pub(super) plan_id: &'a str,
    pub(super) plan_state: &'static str,
    pub(super) prepared_scope: Option<PlanGateContext>,
}

pub(crate) fn gate_receipt_indexes(
    ctx: &RepoContext,
    plan_ids: &[String],
    cancelled: &dyn Fn() -> bool,
) -> AnyResult<WorkGateReceiptIndexes> {
    ensure_gate_collection_active(cancelled)?;
    let work_gates = ctx.work_gates();
    let mut check_tools = BTreeSet::new();
    let mut review_gate_ids = BTreeSet::new();
    let mut evidence_targets = BTreeMap::new();
    let repository = repository_for_evidence_gates(ctx, &work_gates).ok();
    ensure_gate_collection_active(cancelled)?;
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
    ensure_gate_collection_active(cancelled)?;
    Ok(WorkGateReceiptIndexes::new(
        &plan_ids.iter().cloned().collect(),
        &check_tools,
        &review_gate_ids,
        &evidence_targets,
    ))
}

pub(crate) fn open_plan_reports_with_cancellation(
    ctx: &RepoContext,
    baselines: &BTreeMap<String, Option<PlanBaseline>>,
    indexes: BTreeMap<String, WorkGateReceiptIndex>,
    plan_state: &'static str,
    cancelled: &dyn Fn() -> bool,
) -> AnyResult<BTreeMap<String, DashboardGateReport>> {
    ensure_gate_collection_active(cancelled)?;
    if baselines.is_empty() {
        return Ok(BTreeMap::new());
    }
    let current_fingerprint = current_worktree_fingerprint_with_cancellation(ctx, cancelled)?;
    let work_gates = ctx.work_gates();
    let mut snapshots = BTreeMap::new();
    let mut plan_changes =
        BTreeMap::<String, Option<std::result::Result<Rc<PlanChangeSnapshot>, String>>>::new();
    for (plan_id, baseline) in baselines {
        ensure_gate_collection_active(cancelled)?;
        let index = indexes
            .get(plan_id)
            .ok_or_else(|| anyhow!("dashboard receipt reducer omitted plan {plan_id}"))?;
        let cache_key = baseline.as_ref().and_then(plan_change_cache_key);
        let prepared = if let Some(cache_key) = cache_key {
            if let Some(prepared) = plan_changes.get(&cache_key) {
                prepared.clone()
            } else {
                let prepared = PlanGateContext::prepare_plan_change_with_cancellation(
                    ctx, baseline, cancelled,
                );
                plan_changes.insert(cache_key, prepared.clone());
                prepared
            }
        } else {
            None
        };
        let plan_scope = PlanGateContext::from_prepared(baseline.clone(), prepared);
        let report = evaluate_gate_report_from_index(
            ctx,
            GateReportPlanInput {
                plan_id,
                plan_state,
                prepared_scope: Some(plan_scope),
            },
            current_fingerprint.clone(),
            work_gates.clone(),
            index,
            GateCollection::Cancellable(cancelled),
        )?;
        snapshots.insert(plan_id.clone(), DashboardGateReport(report));
    }
    ensure_gate_collection_active(cancelled)?;
    Ok(snapshots)
}

pub(crate) struct DashboardGateReport(pub(super) GateReport);

impl DashboardGateReport {
    pub(crate) fn status_view(&self) -> jig_ui::dashboard::StatusGateReport {
        self.0.status_view()
    }

    pub(crate) fn recorder_view(
        &self,
    ) -> Result<jig_ui::dashboard::GatesObservation, jig_ui::dashboard::SourceError> {
        use jig_ui::dashboard::{BoundedRows, GatesObservation, LimitId};

        let report = self.status_view();
        if report.gates.len() != self.0.gates.len() {
            return Err(jig_ui::dashboard::SourceError::InternalContract {
                message: "status and recorder gate projections have different cardinality"
                    .to_string(),
            });
        }
        let total = report.gates.len();
        let gates = report
            .gates
            .iter()
            .zip(&self.0.gates)
            .take(LimitId::GateRows.ceiling())
            .map(|(status, evaluation)| recorder_gate(status, evaluation, &report.plan_id))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(GatesObservation {
            overall: report.overall,
            gates: BoundedRows::for_limit(gates, Some(total), LimitId::GateRows)
                .map_err(limit_error)?,
        })
    }
}

impl GateReport {
    fn status_view(&self) -> jig_ui::dashboard::StatusGateReport {
        jig_ui::dashboard::StatusGateReport {
            ok: true,
            gates_ok: self.gates_ok(),
            plan_id: self.plan_id.clone(),
            plan_state: self.plan_state.to_string(),
            plan_baseline: self.plan_baseline.clone().map(status_plan_baseline),
            current_worktree_fingerprint: self.current_worktree_fingerprint.clone(),
            current_worktree_fingerprint_error: self.current_worktree_fingerprint_error.clone(),
            gates: self.gates.iter().map(GateEvaluation::status_view).collect(),
            missing_required: self.required_failures.missing.clone(),
            failed_required: self.required_failures.failed.clone(),
            stale_required: self.required_failures.stale.clone(),
            unknown_required: self.required_failures.unknown.clone(),
            unsupported_required: self.required_failures.unsupported.clone(),
            overall: if self.gates_ok() { "passed" } else { "blocked" }.to_string(),
        }
    }
}

fn status_plan_baseline(value: PlanBaseline) -> jig_ui::dashboard::StatusPlanBaseline {
    jig_ui::dashboard::StatusPlanBaseline {
        requested_ref: value.requested_ref,
        commit_oid: value.commit_oid,
        empty_tree_oid: value.empty_tree_oid,
        error: value.error,
    }
}

impl GateEvaluation {
    fn status_view(&self) -> jig_ui::dashboard::StatusGate {
        use jig_ui::dashboard::{
            StatusCheckGate, StatusCodexReviewGate, StatusGate, StatusUnsupportedGate,
        };
        match self {
            Self::Check(gate) => {
                let receipt = &gate.receipt;
                let evidence = gate.evidence.as_ref();
                let current = &gate.current_scope;
                StatusGate::Check(Box::new(StatusCheckGate {
                    id: gate.id.clone(),
                    required: gate.required,
                    tool: gate.tool.clone(),
                    status: gate.outcome.as_str().to_string(),
                    receipt_id: receipt.receipt_id.clone(),
                    freshness_receipt_id: receipt.freshness_receipt_id.clone(),
                    exit_status: receipt.exit_status,
                    ended_at_ms: receipt.ended_at_ms,
                    freshness: receipt.freshness.as_str().to_string(),
                    freshness_reason: receipt.freshness_reason.clone(),
                    changed_paths: receipt.changed_paths.clone(),
                    changed_path_count: receipt.changed_path_count,
                    changed_paths_truncated: receipt.changed_paths_truncated,
                    changed_paths_digest: receipt.changed_paths_digest.clone(),
                    diff_summary: receipt.diff_summary.clone(),
                    receipt_worktree_fingerprint_error: receipt
                        .receipt_worktree_fingerprint_error
                        .clone(),
                    current_worktree_fingerprint_error: receipt
                        .current_worktree_fingerprint_error
                        .clone(),
                    evidence_status: evidence.map(|value| value.status.as_str().to_string()),
                    receipt_applicability: evidence
                        .map(|value| value.applicability.as_str().to_string()),
                    applicability: current
                        .applicability()
                        .map(|value| value.as_str().to_string()),
                    applicability_reason: Some(current.reason().to_string()),
                    applicability_error: current.error().map(str::to_string),
                    paths: gate.paths.clone(),
                    paths_ignore: gate.paths_ignore.clone(),
                    reuse: gate.reuse,
                    forced: evidence.map(|value| value.forced),
                    baseline_oid: current.baseline_oid().map(str::to_string),
                    receipt_baseline_oid: evidence.and_then(|value| value.baseline_oid.clone()),
                    gate_signature: Some(current.gate_signature().to_string()),
                    receipt_gate_signature: evidence.map(|value| value.gate_signature.clone()),
                    scope_fingerprint: current.scope_fingerprint().map(str::to_string),
                    receipt_scope_fingerprint: evidence
                        .and_then(|value| value.scope_fingerprint.clone()),
                    matching_paths: current.matching_paths().to_vec(),
                    matching_path_count: current.matching_path_count(),
                    matching_paths_truncated: current.matching_paths_truncated(),
                    matching_paths_digest: current.matching_paths_digest().map(str::to_string),
                    source_plan_id: evidence.and_then(|value| value.source_plan_id.clone()),
                    source_batch_receipt_id: evidence
                        .and_then(|value| value.source_batch_receipt_id.clone()),
                    source_tool_receipt_id: evidence
                        .and_then(|value| value.source_tool_receipt_id.clone()),
                    valid_until_ms: receipt.valid_until_ms,
                    requires_time_validity: receipt.requires_time_validity,
                }))
            }
            Self::Evidence(gate) => StatusGate::Evidence(Box::new(gate.status_view())),
            Self::CodexReview(gate) => {
                let receipt = &gate.receipt;
                let evidence = gate.evidence.as_ref();
                StatusGate::CodexReview(Box::new(StatusCodexReviewGate {
                    id: gate.id.clone(),
                    required: gate.required,
                    skill: gate.skill.clone(),
                    status: gate.outcome.as_str().to_string(),
                    receipt_id: receipt.receipt_id.clone(),
                    exit_status: receipt.exit_status,
                    ended_at_ms: receipt.ended_at_ms,
                    freshness: receipt.freshness.as_str().to_string(),
                    freshness_reason: receipt.freshness_reason.clone(),
                    changed_paths: receipt.changed_paths.clone(),
                    changed_path_count: receipt.changed_path_count,
                    changed_paths_truncated: receipt.changed_paths_truncated,
                    changed_paths_digest: receipt.changed_paths_digest.clone(),
                    diff_summary: receipt.diff_summary.clone(),
                    finding_count: evidence
                        .and_then(|value| value.finding_count)
                        .and_then(|value| usize::try_from(value).ok()),
                    actionable_count: evidence
                        .and_then(|value| value.actionable_count)
                        .and_then(|value| usize::try_from(value).ok()),
                    retained_finding_count: evidence.and_then(|value| value.retained_finding_count),
                    retained_actionable_count: evidence
                        .and_then(|value| value.retained_actionable_count),
                    findings_truncated: evidence.and_then(|value| value.findings_truncated),
                    actionable_findings_truncated: evidence
                        .and_then(|value| value.actionable_findings_truncated),
                    threshold: evidence.and_then(|value| value.threshold.clone()),
                    parse_error: evidence
                        .and_then(WorkReviewReceiptEvidence::parse_error)
                        .map(str::to_string),
                    receipt_worktree_fingerprint_error: receipt
                        .receipt_worktree_fingerprint_error
                        .clone(),
                    current_worktree_fingerprint_error: receipt
                        .current_worktree_fingerprint_error
                        .clone(),
                    valid_until_ms: receipt.valid_until_ms,
                    requires_time_validity: receipt.requires_time_validity,
                }))
            }
            Self::Unsupported(gate) => StatusGate::Unsupported(StatusUnsupportedGate {
                kind: gate.kind.clone(),
                id: gate.id.clone(),
                required: gate.required,
                status: super::GateOutcome::Unsupported.as_str().to_string(),
                reason: gate.reason.clone(),
                extensions: BTreeMap::new(),
            }),
        }
    }
}

fn recorder_gate(
    status: &jig_ui::dashboard::StatusGate,
    evaluation: &GateEvaluation,
    plan_id: &str,
) -> Result<jig_ui::dashboard::GateObservation, jig_ui::dashboard::SourceError> {
    use jig_ui::dashboard::{BoundedRows, GateFinding, GateObservation, LimitId};

    let (id, tool, skill, required, state, freshness, ended_at_ms, diff_summary, changed, matching) =
        match status {
            jig_ui::dashboard::StatusGate::Check(gate) => (
                gate.id.clone(),
                Some(gate.tool.clone()),
                None,
                gate.required,
                gate.status.clone(),
                Some(gate.freshness.clone()),
                gate.ended_at_ms,
                gate.diff_summary.clone(),
                gate.changed_paths.clone(),
                gate.matching_paths.clone(),
            ),
            jig_ui::dashboard::StatusGate::Evidence(gate) => (
                gate.id.clone(),
                None,
                None,
                gate.required,
                gate.status.clone(),
                Some(gate.freshness.clone()),
                gate.targets
                    .iter()
                    .filter_map(|target| target.ended_at_ms)
                    .max(),
                None,
                Vec::new(),
                Vec::new(),
            ),
            jig_ui::dashboard::StatusGate::CodexReview(gate) => (
                gate.id.clone(),
                None,
                Some(gate.skill.clone()),
                gate.required,
                gate.status.clone(),
                Some(gate.freshness.clone()),
                gate.ended_at_ms,
                gate.diff_summary.clone(),
                gate.changed_paths.clone(),
                Vec::new(),
            ),
            jig_ui::dashboard::StatusGate::Unsupported(gate) => (
                gate.id.clone(),
                None,
                None,
                gate.required,
                gate.status.clone(),
                None,
                None,
                None,
                Vec::new(),
                Vec::new(),
            ),
        };
    let evidence = match evaluation {
        GateEvaluation::CodexReview(gate) => gate.evidence.as_ref(),
        GateEvaluation::Check(_) | GateEvaluation::Evidence(_) | GateEvaluation::Unsupported(_) => {
            None
        }
    };
    let finding_total = evidence
        .and_then(|value| value.finding_count)
        .and_then(|count| usize::try_from(count).ok())
        .map(|count| {
            count.max(
                evidence
                    .map(|value| value.findings.len())
                    .unwrap_or_default(),
            )
        })
        .or_else(|| evidence.map(|value| value.findings.len()))
        .unwrap_or(0);
    let findings = evidence
        .into_iter()
        .flat_map(|value| &value.findings)
        .take(LimitId::GateFindings.ceiling())
        .map(|finding| GateFinding {
            code: finding.code.clone(),
            message: finding.message.clone(),
            path: finding.path.clone(),
            line: finding.line,
        })
        .collect::<Vec<_>>();
    let finding_total = (finding_total == findings.len()
        || findings.len() == LimitId::GateFindings.ceiling())
    .then_some(finding_total);
    Ok(GateObservation {
        remediation: remediation(status, plan_id, &id),
        id,
        tool,
        skill,
        required,
        status: state,
        freshness,
        ended_at_ms,
        diff_summary,
        changed_paths: bounded_rows(changed, LimitId::GateChangedPaths)?,
        matching_paths: bounded_rows(matching, LimitId::GateMatchingPaths)?,
        findings: BoundedRows::for_limit(findings, finding_total, LimitId::GateFindings)
            .map_err(limit_error)?,
    })
}

fn remediation(
    gate: &jig_ui::dashboard::StatusGate,
    plan_id: &str,
    id: &str,
) -> Option<jig_ui::dashboard::Remediation> {
    let argv = match gate {
        jig_ui::dashboard::StatusGate::Check(_) => {
            vec![
                "scripts/jig",
                "work",
                "check",
                "--plan-id",
                plan_id,
                "--gate",
                id,
            ]
        }
        jig_ui::dashboard::StatusGate::CodexReview(_) => {
            vec![
                "scripts/jig",
                "work",
                "review",
                "--plan-id",
                plan_id,
                "--gate",
                id,
            ]
        }
        jig_ui::dashboard::StatusGate::Evidence(_)
        | jig_ui::dashboard::StatusGate::Unsupported(_) => return None,
    }
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    Some(jig_ui::dashboard::Remediation {
        display: argv
            .iter()
            .map(|part| crate::shell::quote(part))
            .collect::<Vec<_>>()
            .join(" "),
        argv,
    })
}

fn bounded_rows<T>(
    mut rows: Vec<T>,
    limit: jig_ui::dashboard::LimitId,
) -> Result<jig_ui::dashboard::BoundedRows<T>, jig_ui::dashboard::SourceError> {
    let total = rows.len();
    rows.truncate(limit.ceiling());
    jig_ui::dashboard::BoundedRows::for_limit(rows, Some(total), limit).map_err(limit_error)
}

fn limit_error(error: impl std::fmt::Display) -> jig_ui::dashboard::SourceError {
    jig_ui::dashboard::SourceError::InternalContract {
        message: error.to_string(),
    }
}
