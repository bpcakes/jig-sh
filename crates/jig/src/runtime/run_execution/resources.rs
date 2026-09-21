use super::*;
use crate::repository::{
    cargo_resources,
    execution_resources::{self, ResolvedResources},
};
use crate::state::ResourceLease;
use target::TargetBudget;
mod execution;
pub(super) mod reuse;
pub(super) use execution::{capture_admitted, reuse_candidate};

pub(super) struct CoordinatedOutcome {
    pub(super) result: TargetRunResult,
    pub(super) compatibility: Option<Value>,
    pub(super) lease: Option<ResourceLease>,
}

pub(super) fn execute_coordinated_target(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    run_control: &mut dyn RepositoryRunControl,
    source_epoch: &mut ExecutionSourceEpoch,
    position: PhasePosition,
    allow_reuse: bool,
) -> Result<CoordinatedOutcome> {
    let budget = TargetBudget::new(finisher.ctx, planned);
    source_epoch.discard_reusable_observation();
    let admission = {
        let mut control = TargetExecutionControl::with_budget(budget, run_control, None);
        acquire(finisher.ctx, planned, &mut control)
    };
    let (lease, resolved, waited) = match admission {
        Ok(admission) => admission,
        Err(stop) => {
            let capture = stopped_before_start(planned, stop);
            let (result, compatibility) = finisher.finish(
                planned,
                CompletedTargetCapture::now(None, capture),
                Err("execution resource admission did not complete; no child was started".into()),
            )?;
            return Ok(CoordinatedOutcome {
                result,
                compatibility,
                lease: None,
            });
        }
    };
    let mut control = TargetExecutionControl::with_budget(budget, run_control, Some(&lease));
    let outcome = execute_admitted(
        finisher,
        planned,
        &mut control,
        source_epoch,
        position,
        &resolved,
        allow_reuse && waited,
    );
    drop(control);
    let (result, compatibility) = outcome?;
    Ok(CoordinatedOutcome {
        result,
        compatibility,
        lease: Some(lease),
    })
}

fn acquire(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    control: &mut TargetExecutionControl<'_>,
) -> std::result::Result<(ResourceLease, ResolvedResources, bool), TargetStop> {
    let resolved = resolve(ctx, planned, control)?;
    if let Some(reason) = resolved.partial_reason {
        let message = format!("Cargo resource coordination is partial: {reason}\n");
        control.event(ExecutionEvent::Output {
            stream: ExecutionStream::Stderr,
            bytes: message.as_bytes(),
        });
        control.flush().map_err(|_| {
            TargetStop::Blocked("resource diagnostic could not be delivered".into())
        })?;
    }
    let mut waited = false;
    loop {
        control.remaining()?;
        match ResourceLease::try_acquire(&resolved.claims) {
            Ok(Some(lease)) => {
                control.remaining()?;
                return Ok((lease, resolved, waited));
            }
            Ok(None) => {}
            Err(_) => {
                return Err(TargetStop::Blocked(
                    "private execution resource ownership could not be established".into(),
                ));
            }
        }
        if !waited {
            control.event(ExecutionEvent::Output {
                stream: ExecutionStream::Stderr,
                bytes: execution_resources::waiting_message(planned),
            });
            control.flush().map_err(|_| {
                TargetStop::Blocked("resource wait diagnostic could not be delivered".into())
            })?;
            waited = true;
        }
        std::thread::sleep(control.remaining()?.min(Duration::from_millis(25)));
    }
}

pub(super) fn resolve(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    control: &TargetExecutionControl<'_>,
) -> std::result::Result<ResolvedResources, TargetStop> {
    let resolved = execution_resources::resolve(ctx, planned, control.remaining()?, &|| {
        control.remaining().is_err()
    });
    // Translate a deadline observed by a cancellation-aware collector back to
    // the actual stop cause, rather than labelling a timeout cancellation.
    control.remaining()?;
    resolved.map_err(
        |error| match error.downcast_ref::<cargo_resources::CargoResourceStop>() {
            Some(cargo_resources::CargoResourceStop::TimedOut) => TargetStop::TimedOut,
            Some(cargo_resources::CargoResourceStop::Cancelled) => TargetStop::Cancelled,
            None => TargetStop::Blocked("execution resource identity could not be established; verify the declared resource policy and its prerequisites".into()),
        },
    )
}

fn post_admission(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    control: &TargetExecutionControl<'_>,
    source_epoch: &mut ExecutionSourceEpoch,
    resolved: &ResolvedResources,
) -> std::result::Result<(), TargetStop> {
    revalidate_authority(finisher, planned, control, resolved)?;
    source_epoch.discard_reusable_observation();
    let source = source_epoch.prepare_target_with(planned, || fingerprint(finisher.ctx, control));
    control.remaining()?;
    source.map_err(TargetStop::Blocked)
}

