use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use super::*;
use crate::execution::ExecutionStream;

const MAX_PARALLEL_LAYER_TARGETS: usize = 8;

pub(super) struct ParallelTargetOutcome {
    pub(super) started_at_ms: Option<u64>,
    pub(super) capture: TargetCapture,
    pub(super) fingerprint: std::result::Result<String, String>,
    pub(super) source_observations: SourceObservationMetrics,
}

pub(super) fn execute_parallel_read_only_layer(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: &crate::state::DurableRun,
    control: &mut dyn RepositoryRunControl,
    targets: &[(&PlannedTarget, PhasePosition)],
) -> Result<Vec<ParallelTargetOutcome>> {
    let cancellation = Arc::new(ParallelCancellationState::default());
    cancellation.update(control.cancelled());
    let next_target = AtomicUsize::new(0);
    let (event_tx, event_rx) = mpsc::channel::<OwnedExecutionEvent>();
    let (outcome_tx, outcome_rx) = mpsc::channel::<(usize, Result<ParallelTargetOutcome>)>();
    let worker_count = targets.len().min(MAX_PARALLEL_LAYER_TARGETS);

    thread::scope(|scope| -> Result<Vec<ParallelTargetOutcome>> {
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let cancellation = Arc::clone(&cancellation);
            let event_tx = event_tx.clone();
            let outcome_tx = outcome_tx.clone();
            let next_target = &next_target;
            workers.push(scope.spawn(move || {
                loop {
                    let index = next_target.fetch_add(1, Ordering::Relaxed);
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
            replay_parallel_events(&event_rx, control);
            cancellation.update(control.cancelled());
            match outcome_rx.recv_timeout(Duration::from_millis(50)) {
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
        replay_parallel_events(&event_rx, control);

        (0..targets.len())
            .map(|index| {
                outcomes.remove(&index).ok_or_else(|| {
                    anyhow::anyhow!(
                        "parallel repository target worker exited without target result"
                    )
                })?
            })
            .collect()
    })
}

fn execute_parallel_target(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    run: &crate::state::DurableRun,
    planned: &PlannedTarget,
    position: PhasePosition,
    control: &mut dyn RepositoryRunControl,
) -> Result<ParallelTargetOutcome> {
    let mut source_epoch =
        ExecutionSourceEpoch::from_plan(run.plan.source.worktree_fingerprint.clone());
    let (started_at_ms, capture, fingerprint) = match control.cancelled() {
        Ok(true) => (
            None,
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
        Err(error) => (
            None,
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
                (
                    None,
                    TargetCapture::blocked(message.clone())
                        .with_alias(catalog.aliases_for_target(&planned.target).first().cloned()),
                    Err(message),
                )
            } else if let Err(message) = source_epoch.prepare_target(ctx, planned) {
                (
                    None,
                    TargetCapture::blocked(message)
                        .with_alias(catalog.aliases_for_target(&planned.target).first().cloned()),
                    source_epoch.receipt_fingerprint(),
                )
            } else {
                mark_target_started(ctx, &run.result.run_id, planned.target.clone())?;
                let started_at_ms = now_ms();
                let label = format!("Repository target '{}'", planned.target);
                let phase = ExecutionPhase::start(control, &label, position);
                let (capture, fingerprint) =
                    run_target(ctx, catalog, planned, control, &mut source_epoch);
                phase.finish(control, capture.conclusion == RunConclusion::Success);
                (Some(started_at_ms), capture, fingerprint)
            }
        }
    };
    Ok(ParallelTargetOutcome {
        started_at_ms,
        capture,
        fingerprint,
        source_observations: source_epoch.metrics(),
    })
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
    events: mpsc::Sender<OwnedExecutionEvent>,
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
) {
    while let Ok(event) = events.try_recv() {
        event.replay(control);
    }
}
