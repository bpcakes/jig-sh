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
