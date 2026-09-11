use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use jig_contract::freshness::{
    DependencyIdentity, FreshnessCollectionStats, FreshnessReasonCode,
    TARGET_FRESHNESS_CONTRACT_VERSION, TARGET_IDENTITY_DOMAIN, TARGET_IDENTITY_SCHEMA_VERSION,
    TargetIdentityV1, WORKTREE_FRESHNESS_CONTRACT_VERSION, supported_freshness_epoch,
};
use jig_contract::{
    ActionInputsPolicy, ActionSourceState, ActionSpec, PlannedTarget, RunPlan, TargetId,
};

use super::RepositoryCatalog;
use crate::context::RepoContext;

mod authority;
mod budget;
mod encoding;
pub(crate) mod proof;
mod source;

pub(crate) use budget::{
    CollectionBudget, CollectionFailure, CollectionLimits, CollectionResult, INSPECTION_TIMEOUT_MS,
    RECORDING_TIMEOUT_MS,
};
use encoding::IdentityEncoder;
pub(crate) use source::ExecutionAuthorityGuard;
pub(crate) use source::{read_native_authority_bytes, revalidate_whole_source};

pub(crate) fn validate_inputs_policy(epoch: u32, action: &ActionSpec) -> Result<()> {
    ensure!(
        epoch >= WORKTREE_FRESHNESS_CONTRACT_VERSION || action.source_state.is_none(),
        "target '{}' source_state requires contract version {WORKTREE_FRESHNESS_CONTRACT_VERSION} or later",
        action.target
    );
    ensure!(
        action.source_state != Some(ActionSourceState::Worktree)
            || !matches!(action.runner, jig_contract::ActionRunner::Native { .. }),
        "native target '{}' must retain Git and comparison authority; source_state = worktree is for commands that consume working files",
        action.target
    );
    ensure!(
        epoch >= TARGET_FRESHNESS_CONTRACT_VERSION || action.inputs_policy.is_none(),
        "target '{}' inputs_policy requires contract version {TARGET_FRESHNESS_CONTRACT_VERSION} or later, including an explicit whole_repository value",
        action.target
    );
    ensure!(
        action.inputs_policy != Some(ActionInputsPolicy::Exhaustive) || !action.inputs.is_empty(),
        "target '{}' exhaustive inputs_policy requires non-empty inputs",
        action.target
    );
    ensure!(
        action.inputs_policy != Some(ActionInputsPolicy::Exhaustive)
            || !action
                .inputs
                .iter()
                .any(|input| input.split('/').any(|part| part == ".git")),
        "target '{}' exhaustive inputs cannot declare excluded Git metadata",
        action.target
    );
    Ok(())
}

pub(crate) struct TargetIdentityCollection {
    source: source::SourceSnapshot,
    pub(crate) targets: BTreeMap<TargetId, CollectionResult<TargetIdentityV1>>,
    #[allow(
        dead_code,
        reason = "collection metrics are measured by the development benchmark; .4.3 exposes them through gate inspection"
    )]
    pub(crate) stats: FreshnessCollectionStats,
}

