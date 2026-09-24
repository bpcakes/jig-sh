//! Admit without hold-and-wait, then publish only after the cohort source check.
use super::*;
use crate::repository::execution_resources::{self, ResolvedResources};
use crate::runtime::run_execution::resources;
use crate::runtime::run_execution::target::TargetBudget;
use crate::state::ResourceLease;

mod execution;
use execution::{WaveOutcome, execute_wave, prepare_wave};

struct Pending<'a> {
    planned: &'a PlannedTarget,
    position: PhasePosition,
    budget: Option<TargetBudget>,
    resolved: Option<ResolvedResources>,
    waited: bool,
    force_execution: bool,
    done: bool,
}

struct Member {
    index: usize,
    lease: Option<ResourceLease>,
    // Drop the lease before making its execution capacity available again.
    _slot: ExecutionSlot,
}

type Publish<'a> = dyn FnMut(
        &TargetId,
        TargetRunResult,
        Option<Value>,
        Option<&std::result::Result<String, String>>,
    ) -> Result<()>
    + 'a;
type StoppedAdmissions = Vec<(usize, TargetStop)>;

pub(in crate::runtime::run_execution) fn execute_resource_layer(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    source_epoch: &mut ExecutionSourceEpoch,
    targets: &[(&PlannedTarget, PhasePosition)],
    allow_reuse: bool,
    slots: &ExecutionSlots,
    publish: &mut Publish<'_>,
) -> Result<()> {
    let mut pending = targets
        .iter()
        .map(|(planned, position)| Pending {
            planned,
            position: *position,
            budget: None,
            resolved: None,
            waited: false,
            force_execution: false,
            done: false,
        })
        .collect::<Vec<_>>();
    source_epoch.begin_read_only_layer();
    while pending.iter().any(|pending| !pending.done) {
        // This pass owns no build claim. Never run a resolver for a not-yet-
        // admitted candidate while retaining another candidate's resource.
        resolve_pending(finisher, control, &mut pending, publish)?;
        let (wave, stopped) = admit_wave(finisher, control, &mut pending, slots)?;
        if wave.is_empty() {
            publish_stopped(finisher, &mut pending, stopped, publish)?;
            if pending.iter().any(|pending| !pending.done) {
                let remaining = pending
                    .iter()
                    .filter(|pending| !pending.done)
                    .filter_map(|pending| pending.budget)
                    .map(TargetBudget::remaining_time)
                    .min()
                    .unwrap_or(Duration::from_millis(25));
                thread::sleep(remaining.min(Duration::from_millis(25)));
            }
            continue;
        }
        let prepared = prepare_wave(finisher, control, &pending, &wave, allow_reuse);
        source_epoch.begin_read_only_layer();
        let precondition = source_epoch.prepare_read_only_layer_with(wave.len(), || {
            wave_fingerprint(finisher.ctx, control, &pending, &wave)
        });
        let outcomes = execute_wave(finisher, control, &pending, &wave, prepared, precondition)?;
        let fingerprint = source_epoch.observe_read_only_layer_postcondition_with(|| {
            wave_fingerprint(finisher.ctx, control, &pending, &wave)
        });
        for (member, outcome) in wave.iter().zip(outcomes) {
            publish_outcome(
                finisher,
                control,
                source_epoch,
                &mut pending[member.index],
                member,
                outcome,
                &fingerprint,
                publish,
            )?;
        }
        // Every child is cleaned up and every wave receipt/result is published
        // before any claim can be released or another wave admitted.
        drop(wave);
        publish_stopped(finisher, &mut pending, stopped, publish)?;
    }
    Ok(())
}

