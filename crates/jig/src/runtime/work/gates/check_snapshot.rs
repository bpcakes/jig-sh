use super::*;

pub(in crate::runtime) fn reusable_invocation_after_resource_wait(
    ctx: &RepoContext,
    plan_id: &str,
    catalog: &RepositoryCatalog,
    planned: &jig_contract::PlannedTarget,
    timeout: std::time::Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<jig_contract::TargetRunResult>> {
    let started = std::time::Instant::now();
    let stopped = || cancelled() || started.elapsed() >= timeout;
    // The outer predicate bounds fingerprinting and journal scans as well as
    // scoped proof collection; none may reset the target's admission budget.
    let snapshot = selected_invocation_snapshot_with_timeout(
        ctx,
        plan_id,
        catalog,
        std::slice::from_ref(planned),
        &stopped,
        timeout,
    )?;
    if stopped() || !snapshot.unavailable.is_empty() {
        return Ok(None);
    }
    Ok(snapshot
        .targets
        .first()
        .and_then(|target| target.reused_result()))
}

pub(super) fn repository_for_evidence_gates(
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

pub(in crate::runtime::work) struct CheckTargetSnapshot {
    pub(in crate::runtime::work) passing: BTreeSet<jig_contract::TargetId>,
    pub(in crate::runtime::work) targets: BTreeMap<jig_contract::TargetId, Value>,
    pub(in crate::runtime::work) fingerprint: Option<String>,
    pub(in crate::runtime::work) freshness_collection:
        Option<jig_contract::freshness::FreshnessCollectionStats>,
}

pub(in crate::runtime::work) struct SelectedInvocationSnapshot {
    pub(in crate::runtime::work) passing: BTreeSet<jig_contract::TargetId>,
    pub(in crate::runtime::work) unavailable: BTreeSet<jig_contract::TargetId>,
    pub(in crate::runtime::work) targets: Vec<super::target_evidence::TargetEvidenceEvaluation>,
    pub(in crate::runtime::work) fingerprint: Option<String>,
    pub(in crate::runtime::work) freshness_collection:
        Option<jig_contract::freshness::FreshnessCollectionStats>,
}

pub(in crate::runtime::work) fn selected_invocation_snapshot(
    ctx: &RepoContext,
    plan_id: &str,
    catalog: &RepositoryCatalog,
    invocations: &[jig_contract::PlannedTarget],
    cancelled: &dyn Fn() -> bool,
) -> Result<SelectedInvocationSnapshot> {
    selected_invocation_snapshot_with_timeout(
        ctx,
        plan_id,
        catalog,
        invocations,
        cancelled,
        std::time::Duration::from_millis(crate::repository::freshness::RECORDING_TIMEOUT_MS),
    )
}

fn selected_invocation_snapshot_with_timeout(
    ctx: &RepoContext,
    plan_id: &str,
    catalog: &RepositoryCatalog,
    invocations: &[jig_contract::PlannedTarget],
    cancelled: &dyn Fn() -> bool,
    timeout: std::time::Duration,
) -> Result<SelectedInvocationSnapshot> {
    let current_fingerprint =
        crate::state::current_worktree_fingerprint_with_cancellation(ctx, cancelled)?;
    let required = invocations
        .iter()
        .map(|invocation| invocation.target.clone())
        .collect::<BTreeSet<_>>();
    let receipts =
        crate::state::target_receipt_index_with_cancellation(ctx, plan_id, &required, cancelled)?;
    let scoped = if catalog.contract_version()
        >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION
    {
        let mut budget = crate::repository::freshness::CollectionBudget::new(
            crate::repository::freshness::CollectionLimits::with_timeout(timeout),
            cancelled,
        );
        Some(
            super::scoped_freshness::ScopedGateFreshness::collect_invocations(
                ctx,
                catalog,
                plan_id,
                invocations,
                &receipts,
                current_fingerprint.fingerprint.as_deref(),
                &mut budget,
            ),
        )
    } else {
        None
    };
    let freshness_collection = scoped.as_ref().map(|scoped| scoped.stats.clone());
    let targets = super::target_evidence::evaluate_targets(
        catalog,
        &current_fingerprint,
        Some(&receipts),
        required,
        super::GateCollection::Cancellable(cancelled),
        scoped.as_ref(),
    )?;
    let passing = targets
        .iter()
        .filter(|target| target.is_passing())
        .map(|target| target.target().clone())
        .collect();
    let unavailable = targets
        .iter()
        .filter(|target| target.authority_unavailable())
        .map(|target| target.target().clone())
        .collect();
    Ok(SelectedInvocationSnapshot {
        passing,
        unavailable,
        targets,
        fingerprint: current_fingerprint.fingerprint,
        freshness_collection,
    })
}

#[cfg(test)]
pub(crate) fn selected_invocation_snapshot_with_test_timeout(
    ctx: &RepoContext,
    plan_id: &str,
    catalog: &RepositoryCatalog,
    invocations: &[jig_contract::PlannedTarget],
    timeout: std::time::Duration,
) -> Result<(
    BTreeSet<jig_contract::TargetId>,
    BTreeSet<jig_contract::TargetId>,
    Option<jig_contract::freshness::FreshnessCollectionStats>,
)> {
    let snapshot = selected_invocation_snapshot_with_timeout(
        ctx,
        plan_id,
        catalog,
        invocations,
        &|| false,
        timeout,
    )?;
    Ok((
        snapshot.passing,
        snapshot.unavailable,
        snapshot.freshness_collection,
    ))
}

pub(in crate::runtime::work) fn check_target_snapshot(
    ctx: &RepoContext,
    plan_id: &str,
    selected: &BTreeSet<jig_contract::TargetId>,
    cancelled: &dyn Fn() -> bool,
) -> Result<CheckTargetSnapshot> {
    let report = gate_report_with_cancellation(ctx, plan_id, cancelled, RECORDING_TIMEOUT_MS)?;
    let mut passing = BTreeSet::new();
    let mut targets = BTreeMap::new();
    let mut freshness_collection = None;
    for gate in &report.gates {
        if let GateEvaluation::Evidence(gate) = gate {
            freshness_collection = gate.collection_stats().cloned().or(freshness_collection);
            for (target, passed, value) in gate.check_targets() {
                if !selected.contains(&target) {
                    continue;
                }
                if passed {
                    passing.insert(target.clone());
                }
                targets.insert(target, value);
            }
        }
    }
    Ok(CheckTargetSnapshot {
        passing,
        targets,
        fingerprint: report.current_worktree_fingerprint,
        freshness_collection,
    })
}
