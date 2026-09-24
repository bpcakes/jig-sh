//! Dispatch ordinary read-only dependents after validated, durable completion.
use super::*;

mod queue;
mod resources;
use queue::ReadyQueue;
use resources::{ResourceBatch, run_resource_batch, validate_resource_source};

type Publish<'a> = dyn FnMut(&TargetId, TargetRunResult, Option<Value>) -> Result<()> + 'a;

enum ReadyOutcome {
    Ordinary {
        index: usize,
        execution: Result<ParallelTargetExecution>,
    },
    Resource {
        index: usize,
        result: TargetRunResult,
        compatibility: Option<Value>,
        fingerprint: Option<std::result::Result<String, String>>,
        acknowledge: mpsc::SyncSender<()>,
    },
    ResourcesFinished {
        result: Result<()>,
        metrics: SourceObservationMetrics,
    },
}

pub(in crate::runtime::run_execution) fn execute_ready_read_only_targets(
    finisher: &TargetFinisher<'_>,
    control: &mut dyn RepositoryRunControl,
    source_epoch: &mut ExecutionSourceEpoch,
    allow_reuse: bool,
    publish: &mut Publish<'_>,
) -> Result<()> {
    let mut queue = ReadyQueue::new(&finisher.run.plan)?;
    let slots = ExecutionSlots::new();
    let cancellation = Arc::new(ParallelCancellationState::default());
    let (event_tx, event_rx) = mpsc::sync_channel(PARALLEL_EVENT_QUEUE_CAPACITY);
    let (outcome_tx, outcome_rx) = mpsc::sync_channel(MAX_PARALLEL_LAYER_TARGETS);
    let mut source_failure = None;

    thread::scope(|scope| {
        let mut workers = BTreeMap::new();
        let mut resource_worker = None;
        let result = (|| -> Result<()> {
            while queue.unfinished > 0 || resource_worker.is_some() {
                cancellation.update(control.cancelled());
                let mut ordinary = Vec::new();
                let mut resource_targets = Vec::new();
                // Bound resource preparation separately from execution. Only
                // admitted wave members take slots, so resource waiters cannot
                // prevent ordinary targets from using idle capacity.
                for index in queue.ready.iter().copied().collect::<Vec<_>>() {
                    let stop = unstarted_reason(
                        control,
                        queue.failed_dependency[index],
                        source_failure.as_deref(),
                    );
                    if let Some((conclusion, reason)) = stop {
                        source_epoch.discard_reusable_observation();
                        queue.finish_unstarted(index, finisher, conclusion, reason, publish)?;
                        continue;
                    }
                    if slots.is_full() {
                        break;
                    }
                    let target = queue.targets[index];
                    if !target.0.resources.is_empty() {
                        if resource_worker.is_some()
                            || resource_targets.len() == MAX_PARALLEL_LAYER_TARGETS
                        {
                            continue;
                        }
                        resource_targets.push((index, target));
                    } else {
                        let Some(slot) = slots.try_acquire() else {
                            break;
                        };
                        ordinary.push((index, target, slot));
                    }
                    queue.ready.remove(&index);
                }

                if !ordinary.is_empty() {
                    let precondition = crate::repository::validate_current_repository_authority(
                        finisher.ctx,
                        &finisher.run.plan.config_digest,
                    )
                    .map_err(|error| {
                        format!("repository execution authority could not be verified: {error:#}")
                    })
                    .and_then(|()| source_epoch.prepare_target(finisher.ctx, ordinary[0].1.0));
                    if let Err(message) = precondition {
                        source_failure = Some(message.clone());
                        cancellation.cancelled.store(true, Ordering::Release);
                        for index in ordinary
                            .into_iter()
                            .map(|(index, _, _)| index)
                            .chain(resource_targets.into_iter().map(|(index, _)| index))
                        {
                            queue.finish_unstarted(
                                index,
                                finisher,
                                RunConclusion::Blocked,
                                message.clone(),
                                publish,
                            )?;
                        }
                        continue;
                    }
                }

                for (index, target, slot) in ordinary {
                    let outcomes = outcome_tx.clone();
                    let mut target_control = ParallelTargetControl {
                        cancellation: Arc::clone(&cancellation),
                        events: event_tx.clone(),
                    };
                    workers.insert(
                        index,
                        (
                            scope.spawn(move || {
                                let execution = catch_worker(|| {
                                    execute_parallel_target(
                                        finisher.ctx,
                                        finisher.catalog,
                                        finisher.run,
                                        target,
                                        &mut target_control,
                                        None,
                                        finisher.freshness,
                                    )
                                });
                                let _ = outcomes.send(ReadyOutcome::Ordinary { index, execution });
                            }),
                            slot,
                        ),
                    );
                }
                if !resource_targets.is_empty() {
                    let batch = ResourceBatch {
                        targets: resource_targets,
                        allow_reuse,
                        slots: slots.clone(),
                        cancellation: Arc::clone(&cancellation),
                        events: event_tx.clone(),
                        outcomes: outcome_tx.clone(),
                    };
                    resource_worker =
                        Some(scope.spawn(move || run_resource_batch(finisher, batch)));
                }

                if workers.is_empty() && resource_worker.is_none() {
                    if queue.unfinished == 0 {
                        break;
                    }
                    if !queue.ready.is_empty() {
                        // A skipped parent may have made another skipped child ready.
                        continue;
                    }
                    bail!(
                        "ready scheduler has unfinished targets without a runnable dependency path"
                    );
                }
                let replayed =
                    replay_parallel_events(&event_rx, control, MAX_EVENTS_PER_COORDINATOR_TICK);
                control.flush()?;
                cancellation.update(control.cancelled());
                source_epoch.discard_reusable_observation();
                let first = match outcome_rx.recv_timeout(parallel_outcome_wait(replayed)) {
                    Ok(outcome) => outcome,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        bail!("ready target workers omitted their results")
                    }
                };
                let mut outcomes = vec![first];
                // The admission bound also bounds retained captures. Batch only
                // completions already available; never wait to fill this batch.
                outcomes.extend(outcome_rx.try_iter().take(MAX_PARALLEL_LAYER_TARGETS - 1));
                // Workers send all start/output events before their outcome.
                // At most one full channel of those events can remain queued;
                // replay them before reporting completion, even under noisy peers.
                replay_parallel_events(&event_rx, control, PARALLEL_EVENT_QUEUE_CAPACITY);
                let mut ordinary = Vec::new();
                for outcome in outcomes {
                    match outcome {
                        ReadyOutcome::Ordinary { index, execution } => {
                            let (worker, slot) =
                                workers.remove(&index).expect("ordinary worker exists");
                            join_worker(worker)?;
                            ordinary.push((index, execution?, slot));
                        }
                        ReadyOutcome::Resource {
                            index,
                            result,
                            compatibility,
                            fingerprint,
                            acknowledge,
                        } => {
                            if let Some(fingerprint) = fingerprint {
                                validate_resource_source(
                                    finisher.ctx,
                                    control,
                                    source_epoch,
                                    fingerprint,
                                    &mut source_failure,
                                    &cancellation,
                                );
                            }
                            queue.publish(index, result, compatibility, publish)?;
                            acknowledge.send(()).map_err(|_| {
                                anyhow::anyhow!("resource worker stopped during publication")
                            })?;
                        }
                        ReadyOutcome::ResourcesFinished { result, metrics } => {
                            source_epoch.include_metrics(metrics);
                            join_worker(resource_worker.take().expect("resource worker exists"))?;
                            result?;
                        }
                    }
                }
                let fingerprint = if ordinary.iter().any(|(_, execution, _)| {
                    matches!(execution, ParallelTargetExecution::Completed { .. })
                }) {
                    let observed = source_epoch.observe_ready_read_only_postcondition(finisher.ctx);
                    let fingerprint = source_failure
                        .as_ref()
                        .map_or_else(|| observed.clone(), |error: &String| Err(error.clone()));
                    retain_source_failure(
                        source_epoch,
                        &observed,
                        &mut source_failure,
                        &cancellation,
                    );
                    fingerprint
                } else {
                    Err("no ordinary target in this completion batch started".into())
                };
                for (index, execution, slot) in ordinary {
                    let planned = queue.targets[index].0;
                    let (completed, fingerprint) = match execution {
                        ParallelTargetExecution::NotStarted {
                            completed,
                            fingerprint,
                        } => (completed, fingerprint),
                        ParallelTargetExecution::Completed { completed, phase } => {
                            let completed = source_epoch.finish_read_only_completion(
                                planned,
                                &fingerprint,
                                completed,
                            );
                            phase.finish(control, completed.succeeded());
                            (completed, fingerprint.clone())
                        }
                    };
                    let (result, compatibility) =
                        finisher.finish(planned, completed, fingerprint)?;
                    queue.publish(index, result, compatibility, publish)?;
                    // Bound retained captures as well as running children.
                    drop(slot);
                }
            }
            drain_parallel_events(&event_rx, control);
            control.flush()
        })();

        // Release blocked event senders and publication acknowledgments before
        // scoped joins. This path also runs after any persistence/observer error.
        if result.is_err() {
            cancellation.cancelled.store(true, Ordering::Release);
        }
        drop(event_rx);
        drop(outcome_rx);
        drop(event_tx);
        drop(outcome_tx);
        let mut joins = Ok(());
        for (worker, _slot) in workers.into_values() {
            if let Err(error) = join_worker(worker) {
                joins = Err(error);
            }
        }
        if let Some(worker) = resource_worker
            && let Err(error) = join_worker(worker)
        {
            joins = Err(error);
        }
        result.and(joins)
    })
}

