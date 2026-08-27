use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use super::*;
use crate::execution::ExecutionStream;

const MAX_PARALLEL_LAYER_TARGETS: usize = 8;
const PARALLEL_EVENT_QUEUE_CAPACITY: usize = 64;
const MAX_EVENTS_PER_COORDINATOR_TICK: usize = 64;

pub(super) struct ParallelTargetOutcome {
    pub(super) completed: CompletedTargetCapture,
    pub(super) fingerprint: std::result::Result<String, String>,
}

pub(super) struct ParallelLayerExecution {
    pub(super) outcomes: Vec<ParallelTargetOutcome>,
}

enum ParallelTargetExecution {
    NotStarted {
        completed: CompletedTargetCapture,
        fingerprint: std::result::Result<String, String>,
    },
    Completed {
        completed: CompletedTargetCapture,
        phase: CompletedExecutionPhase,
    },
}

impl ParallelTargetExecution {
    fn not_started(
        capture: TargetCapture,
        fingerprint: std::result::Result<String, String>,
    ) -> Self {
        debug_assert!(!capture.may_have_executed);
        Self::NotStarted {
            completed: CompletedTargetCapture::now(None, capture),
            fingerprint,
        }
    }

    fn completed(
        started_at_ms: u64,
        capture: TargetCapture,
        phase: CompletedExecutionPhase,
    ) -> Self {
        Self::Completed {
            completed: CompletedTargetCapture::now(Some(started_at_ms), capture),
            phase,
        }
    }
}

pub(super) fn execute_parallel_read_only_layer(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: &crate::state::DurableRun,
    control: &mut dyn RepositoryRunControl,
    source_epoch: &mut ExecutionSourceEpoch,
    targets: &[(&PlannedTarget, PhasePosition)],
) -> Result<ParallelLayerExecution> {
    let cancellation = Arc::new(ParallelCancellationState::default());
    let initial_cancellation = control.cancelled();
    let source_may_be_observed = matches!(initial_cancellation, Ok(false));
    cancellation.update(initial_cancellation);
    // Entering a layer creates an observation gap before authority and source
    // preconditions can succeed. Never carry a predecessor observation across
    // an early return or an initially cancelled layer.
    source_epoch.begin_read_only_layer();
    if source_may_be_observed {
        if let Err(error) =
            crate::repository::validate_current_repository_authority(ctx, &run.plan.config_digest)
        {
            let outcomes = targets
                .iter()
                .map(|(planned, _)| {
                    let message = format!(
                        "target '{}' could not start because repository execution authority could not be verified: {error:#}",
                        planned.target
                    );
                    ParallelTargetOutcome {
                        completed: CompletedTargetCapture::now(
                            None,
                            TargetCapture::blocked(message.clone()).with_alias(
                                catalog.aliases_for_target(&planned.target).first().cloned(),
                            ),
                        ),
                        fingerprint: Err(message),
                    }
                })
                .collect();
            return Ok(ParallelLayerExecution { outcomes });
        }
        if let Err(message) = source_epoch.prepare_read_only_layer(ctx, targets.len()) {
            let fingerprint = source_epoch.receipt_fingerprint();
            let outcomes = targets
                .iter()
                .map(|(planned, _)| ParallelTargetOutcome {
                    completed: CompletedTargetCapture::now(
                        None,
                        TargetCapture::blocked(message.clone()).with_alias(
                            catalog.aliases_for_target(&planned.target).first().cloned(),
                        ),
                    ),
                    fingerprint: fingerprint.clone(),
                })
                .collect();
            return Ok(ParallelLayerExecution { outcomes });
        }
    }

    let worker_count = targets.len().min(MAX_PARALLEL_LAYER_TARGETS);
    // Give each worker one stable initial target. Later claims are distinguishable
    // from the cohort covered by the layer-entry source observation and must
    // establish a fresh source precondition before they start.
    let next_target = AtomicUsize::new(worker_count);
    let (event_tx, event_rx) =
        mpsc::sync_channel::<OwnedExecutionEvent>(PARALLEL_EVENT_QUEUE_CAPACITY);
    // An outcome owns the target's bounded stdout/stderr capture, which can be
    // large. Backpressure workers instead of allowing a second unbounded copy
    // of the layer's completed results to accumulate in the channel.
    let (outcome_tx, outcome_rx) =
        mpsc::sync_channel::<(usize, Result<ParallelTargetExecution>)>(worker_count.max(1));

    let completed = {
        let queued_source_epoch = Mutex::new(&mut *source_epoch);
        thread::scope(|scope| -> Result<Vec<ParallelTargetExecution>> {
            let mut workers = Vec::with_capacity(worker_count);
            for initial_index in 0..worker_count {
                let cancellation = Arc::clone(&cancellation);
                let event_tx = event_tx.clone();
                let outcome_tx = outcome_tx.clone();
                let next_target = &next_target;
                let queued_source_epoch = &queued_source_epoch;
                workers.push(scope.spawn(move || {
                    let mut next_index = Some(initial_index);
                    loop {
                        let index = next_index
                            .take()
                            .unwrap_or_else(|| next_target.fetch_add(1, Ordering::Relaxed));
                        let Some(&(planned, position)) = targets.get(index) else {
                            break;
                        };
                        let mut target_control = ParallelTargetControl {
                            cancellation: Arc::clone(&cancellation),
                            events: event_tx.clone(),
                        };
                        let outcome = execute_parallel_target(
                            ctx,
                            catalog,
                            run,
                            planned,
                            position,
                            &mut target_control,
                            (index >= worker_count).then_some(queued_source_epoch),
                        );
                        if outcome_tx.send((index, outcome)).is_err() {
                            break;
                        }
                    }
                }));
            }
            drop(event_tx);
            drop(outcome_tx);

            let mut outcomes = BTreeMap::new();
            while outcomes.len() < targets.len() {
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
                    .map_err(|_| anyhow::anyhow!("parallel repository target worker panicked"))?;
            }
            drain_parallel_events(&event_rx, control);

            (0..targets.len())
                .map(|index| {
                    outcomes.remove(&index).ok_or_else(|| {
                        anyhow::anyhow!(
                            "parallel repository target worker exited without target result"
                        )
                    })?
                })
                .collect()
        })?
    };

    let fingerprint = if source_may_be_observed {
        source_epoch.observe_read_only_layer_postcondition(ctx)
    } else {
        Err("parallel targets did not start, so no execution-time worktree fingerprint was observed"
            .into())
    };
    let outcomes = targets
        .iter()
        .zip(completed)
        .map(|((planned, _), execution)| match execution {
            ParallelTargetExecution::NotStarted {
                completed,
                fingerprint,
            } => ParallelTargetOutcome {
                completed,
                fingerprint,
            },
            ParallelTargetExecution::Completed { completed, phase } => {
                let (completed, target_fingerprint) = source_epoch
                    .finish_started_read_only_layer_target(planned, &fingerprint, completed);
                phase.finish(control, completed.succeeded());
                ParallelTargetOutcome {
                    completed,
                    fingerprint: target_fingerprint,
                }
            }
        })
        .collect();
    Ok(ParallelLayerExecution { outcomes })
}

