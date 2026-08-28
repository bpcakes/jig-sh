use super::*;

pub(in crate::runtime::work) fn open_plan_snapshots_with_cancellation(
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
    let (repository, repository_error) = repository_for_gate_status(ctx, &work_gates);
    let dependencies = collect_work_gate_dependencies(
        ctx,
        &work_gates,
        repository.as_ref(),
        repository_error.as_deref(),
        GateCollection::Cancellable(cancelled),
    )?;
    let plan_ids_set = plan_ids.iter().cloned().collect::<BTreeSet<_>>();
    let indexes = work_gate_receipt_indexes_with_cancellation(
        ctx,
        &plan_ids_set,
        &dependencies.check_tools,
        &dependencies.review_gate_ids,
        &dependencies.evidence_targets,
        cancelled,
    )?;
    let mut snapshots = BTreeMap::new();
    for plan_id in plan_ids {
        ensure_gate_collection_active(cancelled)?;
        let index = indexes
            .get(plan_id)
            .expect("every requested open plan has a receipt index");
        let report = evaluate_gate_report_from_index(
            plan_id,
            "open",
            current_fingerprint.clone(),
            work_gates.clone(),
            GateEvaluationInputs {
                repository: repository.as_ref(),
                receipt_index: index,
                resolution_errors: &dependencies.resolution_errors,
            },
            GateCollection::Cancellable(cancelled),
        )?;
        snapshots.insert(plan_id.clone(), report.to_value());
    }
    Ok(snapshots)
}

pub(super) fn resolve_plan_state(ctx: &RepoContext, plan_id: &str) -> Result<&'static str> {
    Ok(match plan_status(ctx, plan_id)? {
        Some(PlanStatus::Open) => "open",
        Some(PlanStatus::Closed) => "closed",
        None => bail!("Plan not found: {plan_id}"),
    })
}

pub(super) fn resolve_plan_state_with_cancellation(
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

pub(super) fn resolve_work_plan_id(ctx: &RepoContext, requested: Option<String>) -> Result<String> {
    if let Some(plan_id) = requested {
        ensure_plan_exists(ctx, &plan_id)?;
        return Ok(plan_id);
    }

    let open_plans = open_plan_summaries(ctx)?;
    resolve_open_plan_id(&open_plans)
}

pub(super) fn resolve_work_plan_id_with_cancellation(
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

pub(super) fn resolve_open_plan_id(open_plans: &[Value]) -> Result<String> {
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

pub(super) fn ensure_gate_collection_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

pub(super) fn latest_passing_gates(report: &GateReport) -> Vec<Value> {
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

pub(super) fn evaluate_gate(
    gate: &WorkGate,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    inputs: &GateEvaluationInputs<'_>,
    collection: GateCollection<'_>,
) -> Result<GateEvaluation> {
    collection.ensure_active()?;
    match gate {
        WorkGate::Check(gate) => {
            if let Some(reason) = inputs.resolution_errors.get(&gate.id) {
                return Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    kind: "check".into(),
                    reason: Some(reason.clone()),
                }));
            }
            let tool_name = gate.tool.as_str();
            collection.ensure_active()?;
            let receipt = inputs.receipt_index.tool_receipt(tool_name).cloned();
            collection.ensure_active()?;
            let freshness_receipt = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => {
                    // Freshness is anchored to the batch work-check receipt
                    // when available, since that receipt captures the
                    // before/after worktree fingerprint for the gate run.
                    let latest = inputs
                        .receipt_index
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
            if let Some(reason) = inputs.resolution_errors.get(&gate.id) {
                return Ok(GateEvaluation::Unsupported(UnsupportedGateEvaluation {
                    id: gate.id.clone(),
                    required: gate.required,
                    kind: "evidence".into(),
                    reason: Some(reason.clone()),
                }));
            }
            let catalog = inputs
                .repository
                .ok_or_else(|| anyhow!("repository catalog was not loaded for an evidence gate"))?;
            Ok(GateEvaluation::Evidence(EvidenceGateEvaluation::evaluate(
                gate,
                catalog,
                current_fingerprint,
                inputs.receipt_index,
                collection,
            )?))
        }
        WorkGate::CodexReview(gate) => {
            let skill = gate.skill.as_str();
            collection.ensure_active()?;
            let receipt = inputs.receipt_index.review_receipt(&gate.id).cloned();
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
            reason: None,
        })),
    }
}

pub(super) trait GateReceiptView {
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

pub(super) fn gate_changed_paths<T: GateReceiptView>(
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

pub(super) fn gate_freshness<T: GateReceiptView>(
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

pub(super) fn concise_error(error: &str) -> String {
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
