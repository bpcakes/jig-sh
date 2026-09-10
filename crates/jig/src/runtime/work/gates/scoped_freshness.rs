use jig_contract::freshness::{
    FreshnessCollectionStats, FreshnessReasonCode, TargetFreshness, TargetFreshnessStatus,
};
use jig_contract::{ActionRunner, NativeActionConfigurationV1, PlannedTarget, TargetId};

use super::*;
use crate::repository::freshness::proof::{
    OriginalProofValidator, compare_current_identity, empty, unverified,
};
use crate::repository::freshness::{
    CollectionBudget, CollectionFailure, CollectionResult, collect_target_identities_with_source,
    revalidate_whole_source,
};
use crate::state::{OriginalReceiptIndex, TargetReceiptStatus};

pub(super) struct ScopedGateFreshness {
    pub(super) targets: BTreeMap<TargetId, TargetFreshness>,
    pub(super) stats: FreshnessCollectionStats,
}

pub(super) struct ScopedGateInputs<'a> {
    pub(super) plan_id: &'a str,
    pub(super) baseline: Option<&'a PlanBaseline>,
    pub(super) gates: &'a [WorkGate],
    pub(super) receipts: &'a WorkGateReceiptIndex,
    pub(super) whole_source_token: Option<&'a str>,
}

impl ScopedGateFreshness {
    pub(super) fn collect(
        ctx: &RepoContext,
        catalog: &RepositoryCatalog,
        inputs: ScopedGateInputs<'_>,
        budget: &mut CollectionBudget<'_>,
    ) -> Self {
        let ScopedGateInputs {
            plan_id,
            baseline,
            gates,
            receipts,
            whole_source_token,
        } = inputs;
        let mut required = BTreeSet::new();
        let mut selected: BTreeMap<TargetId, &TargetReceiptStatus> = BTreeMap::new();
        for gate in gates {
            if let WorkGate::Evidence(gate) = gate
                && let Ok(targets) = resolve_evidence_targets(catalog, &gate.selector)
            {
                required.extend(targets);
                if let Some(receipts) = receipts.target_receipts(&gate.id) {
                    selected.extend(
                        receipts
                            .iter()
                            .map(|(target, receipt)| (target.clone(), receipt)),
                    );
                }
            }
        }
        let results = (|| {
            let proof_started = std::time::Instant::now();
            let invocations =
                default_invocations(ctx, catalog, &required, plan_id, baseline, budget)?;
            let originals = OriginalReceiptIndex::open_for_plan(
                &ctx.state_file("receipts.jsonl"),
                plan_id,
                budget,
            )?;
            let mut validator =
                OriginalProofValidator::new(originals, plan_id, crate::state::now_ms());
            let mut targets = BTreeMap::new();
            for target in &required {
                budget.ensure_active()?;
                let evaluation = if let Some(receipt) = selected.get(target) {
                    validator.evaluate_original(receipt, budget)
                } else {
                    missing()
                };
                targets.insert(target.clone(), evaluation);
            }
            budget.stats.proof_us += proof_started.elapsed().as_micros() as u64;
            // Resolve every original before observing current source. The
            // collector's final source revalidation then follows all journal
            // I/O; no second complete Git/source scan is needed after lookup.
            let identities = collect_target_identities_with_source(
                ctx,
                catalog,
                &invocations,
                whole_source_token,
                budget,
            )?;
            for (target, result) in &mut targets {
                budget.ensure_active()?;
                if let Some(receipt) = selected.get(target) {
                    let expected = identities.targets.get(target).ok_or_else(|| {
                        CollectionFailure::new(
                            FreshnessReasonCode::CollectionFailed,
                            "current required target identity is missing",
                        )
                    })?;
                    compare_current_identity(result, receipt, expected);
                }
            }
            revalidate_whole_source(ctx, catalog, &invocations, whole_source_token, budget)?;
            validator.revalidate(budget)?;
            let now = crate::state::now_ms();
            for result in targets.values_mut() {
                budget.ensure_active()?;
                crate::repository::freshness::proof::apply_time(result, now);
            }
            Ok::<_, CollectionFailure>(targets)
        })();
        let targets = results.unwrap_or_else(|failure| {
            required
                .into_iter()
                .map(|target| {
                    let result = if let Some(receipt) = selected.get(&target) {
                        unverified(receipt, failure.reason.clone(), crate::state::now_ms())
                    } else {
                        missing()
                    };
                    (target, result)
                })
                .collect()
        });
        Self {
            targets,
            stats: budget.finish_stats(),
        }
    }
}

fn missing() -> TargetFreshness {
    let mut result = empty();
    result.status = TargetFreshnessStatus::Missing;
    result
}

fn default_invocations(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    required: &BTreeSet<TargetId>,
    plan_id: &str,
    baseline: Option<&PlanBaseline>,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<Vec<PlannedTarget>> {
    let mut pending: Vec<_> = required.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut edges = 0_u64;
    let mut invocations = Vec::new();
    while let Some(target) = pending.pop() {
        budget.ensure_active()?;
        if !seen.insert(target.clone()) {
            continue;
        }
        let action = catalog.action(&target).ok_or_else(|| {
            CollectionFailure::new(
                FreshnessReasonCode::UnsupportedReference,
                "gate closure contains an unresolved target",
            )
        })?;
        edges = edges.saturating_add(action.depends_on.len() as u64);
        if seen.len() as u64 > budget.limits.targets || edges > budget.limits.edges {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "default invocation closure exceeds the shared graph limits",
            ));
        }
        pending.extend(action.depends_on.iter().cloned());
        let mut invocation = PlannedTarget::new(target, action.intent, action.runner.clone(), "");
        invocation.effects.clone_from(&action.effects);
        invocation.inputs.clone_from(&action.inputs);
        invocation.depends_on.clone_from(&action.depends_on);
        invocation.timeout_seconds = action.timeout_seconds;
        invocation.result_parser = action.result_parser;
        // Empty request arguments are the declared execution defaults; binding
        // and any missing required arguments are checked by the authority collector.
        if let ActionRunner::Native {
            operation,
            configuration,
        } = &action.runner
            && operation == jig_contract::tool::FILE_BUDGET
            && let Some(configuration) = configuration
                .as_ref()
                .and_then(NativeActionConfigurationV1::as_file_budget)
        {
            // Failure leaves prepared authority absent. The collector propagates
            // that unknown identity through exactly this target's dependents.
            invocation.prepared_native_input =
                match crate::repository::prepare_gate_file_budget_input(
                    ctx,
                    configuration.clone(),
                    plan_id,
                    baseline,
                    budget,
                ) {
                    Ok(prepared) => Some(prepared),
                    Err(error) if error.reason.code == FreshnessReasonCode::CollectionLimit => {
                        return Err(error);
                    }
                    Err(_) => None,
                };
            budget.ensure_active()?;
        }
        invocations.push(invocation);
    }
    Ok(invocations)
}
