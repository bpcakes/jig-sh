use super::*;
use crate::repository::RepositoryCatalog;
use jig_contract::PlannedTarget;

pub(crate) fn revalidate_whole_source(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    invocations: &[PlannedTarget],
    initial: Option<&str>,
    budget: &CollectionBudget<'_>,
) -> CollectionResult<()> {
    if !invocations.iter().any(|invocation| {
        catalog.action(&invocation.target).is_some_and(|action| {
            action.inputs_policy != Some(jig_contract::ActionInputsPolicy::Exhaustive)
        })
    }) {
        return Ok(());
    }
    // Whole-policy dependencies use the legacy global source authority, which
    // predates journal lookup. Its existing collector and limits still apply,
    // but a second observation must fit the remaining inspection deadline.
    budget.ensure_active()?;
    let initial = initial.ok_or_else(unavailable)?;
    let current =
        crate::state::current_worktree_fingerprint_with_cancellation(ctx, &|| budget.stopped());
    budget.ensure_active()?;
    let current = current
        .map_err(|_| unavailable())?
        .fingerprint
        .ok_or_else(unavailable)?;
    if current != initial {
        return Err(CollectionFailure::new(
            FreshnessReasonCode::SourceRaced,
            "whole-repository source changed during target proof inspection",
        ));
    }
    Ok(())
}

fn unavailable() -> CollectionFailure {
    CollectionFailure::new(
        FreshnessReasonCode::CollectionFailed,
        "whole-repository source authority could not be revalidated",
    )
}