fn execute_parallel_target(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: &crate::state::DurableRun,
    planned: &PlannedTarget,
    position: PhasePosition,
    control: &mut dyn RepositoryRunControl,
    queued_source_epoch: Option<&Mutex<&mut ExecutionSourceEpoch>>,
) -> Result<ParallelTargetExecution> {
    let execution = match control.cancelled() {
        Ok(true) => ParallelTargetExecution::not_started(
            TargetCapture::not_started(
                RunConclusion::Cancelled,
                "run cancellation was requested before the target started",
            )
            .with_alias(catalog.aliases_for_target(&planned.target).first().cloned()),
            Err(format!(
                "target '{}' did not start, so no execution-time worktree fingerprint was observed",
                planned.target
            )),
        ),
        Err(error) => ParallelTargetExecution::not_started(
            TargetCapture::not_started(
                RunConclusion::Blocked,
                format!("run cancellation state could not be inspected: {error:#}"),
            )
            .with_alias(catalog.aliases_for_target(&planned.target).first().cloned()),
            Err(format!(
                "target '{}' did not start, so no execution-time worktree fingerprint was observed",
                planned.target
            )),
        ),
        Ok(false) => {
            if let Err(error) = crate::repository::validate_current_repository_authority(
                ctx,
                &run.plan.config_digest,
            ) {
                let message = format!(
                    "target '{}' could not start because repository execution authority could not be verified: {error:#}",
                    planned.target
                );
                ParallelTargetExecution::not_started(
                    TargetCapture::blocked(message.clone())
                        .with_alias(catalog.aliases_for_target(&planned.target).first().cloned()),
                    Err(message),
                )
            } else if let Some(execution) = queued_source_epoch.and_then(|source_epoch| {
                revalidate_queued_target_source(ctx, catalog, planned, source_epoch)
            }) {
                execution
            } else {
                mark_target_started(ctx, &run.result.run_id, planned.target.clone())?;
                let started_at_ms = now_ms();
                let label = format!("Repository target '{}'", planned.target);
                let phase = ExecutionPhase::start(control, &label, position);
                let capture = run_target_capture(ctx, catalog, planned, control);
                let phase = phase.complete_owned();
                ParallelTargetExecution::completed(started_at_ms, capture, phase)
            }
        }
    };
    Ok(execution)
}

