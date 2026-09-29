//! Shared serial/wave execution: never publishes results or releases claims.
use super::*;

pub(in crate::runtime::run_execution) fn capture_admitted(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    control: &mut TargetExecutionControl<'_>,
    position: PhasePosition,
) -> Result<(CompletedTargetCapture, Option<CompletedExecutionPhase>)> {
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
    let capture = run_target_with_control(
        finisher.ctx,
        finisher.catalog,
        &finisher.run.result.run_id,
        planned,
        control,
    );
    Ok((
        CompletedTargetCapture::now(Some(started), capture),
        Some(phase.complete_owned()),
    ))
}
