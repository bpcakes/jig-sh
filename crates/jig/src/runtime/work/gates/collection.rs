use super::*;

pub(super) fn gate_report(ctx: &RepoContext, plan_id: &str, timeout_ms: u64) -> Result<GateReport> {
    let plan_state = resolve_plan_state(ctx, plan_id)?;
    evaluate_gate_report(
        ctx,
        plan_id,
        plan_state,
        current_worktree_fingerprint(ctx),
        GateCollection::Blocking,
        timeout_ms,
    )
}

pub(super) fn gate_report_with_cancellation(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
    timeout_ms: u64,
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
        timeout_ms,
    )
}

fn evaluate_gate_report(
    ctx: &RepoContext,
    plan_id: &str,
    plan_state: &'static str,
    current_fingerprint: crate::state::CurrentWorktreeFingerprint,
    collection: GateCollection<'_>,
    timeout_ms: u64,
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

    let plan_scope = match collection {
        GateCollection::Blocking => PlanGateContext::load(ctx, plan_id)?,
        GateCollection::Cancellable(cancelled) => {
            PlanGateContext::load_with_cancellation(ctx, plan_id, cancelled)?
        }
    };
    let cancelled = || collection.cancelled();
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_millis(timeout_ms)),
        &cancelled,
    );
    evaluate_gate_report_from_index(
        ctx,
        GateReportPlanInput {
            plan_id,
            plan_state,
            prepared_scope: plan_scope,
        },
        current_fingerprint,
        work_gates,
        &receipt_index,
        collection,
        &mut budget,
    )
}

pub(super) fn evaluate_gate_report_from_index(
    ctx: &RepoContext,
    plan: GateReportPlanInput<'_>,
    current_fingerprint: crate::state::CurrentWorktreeFingerprint,
    work_gates: Vec<WorkGate>,
    receipt_index: &WorkGateReceiptIndex,
    collection: GateCollection<'_>,
    budget: &mut CollectionBudget<'_>,
) -> Result<GateReport> {
    let GateReportPlanInput {
        plan_id,
        plan_state,
        prepared_scope,
    } = plan;
    let mut gates = Vec::new();
    let mut required_failures = RequiredGateFailures::default();
    let plan_scope = prepared_scope;
    plan_scope.seed_legacy_fingerprint(current_fingerprint.clone());

    let repository = repository_for_evidence_gates(ctx, &work_gates).ok();
    let scoped = repository
        .as_ref()
        .filter(|catalog| {
            catalog.contract_version() >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION
        })
        .map(|catalog| {
            scoped_freshness::ScopedGateFreshness::collect(
                ctx,
                catalog,
                scoped_freshness::ScopedGateInputs {
                    plan_id,
                    baseline: plan_scope.baseline(),
                    gates: &work_gates,
                    receipts: receipt_index,
                    whole_source_token: current_fingerprint.fingerprint.as_deref(),
                },
                budget,
            )
        });

    for gate in work_gates {
        collection.ensure_active()?;
        let status = evaluate_gate(
            ctx,
            &plan_scope,
            &gate,
            &current_fingerprint,
            receipt_index,
            collection,
            scoped.as_ref(),
        )?;
        collection.ensure_active()?;
        required_failures.observe(&status);
        gates.push(status);
    }
    collection.ensure_active()?;

    let mut report = GateReport {
        recovery: None,
        plan_id: plan_id.to_string(),
        plan_state,
        plan_baseline: plan_scope.baseline().cloned(),
        current_worktree_fingerprint: current_fingerprint.fingerprint,
        current_worktree_fingerprint_error: current_fingerprint.error,
        gates,
        required_failures,
    };
    report.recovery = recovery::from_report(&report, repository.as_ref());
    Ok(report)
}
