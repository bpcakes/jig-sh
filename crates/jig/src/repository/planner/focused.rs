use super::*;

pub(crate) fn plan_focused_check_run_with_cancellation(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    request: PlanRunRequest,
    arguments: BTreeMap<TargetId, ActionArguments>,
    cancelled: &dyn Fn() -> bool,
) -> Result<RunPlan> {
    plan_run_with_policy(
        ctx,
        catalog,
        request,
        arguments,
        PlanningPolicy::ChecksOnly,
        Some(cancelled),
    )
}