fn revalidate_queued_target_source(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    planned: &PlannedTarget,
    source_epoch: &Mutex<&mut ExecutionSourceEpoch>,
) -> Option<ParallelTargetExecution> {
    let (precondition, fingerprint) =
        queued_target_source_precondition(planned, source_epoch, || {
            ExecutionSourceObservation::collect(ctx)
        });
    let Err(message) = precondition else {
        return None;
    };
    Some(ParallelTargetExecution::not_started(
        TargetCapture::blocked(message)
            .with_alias(catalog.aliases_for_target(&planned.target).first().cloned()),
        fingerprint,
    ))
}

fn queued_target_source_precondition(
    planned: &PlannedTarget,
    source_epoch: &Mutex<&mut ExecutionSourceEpoch>,
    collect: impl FnOnce() -> ExecutionSourceObservation,
) -> (
    std::result::Result<(), String>,
    std::result::Result<String, String>,
) {
    // Fingerprinting performs repository I/O and is independent for each
    // queued claim. Only the epoch comparison and metrics update need the
    // shared mutex.
    let observation = collect();
    let mut source_epoch = source_epoch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    source_epoch.prepare_queued_read_only_target(planned, observation)
}

fn parallel_outcome_wait(replayed_events: usize) -> Duration {
    if replayed_events == MAX_EVENTS_PER_COORDINATOR_TICK {
        Duration::ZERO
    } else {
        Duration::from_millis(50)
    }
}

#[derive(Default)]
struct ParallelCancellationState {
    cancelled: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl ParallelCancellationState {
    fn update(&self, result: Result<bool>) {
        match result {
            Ok(true) => self.cancelled.store(true, Ordering::Release),
            Ok(false) => {}
            Err(error) => {
                let mut failure = self
                    .failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if failure.is_none() {
                    *failure = Some(format!("{error:#}"));
                }
            }
        }
    }

    fn current(&self) -> Result<bool> {
        if let Some(failure) = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            return Err(anyhow::anyhow!(failure));
        }
        Ok(self.cancelled.load(Ordering::Acquire))
    }
}

struct ParallelTargetControl {
    cancellation: Arc<ParallelCancellationState>,
    events: mpsc::SyncSender<OwnedExecutionEvent>,
}

impl ExecutionObserver for ParallelTargetControl {
    fn event(&mut self, event: ExecutionEvent<'_>) {
        let _ = self.events.send(event.into());
    }
}

impl RepositoryRunControl for ParallelTargetControl {
    fn cancelled(&self) -> Result<bool> {
        self.cancellation.current()
    }
}

enum OwnedExecutionEvent {
    PhaseStarted {
        label: String,
        position: PhasePosition,
    },
    Output {
        stream: ExecutionStream,
        bytes: Vec<u8>,
    },
    Heartbeat {
        label: String,
        elapsed: Duration,
    },
    PhaseFinished {
        label: String,
        success: bool,
        elapsed: Duration,
    },
}

impl From<ExecutionEvent<'_>> for OwnedExecutionEvent {
    fn from(event: ExecutionEvent<'_>) -> Self {
        match event {
            ExecutionEvent::PhaseStarted { label, position } => Self::PhaseStarted {
                label: label.into(),
                position,
            },
            ExecutionEvent::Output { stream, bytes } => Self::Output {
                stream,
                bytes: bytes.to_vec(),
            },
            ExecutionEvent::Heartbeat { label, elapsed } => Self::Heartbeat {
                label: label.into(),
                elapsed,
            },
            ExecutionEvent::PhaseFinished {
                label,
                success,
                elapsed,
            } => Self::PhaseFinished {
                label: label.into(),
                success,
                elapsed,
            },
        }
    }
}

