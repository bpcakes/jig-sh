//! One bounded request-local observation, never a persistent completion token.
use super::*;
use crate::repository::freshness::proof::OriginalProofValidator;
use crate::repository::freshness::{
    TargetIdentityCollection, collect_target_identities_with_source,
};
use crate::state::OriginalReceiptIndex;

#[derive(Default)]
pub(in crate::runtime::work::gates) struct ScopedGateObservations {
    retained: Option<Observation>,
}

pub(super) struct Request<'a> {
    pub(super) plan_id: &'a str,
    pub(super) required: &'a BTreeSet<TargetId>,
    pub(super) selected: &'a BTreeMap<TargetId, &'a TargetReceiptStatus>,
    pub(super) invocations: &'a [PlannedTarget],
    pub(super) whole_source_token: Option<&'a str>,
}

pub(super) struct Observation {
    configuration: String,
    whole_source_token: Option<String>,
    required: BTreeSet<TargetId>,
    selected: BTreeMap<TargetId, TargetReceiptStatus>,
    invocations: Vec<PlannedTarget>,
    pub(super) originals: OriginalProofValidator,
    pub(super) identities: TargetIdentityCollection,
    // Keep original proof outcomes, not already-compared or time-adjusted views.
    pub(super) original_targets: BTreeMap<TargetId, TargetFreshness>,
}

impl ScopedGateObservations {
    pub(super) fn observe(
        &mut self,
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        request: Request<'_>,
        budget: &mut CollectionBudget<'_>,
    ) -> CollectionResult<&Observation> {
        budget.ensure_active()?;
        let reusable = crate::repository::plan_independent_targets(catalog, request.required);
        // The existing receipt policy excludes native actions and their
        // dependents. Their comparison authority stays per-plan, even when
        // superficially similar invocations would otherwise compare equal.
        let can_share = !request.required.is_empty() && reusable == *request.required;
        let hit = can_share
            && self.retained.as_ref().is_some_and(|observation| {
                observation.configuration == catalog.config_digest()
                    && observation.whole_source_token.as_deref() == request.whole_source_token
                    && observation.required == *request.required
                    && observation.invocations == request.invocations
                    && observation.selected.len() == request.selected.len()
                    && request
                        .selected
                        .iter()
                        .all(|(target, receipt)| observation.selected.get(target) == Some(*receipt))
            });
        if !hit {
            // At most one retained graph/snapshot; discard mismatched authority.
            self.retained = None;
            let proof_started = std::time::Instant::now();
            let originals = OriginalReceiptIndex::open_for_work_reuse(
                &ctx.state_file("receipts.jsonl"),
                request.plan_id,
                &reusable,
                budget,
            )?;
            let mut originals =
                OriginalProofValidator::for_work_reuse(originals, crate::state::now_ms());
            let mut original_targets = BTreeMap::new();
            for target in request.required {
                budget.ensure_active()?;
                let evaluation = request
                    .selected
                    .get(target)
                    .map_or_else(missing, |receipt| {
                        originals.evaluate_original(receipt, budget)
                    });
                original_targets.insert(target.clone(), evaluation);
            }
            budget.stats.proof_us += proof_started.elapsed().as_micros() as u64;
            // Journal resolution precedes the source collection and its final
            // revalidation, preserving the existing observation ordering.
            let identities = collect_target_identities_with_source(
                ctx,
                catalog,
                request.invocations,
                request.whole_source_token,
                budget,
            )?;
            self.retained = Some(Observation {
                configuration: catalog.config_digest().to_owned(),
                whole_source_token: request.whole_source_token.map(str::to_owned),
                required: request.required.clone(),
                selected: request
                    .selected
                    .iter()
                    .map(|(target, receipt)| (target.clone(), (*receipt).clone()))
                    .collect(),
                invocations: request.invocations.to_vec(),
                originals,
                identities,
                original_targets,
            });
        }
        let observation = self.retained.as_ref().expect("observation was collected");
        if hit {
            // A cache hit is not an authority shortcut. A changed path, journal
            // generation, source projection or configuration fails closed.
            let proof_started = std::time::Instant::now();
            observation.originals.revalidate(budget)?;
            budget.stats.proof_us += proof_started.elapsed().as_micros() as u64;
            observation.identities.revalidate(ctx, budget)?;
        }
        budget.ensure_active()?;
        Ok(observation)
    }
}