pub(super) fn revalidate_authority(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    control: &TargetExecutionControl<'_>,
    resolved: &ResolvedResources,
) -> std::result::Result<(), TargetStop> {
    control.remaining()?;
    crate::repository::validate_current_repository_authority(
        finisher.ctx,
        &finisher.run.plan.config_digest,
    )
    .map_err(|_| {
        TargetStop::Blocked(
            "repository execution authority changed while awaiting an execution resource; plan again"
                .into(),
        )
    })?;
    if !resolved.same_identity(&resolve(finisher.ctx, planned, control)?) {
        return Err(TargetStop::Blocked(
            "execution resource authority changed while waiting; plan again".into(),
        ));
    }
    control.remaining()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_admitted(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    control: &mut TargetExecutionControl<'_>,
    source_epoch: &mut ExecutionSourceEpoch,
    position: PhasePosition,
    resolved: &ResolvedResources,
    allow_reuse: bool,
) -> Result<(TargetRunResult, Option<Value>)> {
    let preparation = post_admission(finisher, planned, control, source_epoch, resolved);
    if let Err(stop) = preparation {
        return finish_unstarted(finisher, planned, source_epoch, resolved, stop);
    }
    match reuse_candidate(finisher, planned, control, allow_reuse) {
        Err(stop) => return finish_unstarted(finisher, planned, source_epoch, resolved, stop),
        Ok(Some(result)) => {
            if let Err(stop) = post_admission(finisher, planned, control, source_epoch, resolved) {
                return finish_unstarted(finisher, planned, source_epoch, resolved, stop);
            }
            if let Some(result) = reuse::finalize_reuse(result, resolved.partial_reason, now_ms()) {
                control.event(ExecutionEvent::Output {
                    stream: ExecutionStream::Stderr,
                    bytes: b"Reusing verified original target evidence after the Cargo resource wait.\n",
                });
                return Ok((result, None));
            }
            // Expiry during the final admission scan requires actual execution,
            // using the same remaining target budget.
        }
        Ok(None) => {}
    }
    let (completed, phase) = capture_admitted(finisher, planned, control, position)?;
    let started = completed.started_at_ms;
    let (mut capture, fingerprint) =
        source_epoch.finish_target_with(planned, completed.capture, || {
            fingerprint(finisher.ctx, control)
        });
    if let Err(stop) = control.remaining()
        && matches!(
            capture.conclusion,
            RunConclusion::Success | RunConclusion::Blocked
        )
    {
        let stopped = stopped_before_start(planned, stop);
        capture.conclusion = stopped.conclusion;
        capture.receipt_exit_status = 1;
        capture.stderr.push_str(&stopped.stderr);
    }
    explain_partial(&mut capture, resolved);
    let completed = CompletedTargetCapture::now(started, capture);
    if let Some(phase) = phase {
        phase.finish(control, completed.succeeded());
    }
    finisher.finish(planned, completed, fingerprint)
}

fn finish_unstarted(
    finisher: &TargetFinisher<'_>,
    planned: &PlannedTarget,
    source_epoch: &ExecutionSourceEpoch,
    resolved: &ResolvedResources,
    stop: TargetStop,
) -> Result<(TargetRunResult, Option<Value>)> {
    let mut capture = stopped_before_start(planned, stop);
    explain_partial(&mut capture, resolved);
    finisher.finish(
        planned,
        CompletedTargetCapture::now(None, capture),
        source_epoch.receipt_fingerprint(),
    )
}

pub(super) fn explain_partial(capture: &mut TargetCapture, resolved: &ResolvedResources) {
    if let Some(reason) = resolved.partial_reason {
        let message = format!("Cargo resource coordination is partial: {reason}");
        capture.stderr.push_str(&format!("{message}\n"));
        let mut warning = finding(message, "cargo_resource");
        warning.severity = FindingSeverity::Warning;
        capture.findings.push(warning);
    }
}

fn fingerprint(
    ctx: &RepoContext,
    control: &TargetExecutionControl<'_>,
) -> std::result::Result<String, String> {
    crate::git_receipts::repository_source_snapshot_with_cancellation(ctx.root(), &|| {
        control.remaining().is_err()
    })
    .map(|snapshot| snapshot.worktree_fingerprint)
    .map_err(|_| {
        "source authority could not be established within the resource target budget".into()
    })
}

struct BudgetedObservation<'a, 'b>(&'a mut TargetExecutionControl<'b>);

impl ExecutionObserver for BudgetedObservation<'_, '_> {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        self.0.event(event);
    }
    fn flush(&mut self) -> Result<()> {
        self.0.flush()
    }
}

impl RepositoryRunControl for BudgetedObservation<'_, '_> {
    fn cancelled(&self) -> Result<bool> {
        Ok(self.0.remaining().is_err())
    }
}
