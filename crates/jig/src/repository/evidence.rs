use super::*;

pub(crate) fn resolve_evidence_targets(
    catalog: &RepositoryCatalog,
    selector: &WorkEvidenceSelector,
) -> Result<BTreeSet<TargetId>> {
    let targets = match selector {
        WorkEvidenceSelector::Target(target) => {
            if catalog.action(target).is_none() {
                bail!("work evidence gate references unknown target '{target}'");
            }
            BTreeSet::from([target.clone()])
        }
        WorkEvidenceSelector::Profile(profile) => {
            let profile = catalog.profile(profile).ok_or_else(|| {
                anyhow::anyhow!("work evidence gate references unknown profile '{profile}'")
            })?;
            if profile.targets.is_empty() {
                bail!(
                    "work evidence gate profile '{}' contains no targets",
                    profile.id
                );
            }
            profile.targets.iter().cloned().collect()
        }
    };
    let required_targets = targets.clone();
    let mut targets = targets;
    let mut pending: Vec<_> = targets.iter().cloned().collect();
    while let Some(target) = pending.pop() {
        let action = catalog
            .action(&target)
            .ok_or_else(|| anyhow::anyhow!("work evidence references unknown target '{target}'"))?;
        for dependency in &action.depends_on {
            if targets.insert(dependency.clone()) {
                pending.push(dependency.clone());
            }
        }
    }
    planner::validate_check_actions(catalog, targets.iter())?;
    if catalog.contract_version() >= jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION {
        Ok(required_targets)
    } else {
        Ok(targets)
    }
}

/// Targets whose execution authority is independent of the work plan consuming it.
/// Native runners may prepare plan-specific inputs; keep them and their transitive
/// dependents plan-local so checks in another plan cannot supersede their evidence.
pub(crate) fn plan_independent_targets(
    catalog: &RepositoryCatalog,
    required: &BTreeSet<TargetId>,
) -> BTreeSet<TargetId> {
    if catalog.contract_version() < jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION {
        return BTreeSet::new();
    }
    let mut dependents = BTreeMap::<TargetId, Vec<TargetId>>::new();
    let mut pending = Vec::new();
    for action in catalog.actions() {
        if matches!(action.runner, jig_contract::ActionRunner::Native { .. }) {
            pending.push(action.target.clone());
        }
        for dependency in &action.depends_on {
            dependents
                .entry(dependency.clone())
                .or_default()
                .push(action.target.clone());
            if catalog.action(dependency).is_none() {
                pending.push(action.target.clone());
            }
        }
    }
    let mut plan_bound = BTreeSet::new();
    while let Some(target) = pending.pop() {
        if plan_bound.insert(target.clone()) {
            pending.extend(dependents.get(&target).into_iter().flatten().cloned());
        }
    }
    required
        .difference(&plan_bound)
        .filter(|target| catalog.action(target).is_some())
        .cloned()
        .collect()
}

pub(crate) fn cross_plan_evidence_targets(
    ctx: &RepoContext,
    gates: &BTreeMap<String, BTreeSet<TargetId>>,
) -> Result<BTreeSet<TargetId>> {
    if gates.is_empty()
        || ctx.contract_version() < jig_contract::freshness::TARGET_FRESHNESS_CONTRACT_VERSION
    {
        return Ok(BTreeSet::new());
    }
    let catalog = RepositoryCatalog::from_context(ctx)?;
    Ok(plan_independent_targets(
        &catalog,
        &gates.values().flatten().cloned().collect(),
    ))
}