impl OwnedExecutionEvent {
    fn replay(self, control: &mut dyn RepositoryRunControl) {
        match self {
            Self::PhaseStarted { label, position } => {
                control.event(ExecutionEvent::PhaseStarted {
                    label: &label,
                    position,
                });
            }
            Self::Output { stream, bytes } => {
                control.event(ExecutionEvent::Output {
                    stream,
                    bytes: &bytes,
                });
            }
            Self::Heartbeat { label, elapsed } => {
                control.event(ExecutionEvent::Heartbeat {
                    label: &label,
                    elapsed,
                });
            }
            Self::PhaseFinished {
                label,
                success,
                elapsed,
            } => {
                control.event(ExecutionEvent::PhaseFinished {
                    label: &label,
                    success,
                    elapsed,
                });
            }
        }
    }
}

fn replay_parallel_events(
    events: &mpsc::Receiver<OwnedExecutionEvent>,
    control: &mut dyn RepositoryRunControl,
    limit: usize,
) -> usize {
    let mut replayed = 0;
    for _ in 0..limit {
        let Ok(event) = events.try_recv() else {
            break;
        };
        event.replay(control);
        replayed += 1;
    }
    replayed
}

fn drain_parallel_events(
    events: &mpsc::Receiver<OwnedExecutionEvent>,
    control: &mut dyn RepositoryRunControl,
) {
    while let Ok(event) = events.try_recv() {
        event.replay(control);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[derive(Default)]
    struct CountingControl {
        events: usize,
    }

    impl ExecutionObserver for CountingControl {
        fn event(&mut self, _event: ExecutionEvent<'_>) {
            self.events += 1;
        }
    }

    impl RepositoryRunControl for CountingControl {
        fn cancelled(&self) -> Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn saturated_event_batches_are_replayed_without_an_outcome_sleep() {
        let (events_tx, events_rx) = mpsc::sync_channel(MAX_EVENTS_PER_COORDINATOR_TICK);
        let saturated = Arc::new(std::sync::Barrier::new(2));
        let producer_saturated = Arc::clone(&saturated);
        let producer = thread::spawn(move || {
            for _ in 0..MAX_EVENTS_PER_COORDINATOR_TICK {
                events_tx
                    .send(OwnedExecutionEvent::Heartbeat {
                        label: "chatty target".into(),
                        elapsed: Duration::ZERO,
                    })
                    .unwrap();
            }
            producer_saturated.wait();
            events_tx
                .send(OwnedExecutionEvent::Heartbeat {
                    label: "chatty target".into(),
                    elapsed: Duration::ZERO,
                })
                .unwrap();
        });
        saturated.wait();
        let mut control = CountingControl::default();

        let replayed =
            replay_parallel_events(&events_rx, &mut control, MAX_EVENTS_PER_COORDINATOR_TICK);
        producer.join().unwrap();

        assert_eq!(replayed, MAX_EVENTS_PER_COORDINATOR_TICK);
        assert_eq!(control.events, MAX_EVENTS_PER_COORDINATOR_TICK);
        assert!(
            events_rx.try_recv().is_ok(),
            "the event queue is still busy"
        );
        assert_eq!(parallel_outcome_wait(replayed), Duration::ZERO);
    }

    #[test]
    fn queued_source_scans_do_not_serialize_behind_epoch_bookkeeping() {
        let planned = PlannedTarget::new(
            "repo:test".parse().unwrap(),
            jig_contract::ActionIntent::Check,
            ActionRunner::command("test_command"),
            "sha256:input",
        );
        let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
        let epoch = Mutex::new(&mut epoch);
        let collecting = Arc::new((Mutex::new((0_usize, 0_usize)), std::sync::Condvar::new()));
        let overlapped = Arc::new(AtomicBool::new(false));

        thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..2 {
                let collecting = Arc::clone(&collecting);
                let overlapped = Arc::clone(&overlapped);
                let planned = &planned;
                let epoch = &epoch;
                workers.push(scope.spawn(move || {
                    let (precondition, _) =
                        queued_target_source_precondition(planned, epoch, || {
                            let (active, changed) = &*collecting;
                            let mut state = active.lock().unwrap();
                            state.0 += 1;
                            state.1 += 1;
                            changed.notify_all();
                            let (mut state, _) = changed
                                .wait_timeout_while(state, Duration::from_secs(2), |state| {
                                    state.1 < 2
                                })
                                .unwrap();
                            if state.0 >= 2 {
                                overlapped.store(true, Ordering::Release);
                            }
                            state.0 -= 1;
                            changed.notify_all();
                            ExecutionSourceObservation::collect_with(|| Ok("sha256:stable".into()))
                        });
                    precondition.unwrap();
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });

        assert!(
            overlapped.load(Ordering::Acquire),
            "queued fingerprint scans must run outside the epoch mutex"
        );
        assert_eq!(epoch.into_inner().unwrap().metrics().count, 2);
    }
}
