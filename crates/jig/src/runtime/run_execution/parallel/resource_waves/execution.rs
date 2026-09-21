use super::*;

pub(super) enum Prepared {
    Run,
    Reused(Box<TargetRunResult>),
    Stopped(TargetStop),
}

pub(super) enum WaveOutcome {
    Captured(CompletedTargetCapture, Option<CompletedExecutionPhase>),
    Reused(TargetRunResult),
}

pub(super) fn prepare_wave(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    pending: &[Pending<'_>],
    wave: &[Member],
    allow_reuse: bool,
) -> Vec<Prepared> {
    wave.iter()
        .map(|member| {
            let pending = &pending[member.index];
            if pending.planned.resources.is_empty() {
                // The ordinary executor owns cancellation/authority checks and
                // starts its command budget after freshness preparation.
                return Prepared::Run;
            }
            let target_control = TargetExecutionControl::with_budget(
                pending.budget.expect("admitted budget"),
                control,
                member.lease.as_ref(),
            );
            let prepare = || -> std::result::Result<Prepared, TargetStop> {
                target_control.remaining()?;
                crate::repository::validate_current_repository_authority(
                    finisher.ctx,
                    &finisher.run.plan.config_digest,
                )
                .map_err(|_| {
                    TargetStop::Blocked(
                        "repository authority changed during resource admission".into(),
                    )
                })?;
                if let Some(resolved) = &pending.resolved {
                    resources::revalidate_authority(
                        finisher,
                        pending.planned,
                        &target_control,
                        resolved,
                    )?;
                    if let Some(result) = resources::reuse_candidate(
                        finisher,
                        pending.planned,
                        &target_control,
                        allow_reuse && pending.waited && !pending.force_execution,
                    )? {
                        // The proof query performs bounded source/journal work.
                        // Re-establish the resource and configuration authority
                        // afterwards, as on the serial admission path.
                        resources::revalidate_authority(
                            finisher,
                            pending.planned,
                            &target_control,
                            resolved,
                        )?;
                        return Ok(Prepared::Reused(Box::new(result)));
                    }
                }
                Ok(Prepared::Run)
            };
            prepare().unwrap_or_else(Prepared::Stopped)
        })
        .collect()
}

pub(super) fn execute_wave(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    pending: &[Pending<'_>],
    wave: &[Member],
    prepared: Vec<Prepared>,
    precondition: std::result::Result<(), String>,
) -> Result<Vec<WaveOutcome>> {
    let cancellation = Arc::new(ParallelCancellationState::default());
    cancellation.update(control.cancelled());
    let (event_tx, event_rx) = mpsc::sync_channel(PARALLEL_EVENT_QUEUE_CAPACITY);
    let (outcome_tx, outcome_rx) = mpsc::sync_channel(wave.len());
    thread::scope(|scope| {
        let mut workers = Vec::new();
        for (index, (member, prepared)) in wave.iter().zip(prepared).enumerate() {
            let pending = &pending[member.index];
            let cancellation = Arc::clone(&cancellation);
            let event_tx = event_tx.clone();
            let outcome_tx = outcome_tx.clone();
            let precondition = &precondition;
            workers.push(scope.spawn(move || {
                let mut run_control = ParallelTargetControl {
                    cancellation,
                    events: event_tx,
                };
                let prepared = match (prepared, precondition) {
                    (Prepared::Stopped(stop), _) => Prepared::Stopped(stop),
                    (_, Err(message)) => Prepared::Stopped(TargetStop::Blocked(message.clone())),
                    (prepared, Ok(())) => prepared,
                };
                let outcome = match prepared {
                    Prepared::Stopped(stop) => Ok(WaveOutcome::Captured(
                        CompletedTargetCapture::now(
                            None,
                            stopped_before_start(pending.planned, stop),
                        ),
                        None,
                    )),
                    Prepared::Reused(result) => Ok(WaveOutcome::Reused(*result)),
                    Prepared::Run if pending.planned.resources.is_empty() => {
                        execute_parallel_target(
                            finisher.ctx,
                            finisher.catalog,
                            finisher.run,
                            (pending.planned, pending.position),
                            &mut run_control,
                            None,
                            finisher.freshness,
                        )
                        .map(|execution| match execution {
                            ParallelTargetExecution::NotStarted { completed, .. } => {
                                WaveOutcome::Captured(completed, None)
                            }
                            ParallelTargetExecution::Completed { completed, phase } => {
                                WaveOutcome::Captured(completed, Some(phase))
                            }
                        })
                    }
                    Prepared::Run => {
                        let mut target_control = TargetExecutionControl::with_budget(
                            pending.budget.expect("resource admission budget"),
                            &mut run_control,
                            member.lease.as_ref(),
                        );
                        resources::capture_admitted(
                            finisher,
                            pending.planned,
                            &mut target_control,
                            pending.position,
                        )
                        .map(|(completed, phase)| WaveOutcome::Captured(completed, phase))
                    }
                };
                let _ = outcome_tx.send((index, outcome));
            }));
        }
        drop(event_tx);
        drop(outcome_tx);
        let mut outcomes = BTreeMap::new();
        while outcomes.len() < wave.len() {
            let replayed =
                replay_parallel_events(&event_rx, control, MAX_EVENTS_PER_COORDINATOR_TICK);
            cancellation.update(control.cancelled());
            match outcome_rx.recv_timeout(parallel_outcome_wait(replayed)) {
                Ok((index, outcome)) => {
                    outcomes.insert(index, outcome);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("resource wave worker panicked"))?;
        }
        drain_parallel_events(&event_rx, control);
        (0..wave.len())
            .map(|index| {
                outcomes
                    .remove(&index)
                    .ok_or_else(|| anyhow::anyhow!("resource wave worker omitted its outcome"))?
            })
            .collect()
    })
}
