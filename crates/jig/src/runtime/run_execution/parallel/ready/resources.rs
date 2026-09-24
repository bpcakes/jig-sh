use super::*;

pub(super) struct ResourceBatch<'a> {
    pub(super) targets: Vec<(usize, (&'a PlannedTarget, PhasePosition))>,
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
            &targets,
            batch.allow_reuse,
            &batch.slots,
            &mut |target, result, compatibility, fingerprint| {
                let index = batch
                    .targets
                    .iter()
                    .find(|(_, (planned, _))| &planned.target == target)
                    .expect("resource result belongs to its batch")
                    .0;
                let (acknowledge, acknowledgment) = mpsc::sync_channel(0);
                batch
                    .outcomes
                    .send(ReadyOutcome::Resource {
                        index,
                        result,
                        compatibility,
                        fingerprint: fingerprint.cloned(),
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
