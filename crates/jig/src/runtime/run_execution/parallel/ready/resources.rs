use super::*;

pub(super) fn validate_resource_source(
    ctx: &RepoContext,
    control: &dyn RepositoryRunControl,
    epoch: &mut ExecutionSourceEpoch,
    fingerprint: std::result::Result<String, String>,
    source_failure: &mut Option<String>,
    cancellation: &ParallelCancellationState,
) {
    let cancelled = || {
        cancellation.update(control.cancelled());
        cancellation.current().unwrap_or(true)
    };
    if cancelled() {
        return;
    }
    // Resource observations use the wave's remaining target budgets. Exhaustion
    // leaves its evidence incomplete, but cannot establish a source failure for
    // unrelated targets. Verify independently before cancelling shared work.
    let fingerprint = fingerprint.or_else(|_| {
        epoch.observe_read_only_layer_postcondition_with(|| {
            crate::git_receipts::repository_source_snapshot_with_cancellation(
                ctx.root(),
                &cancelled,
            )
            .map(|snapshot| snapshot.worktree_fingerprint)
            .map_err(|error| format!("{error:#}"))
        })
    });
    // Do not retain cancellation as a source failure. The caller must still
    // publish and acknowledge the resource result so the worker can release claims.
    if !cancelled() {
        retain_source_failure(epoch, &fingerprint, source_failure, cancellation);
    }
}

pub(super) struct ResourceBatch<'a> {
    pub(super) targets: Vec<(usize, (&'a PlannedTarget, PhasePosition))>,
    pub(super) arrivals: mpsc::Receiver<(&'a PlannedTarget, PhasePosition)>,
    pub(super) indices: BTreeMap<TargetId, usize>,
    pub(super) allow_reuse: bool,
    pub(super) slots: ExecutionSlots,
    pub(super) cancellation: Arc<ParallelCancellationState>,
    pub(super) events: mpsc::SyncSender<OwnedExecutionEvent>,
    pub(super) outcomes: mpsc::SyncSender<ReadyOutcome>,
}

pub(super) fn run_resource_batch(finisher: &TargetFinisher<'_>, batch: ResourceBatch<'_>) {
    let mut epoch =
        ExecutionSourceEpoch::from_plan(finisher.run.plan.source.worktree_fingerprint.clone());
    let mut control = ParallelTargetControl {
        cancellation: batch.cancellation,
        events: batch.events,
    };
    let targets = batch
        .targets
        .iter()
        .map(|(_, target)| *target)
        .collect::<Vec<_>>();
    let result = catch_worker(|| {
        execute_resource_layer(
            finisher,
            &mut control,
            &mut epoch,
            ResourceCandidates {
                initial: &targets,
                arrivals: Some(&batch.arrivals),
            },
            batch.allow_reuse,
            &batch.slots,
            &mut |target, result, compatibility, fingerprint, wave_number| {
                let index = *batch
                    .indices
                    .get(target)
                    .expect("resource result belongs to its plan");
                let (acknowledge, acknowledgment) = mpsc::sync_channel(0);
                batch
                    .outcomes
                    .send(ReadyOutcome::Resource {
                        index,
                        result,
                        compatibility,
                        source: fingerprint.zip(wave_number).map(|(fingerprint, number)| {
                            WaveFingerprint {
                                number,
                                fingerprint: fingerprint.clone(),
                            }
                        }),
                        acknowledge,
                    })
                    .map_err(|_| {
                        anyhow::anyhow!("ready coordinator stopped before resource publication")
                    })?;
                // The existing resource executor retains its leases until this
                // callback returns. A dequeue alone is not durable publication.
                acknowledgment.recv().map_err(|_| {
                    anyhow::anyhow!("ready coordinator could not publish a resource result")
                })
            },
        )
    });
    let _ = batch.outcomes.send(ReadyOutcome::ResourcesFinished {
        result,
        metrics: epoch.metrics(),
    });
}
