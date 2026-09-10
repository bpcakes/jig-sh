use super::*;

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

pub(in crate::runtime::work) fn check_target_snapshot(
    ctx: &RepoContext,
    plan_id: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<CheckTargetSnapshot> {
    let report = gate_report_with_cancellation(ctx, plan_id, cancelled, RECORDING_TIMEOUT_MS)?;
    let mut passing = BTreeSet::new();
    let mut targets = BTreeMap::new();
    let mut freshness_collection = None;
    for gate in &report.gates {
        if let GateEvaluation::Evidence(gate) = gate
            && gate.required()
        {
            freshness_collection = gate.collection_stats().cloned().or(freshness_collection);
            for (target, passed, value) in gate.check_targets() {
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
