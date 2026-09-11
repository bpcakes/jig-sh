use super::*;

pub(super) fn check_with_execution(
    ctx: &RepoContext,
    opts: WorkCheckRequest,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    if opts.gates.is_empty() && opts.tools.is_empty() {
        return check_configured_with_execution(
            ctx,
            &opts.plan_id,
            FailureMode::Abort,
            execution,
            observer,
        );
    }
    let mut result = check_selected_with_observer(
        ctx,
        &opts.plan_id,
        selected_checks(ctx, &opts.gates, &opts.tools)?,
        execution,
        observer,
    )?;
    if !opts.tools.is_empty()
        && ctx
            .work_gates()
            .iter()
            .any(|gate| matches!(gate, crate::context::WorkGate::Evidence(_)))
    {
        result["native_evidence_note"] = json!(
            "Selected --tool checks record legacy tool evidence; these receipts cannot satisfy configured native target gates. Inspect work gates for plan-bound native refresh commands."
        );
    }
    Ok(result)
}
