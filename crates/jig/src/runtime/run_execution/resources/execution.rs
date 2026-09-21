//! Shared serial/wave execution: never publishes receipts or releases claims.
use super::*;

pub(in crate::runtime::run_execution) fn reuse_candidate(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    control: &TargetExecutionControl<'_>,
    allow_reuse: bool,
) -> std::result::Result<Option<TargetRunResult>, TargetStop> {
    let terminal = !finisher
        .run
        .plan
        .targets
        .iter()
        .any(|target| target.depends_on.contains(&planned.target));
    if !allow_reuse || !terminal || !finisher.record_receipts {
        return Ok(None);
    }
    let Some(plan_id) = finisher.work_plan_id else {
        return Ok(None);
    };
    let reusable = crate::runtime::work::reusable_invocation_after_resource_wait(
        finisher.ctx,
        plan_id,
        finisher.catalog,
        planned,
        control.remaining()?,
        &|| control.remaining().is_err(),
    );
    control.remaining()?;
    // Unknown or failed evidence is not permission to skip execution.
    Ok(reusable.ok().flatten())
}

pub(in crate::runtime::run_execution) fn capture_admitted(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    control: &mut TargetExecutionControl<'_>,
    position: PhasePosition,
) -> Result<(CompletedTargetCapture, Option<CompletedExecutionPhase>)> {
    let authority = finisher.freshness.map(|freshness| {
        freshness.before_target(
            finisher.ctx,
            finisher.catalog,
            planned,
            &mut BudgetedObservation(control),
        )
    });
    if let Err(stop) = control.remaining() {
        return Ok((
            CompletedTargetCapture::now(None, stopped_before_start(planned, stop)),
            None,
        ));
    }
    mark_target_started(
        finisher.ctx,
        &finisher.run.result.run_id,
        planned.target.clone(),
    )?;
    let started = now_ms();
    let label = format!("Repository target '{}'", planned.target);
    let phase = ExecutionPhase::start(control, &label, position);
    let mut capture = run_target_with_control(
        finisher.ctx,
        finisher.catalog,
        &finisher.run.result.run_id,
        finisher.work_plan_id,
        planned,
        control,
    );
    if let (Some(freshness), Some(authority)) = (finisher.freshness, authority) {
        capture.freshness_authority = Some(authority.and_then(|guard| {
            freshness.after_target(
                finisher.ctx,
                finisher.catalog,
                planned,
                guard,
                &mut BudgetedObservation(control),
            )
        }));
        capture.authority_started_at_ms = Some(started);
    }
    Ok((
        CompletedTargetCapture::now(Some(started), capture),
        Some(phase.complete_owned()),
    ))
}