impl TargetIdentityCollection {
    pub(crate) fn execution_authority(
        &mut self,
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        invocation: &PlannedTarget,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<ExecutionAuthorityGuard> {
        self.source.revalidate_execution(ctx, budget)?;
        let identity = self
            .targets
            .get(&invocation.target)
            .ok_or_else(|| {
                CollectionFailure::new(
                    FreshnessReasonCode::CollectionFailed,
                    "the live worker did not collect this target identity",
                )
            })?
            .as_ref()
            .map_err(Clone::clone)?;
        let action = catalog.action(&invocation.target).ok_or_else(|| {
            CollectionFailure::new(
                FreshnessReasonCode::UnsupportedReference,
                "execution target is missing from the repository catalog",
            )
        })?;
        self.source.execution_authority(
            ctx,
            catalog,
            action,
            invocation,
            &identity.authority_digest,
            budget,
        )
    }

    pub(crate) fn revalidate(
        &self,
        ctx: &RepoContext,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<()> {
        self.source.revalidate(ctx, budget)
    }
}

pub(crate) fn prepare_plan_identities(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: &mut RunPlan,
    budget: &mut CollectionBudget<'_>,
) -> Result<()> {
    if catalog.contract_version() < TARGET_FRESHNESS_CONTRACT_VERSION {
        return Ok(());
    }
    let identities = collect_target_identities(
        ctx,
        catalog,
        &plan.targets,
        &plan.source.worktree_fingerprint,
        budget,
    );
    for target in &mut plan.targets {
        let identity = match &identities {
            Ok(identities) => identities
                .targets
                .get(&target.target)
                .ok_or_else(|| anyhow::anyhow!("target identity missing for {}", target.target))?
                .clone(),
            Err(error) => Err(error.clone()),
        };
        match identity {
            Ok(mut identity) => {
                // Diagnostic previews are not executable plan authority. Keep
                // the full token and counts without repeating path lists in
                // durable plans or their equality hash.
                identity.source_preview.clear();
                identity.source_preview_truncated = identity.source_entry_count != 0;
                target.target_identity = Some(identity);
                target.target_identity_error = None;
            }
            Err(error) => {
                target.target_identity = None;
                target.target_identity_error = Some(error.reason);
            }
        }
    }
    // The new proof never replaces the existing global source/config checks.
    super::validate_run_plan_source(ctx, plan)?;
    super::validate_current_repository_authority(ctx, catalog.config_digest())?;
    Ok(())
}

pub(crate) fn collect_target_identities(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    invocations: &[PlannedTarget],
    whole_repository_token: &str,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<TargetIdentityCollection> {
    collect_target_identities_with_source(
        ctx,
        catalog,
        invocations,
        Some(whole_repository_token),
        budget,
    )
}

pub(crate) fn collect_target_identities_with_source(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    invocations: &[PlannedTarget],
    whole_repository_token: Option<&str>,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<TargetIdentityCollection> {
    if !supported_freshness_epoch(catalog.contract_version()) {
        return Err(CollectionFailure::new(
            FreshnessReasonCode::UnsupportedAuthority,
            "target identity contract epoch is unsupported",
        ));
    }
    let order = dependency_order(
        catalog,
        invocations.iter().map(|target| &target.target),
        budget,
    )?;
    let mut bound_invocations = BTreeMap::new();
    for invocation in invocations {
        budget.ensure_active()?;
        if bound_invocations
            .insert(&invocation.target, invocation)
            .is_some()
        {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnsupportedAuthority,
                "target closure contains ambiguous duplicate invocations",
            ));
        }
    }
    let actions = order
        .iter()
        .map(|target| catalog.action(target).expect("validated dependency target"))
        .collect::<Vec<_>>();
    let mut snapshot = source::SourceSnapshot::capture(ctx, &actions, budget)?;
    let mut targets: BTreeMap<TargetId, CollectionResult<TargetIdentityV1>> = BTreeMap::new();
    let identity_started = std::time::Instant::now();
    for target in order {
        budget.ensure_active()?;
        let action = catalog
            .action(&target)
            .expect("validated dependency target");
        let identity = (|| {
            let invocation = bound_invocations.get(&target).ok_or_else(|| {
                CollectionFailure::new(
                    FreshnessReasonCode::UnsupportedAuthority,
                    "complete bound invocation is missing from the target closure",
                )
            })?;
            let mut dependencies = action.depends_on.clone();
            dependencies.sort();
            dependencies.dedup();
            let dependencies = dependencies
                .iter()
                .map(|dependency| {
                    let identity = targets.get(dependency).ok_or_else(|| {
                        CollectionFailure::new(
                            FreshnessReasonCode::UnsupportedReference,
                            "dependency identity is missing",
                        )
                    })?;
                    match identity {
                        Ok(identity) => Ok(DependencyIdentity {
                            target: dependency.clone(),
                            identity_digest: identity.identity_digest.clone(),
                        }),
                        Err(error) => {
                            let mut error = error.clone();
                            error.reason.target = Some(dependency.clone());
                            Err(error)
                        }
                    }
                })
                .collect::<CollectionResult<Vec<_>>>()?;
            let source = snapshot.for_action(
                catalog.contract_version(),
                action,
                whole_repository_token,
                budget,
            )?;
            let authority =
                authority::collect(ctx, catalog, action, invocation, &mut snapshot, budget)?;
            let mut dependency_hash =
                IdentityEncoder::new("jig-target-dependencies-v1", catalog.contract_version());
            dependency_hash.number(dependencies.len() as u64);
            for dependency in &dependencies {
                dependency_hash.target(&dependency.target);
                dependency_hash.text(&dependency.identity_digest);
            }
            let dependency_digest = dependency_hash.finish();
            let mut identity_hash =
                IdentityEncoder::new(TARGET_IDENTITY_DOMAIN, catalog.contract_version());
            identity_hash.target(&target);
            let source_state = (catalog.contract_version() >= WORKTREE_FRESHNESS_CONTRACT_VERSION)
                .then(|| action.source_state.unwrap_or_default());
            if catalog.contract_version() >= WORKTREE_FRESHNESS_CONTRACT_VERSION {
                identity_hash.source_state(source_state);
            }
            identity_hash.text(&source.digest);
            identity_hash.text(&authority.digest);
            identity_hash.text(&dependency_digest);
            Ok(TargetIdentityV1 {
                contract_epoch: catalog.contract_version(),
                schema_version: TARGET_IDENTITY_SCHEMA_VERSION,
                digest_domain: TARGET_IDENTITY_DOMAIN.into(),
                target: target.clone(),
                inputs_policy: action.inputs_policy.unwrap_or_default(),
                source_state,
                source_digest: source.digest,
                authority_digest: authority.digest,
                dependency_digest,
                identity_digest: identity_hash.finish(),
                configuration_digest: catalog.config_digest().into(),
                runner_digest: authority.runner_digest,
                invocation_digest: authority.invocation_digest,
                source_preview: source.preview,
                source_entry_count: source.count,
                source_preview_truncated: source.truncated,
                dependencies,
            })
        })();
        targets.insert(target, identity);
    }
    budget.stats.identity_us += identity_started.elapsed().as_micros() as u64;
    snapshot.revalidate(ctx, budget)?;
    Ok(TargetIdentityCollection {
        source: snapshot,
        targets,
        stats: budget.finish_stats(),
    })
}

fn dependency_order<'a>(
    catalog: &RepositoryCatalog,
    roots: impl Iterator<Item = &'a TargetId>,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<Vec<TargetId>> {
    let mut pending = Vec::new();
    for target in roots {
        budget.ensure_active()?;
        if pending.len() as u64 >= budget.limits.targets {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "freshness target root limit was exceeded",
            ));
        }
        pending.push((target.clone(), false));
    }
    let mut visiting = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut order = Vec::new();
    while let Some((target, exiting)) = pending.pop() {
        budget.ensure_active()?;
        if complete.contains(&target) {
            continue;
        }
        if exiting {
            visiting.remove(&target);
            complete.insert(target.clone());
            order.push(target);
            continue;
        }
        if !visiting.insert(target.clone()) {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnsupportedReference,
                "target dependency graph contains a cycle",
            ));
        }
        budget.target()?;
        let action = catalog.action(&target).ok_or_else(|| {
            CollectionFailure::new(
                FreshnessReasonCode::UnsupportedReference,
                "target dependency reference is unresolved",
            )
        })?;
        budget.edges(action.depends_on.len() as u64)?;
        pending.push((target, true));
        let dependencies = action.depends_on.iter().collect::<BTreeSet<_>>();
        pending.extend(
            dependencies
                .into_iter()
                .rev()
                .map(|target| (target.clone(), false)),
        );
    }
    Ok(order)
}

#[cfg(test)]
mod tests;