fn resolve_pending(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    pending: &mut [Pending<'_>],
    publish: &mut Publish<'_>,
) -> Result<()> {
    for pending in pending.iter_mut().filter(|pending| !pending.done) {
        if pending.planned.resources.is_empty() {
            continue;
        }
        let budget = *pending
            .budget
            .get_or_insert_with(|| TargetBudget::new(finisher.ctx, pending.planned));
        let target_control = TargetExecutionControl::with_budget(budget, control, None);
        let resolution = target_control.remaining().and_then(|_| {
            if let Some(resolved) = &pending.resolved {
                Ok(resolved.clone())
            } else {
                resources::resolve(finisher.ctx, pending.planned, &target_control)
            }
        });
        match resolution {
            Ok(resolved) => pending.resolved = Some(resolved),
            Err(stop) => publish_unstarted(finisher, pending, stop, publish)?,
        }
    }
    Ok(())
}

fn admit_wave(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    pending: &mut [Pending<'_>],
    slots: &ExecutionSlots,
) -> Result<(Vec<Member>, StoppedAdmissions)> {
    let mut wave = Vec::new();
    let mut stopped = Vec::new();
    for (index, pending) in pending
        .iter_mut()
        .enumerate()
        .filter(|(_, pending)| !pending.done)
    {
        let Some(slot) = slots.try_acquire() else {
            break;
        };
        if pending.planned.resources.is_empty() {
            // Unopted peers retain their ordinary execution-only timeout.
            // Admission and a resource sibling's preparation spend no budget.
            wave.push(Member {
                index,
                lease: None,
                _slot: slot,
            });
            continue;
        }
        let budget = *pending
            .budget
            .get_or_insert_with(|| TargetBudget::new(finisher.ctx, pending.planned));
        let target_control = TargetExecutionControl::with_budget(budget, control, None);
        if let Err(stop) = target_control.remaining() {
            stopped.push((index, stop));
            continue;
        }
        let lease = match &pending.resolved {
            None => Some(None),
            Some(resolved) => match ResourceLease::try_acquire(&resolved.claims) {
                Ok(Some(lease)) => Some(Some(lease)),
                Ok(None) => {
                    if !pending.waited {
                        pending.waited = true;
                        control.event(ExecutionEvent::Output {
                            stream: ExecutionStream::Stderr,
                            bytes: execution_resources::waiting_message(pending.planned),
                        });
                    }
                    None
                }
                Err(_) => {
                    stopped.push((
                        index,
                        TargetStop::Blocked(
                            "private execution resource ownership could not be established".into(),
                        ),
                    ));
                    None
                }
            },
        };
        if let Some(lease) = lease {
            wave.push(Member {
                index,
                lease,
                _slot: slot,
            });
        }
        // A failed claim attempt releases its slot immediately; it must not
        // reserve capacity while waiting for another member's resource.
    }
    // No lease waits, metadata, or receipt writes occur in the admission scan.
    // Flush may report an observer failure; dropping the vector then releases
    // every admitted claim without ever spawning a child.
    control.flush()?;
    Ok((wave, stopped))
}

fn publish_stopped(
    finisher: &TargetFinisher<'_>,
    pending: &mut [Pending<'_>],
    stopped: Vec<(usize, TargetStop)>,
    publish: &mut Publish<'_>,
) -> Result<()> {
    for (index, stop) in stopped {
        publish_unstarted(finisher, &mut pending[index], stop, publish)?;
    }
    Ok(())
}

fn publish_unstarted(
    finisher: &TargetFinisher<'_>,
    pending: &mut Pending<'_>,
    stop: TargetStop,
    publish: &mut Publish<'_>,
) -> Result<()> {
    let mut capture = stopped_before_start(pending.planned, stop);
    if let Some(resolved) = &pending.resolved {
        resources::explain_partial(&mut capture, resolved);
    }
    let (result, compatibility) = finisher.finish(
        pending.planned,
        CompletedTargetCapture::now(None, capture),
        Err("resource wave admission did not complete; no child was started".into()),
    )?;
    publish(&pending.planned.target, result, compatibility, None)?;
    pending.done = true;
    Ok(())
}

fn wave_fingerprint(
    ctx: &RepoContext,
    control: &dyn RepositoryRunControl,
    pending: &[Pending<'_>],
    wave: &[Member],
) -> std::result::Result<String, String> {
    let cancelled = || {
        control.cancelled().unwrap_or(true)
            || wave.iter().all(|member| {
                pending[member.index]
                    .budget
                    .is_some_and(|budget| budget.remaining_time().is_zero())
            })
    };
    crate::git_receipts::repository_source_snapshot_with_cancellation(ctx.root(), &cancelled)
        .map(|snapshot| snapshot.worktree_fingerprint)
        .map_err(|_| {
            "resource wave source authority could not be established within its remaining budget"
                .into()
        })
}

#[allow(clippy::too_many_arguments)]
fn publish_outcome(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    source_epoch: &ExecutionSourceEpoch,
    pending: &mut Pending<'_>,
    member: &Member,
    outcome: WaveOutcome,
    fingerprint: &std::result::Result<String, String>,
    publish: &mut Publish<'_>,
) -> Result<()> {
    // Only opted-in resource owners budget through publication. An ordinary
    // peer's completed capture must not be re-timed while waiting for siblings.
    let stop = pending.budget.and_then(|budget| {
        TargetExecutionControl::with_budget(budget, control, member.lease.as_ref())
            .remaining()
            .err()
    });
    let (mut completed, phase) = match outcome {
        WaveOutcome::Reused(result) => {
            if let Some(stop) = stop {
                return publish_unstarted(finisher, pending, stop, publish);
            }
            let result = finalize_wave_reuse(pending, source_epoch, fingerprint, result, now_ms());
            match result {
                Ok(Some(result)) => {
                    publish(&pending.planned.target, result, None, Some(fingerprint))?;
                    pending.done = true;
                }
                Ok(None) => {}
                Err(stop) => return publish_unstarted(finisher, pending, stop, publish),
            }
            return Ok(());
        }
        WaveOutcome::Captured(completed, phase) => (completed, phase),
    };
    if completed.was_started() {
        completed = source_epoch
            .finish_started_read_only_layer_target(pending.planned, fingerprint, completed)
            .0;
    }
    if let Some(stop) = stop
        && matches!(
            completed.capture.conclusion,
            RunConclusion::Success | RunConclusion::Blocked
        )
    {
        let stopped = stopped_before_start(pending.planned, stop);
        completed.capture.conclusion = stopped.conclusion;
        completed.capture.receipt_exit_status = 1;
        completed.capture.stderr.push_str(&stopped.stderr);
    }
    if let Some(resolved) = &pending.resolved {
        resources::explain_partial(&mut completed.capture, resolved);
    }
    if let Some(phase) = phase {
        phase.finish(control, completed.succeeded());
    }
    let (result, compatibility) =
        finisher.finish(pending.planned, completed, fingerprint.clone())?;
    publish(
        &pending.planned.target,
        result,
        compatibility,
        Some(fingerprint),
    )?;
    pending.done = true;
    Ok(())
}

fn finalize_wave_reuse(
    pending: &mut Pending<'_>,
    source_epoch: &ExecutionSourceEpoch,
    fingerprint: &std::result::Result<String, String>,
    result: TargetRunResult,
    observed_at_ms: u64,
) -> std::result::Result<Option<TargetRunResult>, TargetStop> {
    if !source_epoch.read_only_postcondition_matches(fingerprint) {
        return Err(TargetStop::Blocked(
            "repository source changed during the resource wave; original evidence cannot be reused"
                .into(),
        ));
    }
    let result = resources::reuse::finalize_reuse(
        result,
        pending
            .resolved
            .as_ref()
            .and_then(|resolved| resolved.partial_reason),
        observed_at_ms,
    );
    if result.is_none() {
        pending.force_execution = true;
        pending.resolved = None;
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