fn unstarted_reason(
    control: &dyn RepositoryRunControl,
    dependency_failed: bool,
    source_failure: Option<&str>,
) -> Option<(RunConclusion, String)> {
    match control.cancelled() {
        Ok(true) => Some((
            RunConclusion::Cancelled,
            "run cancellation was requested before the target started".into(),
        )),
        Err(error) => Some((
            RunConclusion::Blocked,
            format!("run cancellation state could not be inspected: {error:#}"),
        )),
        Ok(false) if dependency_failed => Some((
            RunConclusion::Skipped,
            "a declared target dependency did not succeed".into(),
        )),
        Ok(false) => source_failure.map(|reason| (RunConclusion::Blocked, reason.into())),
    }
}

fn retain_source_failure(
    epoch: &ExecutionSourceEpoch,
    fingerprint: &std::result::Result<String, String>,
    failure: &mut Option<String>,
    cancellation: &ParallelCancellationState,
) {
    if !epoch.read_only_postcondition_matches(fingerprint) {
        failure.get_or_insert_with(|| match fingerprint {
            Ok(_) => "repository source changed during read-only execution; plan again".into(),
            Err(error) => format!("repository source could not be verified: {error}"),
        });
        cancellation.cancelled.store(true, Ordering::Release);
    }
}

fn catch_worker<T>(run: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("ready repository target worker panicked")))
}

fn join_worker(worker: thread::ScopedJoinHandle<'_, ()>) -> Result<()> {
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("ready repository target worker panicked"))
}
