use anyhow::{Context, Result, ensure};
use jig_contract::freshness::{EffectiveTimeValidityV1, TargetFreshnessStatus};
use jig_contract::{PlannedTarget, PreparedNativeInputV1};

use crate::context::RepoContext;
use crate::repository::RepositoryCatalog;
use crate::repository::freshness::proof::OriginalProofValidator;
use crate::repository::freshness::{CollectionBudget, collect_target_identities};
use crate::state::{OriginalReceiptIndex, TargetReceiptStatus};

pub(super) fn validate(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    receipt: &TargetReceiptStatus,
    prepared: PreparedNativeInputV1,
    source: &str,
    originals: OriginalReceiptIndex,
    budget: &mut CollectionBudget<'_>,
) -> Result<EffectiveTimeValidityV1> {
    let action = catalog
        .action(&receipt.target)
        .context("native lifecycle target disappeared")?;
    let mut invocation = PlannedTarget::new(
        action.target.clone(),
        action.intent,
        action.runner.clone(),
        "",
    );
    invocation.effects.clone_from(&action.effects);
    invocation.inputs.clone_from(&action.inputs);
    invocation.depends_on.clone_from(&action.depends_on);
    invocation.timeout_seconds = action.timeout_seconds;
    invocation.result_parser = action.result_parser;
    crate::repository::revalidate_freshness_native_input(ctx, &prepared, budget)?;
    invocation.prepared_native_input = Some(prepared);
    let identities = collect_target_identities(ctx, catalog, &[invocation], source, budget)?;
    let expected = identities
        .targets
        .get(&receipt.target)
        .context("native lifecycle identity is missing")?;
    let mut validator = OriginalProofValidator::for_receipt_plan(
        originals,
        receipt.plan_id.as_deref(),
        crate::state::now_ms(),
    );
    let result = validator.evaluate(receipt, expected, budget);
    ensure!(
        result.status == TargetFreshnessStatus::Fresh,
        "the latest repo:file-budget receipt has unusable target freshness: {:?} ({:?})",
        result.status,
        result.reasons.reasons
    );
    validator.revalidate(budget)?;
    identities.revalidate(ctx, budget)?;
    let validity = EffectiveTimeValidityV1::new(
        result.effective_valid_until_ms,
        result.effective_requires_time_validity,
    );
    ensure!(
        crate::state::time_validity_is_current(
            validity.effective_valid_until_ms,
            validity.effective_requires_time_validity,
            crate::state::now_ms()
        ),
        "the latest repo:file-budget effective validity expired during proof collection"
    );
    Ok(validity)
}

#[cfg(test)]
mod tests;
