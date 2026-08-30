use anyhow::{Result, bail};

use super::super::occurrence::{
    OccurrenceClaim, OccurrenceFinalization, OccurrenceFinish, OccurrenceGuard, OccurrenceOutcome,
    OccurrenceStore,
};
use super::super::workflow::{
    CodexTaskCheckout, ResolvedWorkflow, WorkflowCompletion, WorkflowOutcome,
};

pub(super) struct ManualOccurrenceGuard {
    guard: OccurrenceGuard,
}

impl ManualOccurrenceGuard {
    pub(super) fn start(
        workflow: &ResolvedWorkflow,
        item_key: &str,
        ctx: &crate::context::RepoContext,
    ) -> Result<Self> {
        let block_retained_worktree = workflow
            .codex_task
            .as_ref()
            .is_some_and(|task| task.checkout == CodexTaskCheckout::Worktree);
        let mut store = OccurrenceStore::new(ctx);
        let claim = store.claim_manual(
            &workflow.id,
            item_key,
            workflow.lease_ttl_seconds,
            block_retained_worktree,
        )?;
        let occurrence = match claim {
            OccurrenceClaim::Acquired(occurrence) => occurrence,
            OccurrenceClaim::AlreadyRecorded(occurrence) => bail!(
                "Manual loop occurrence '{}' is already {}",
                occurrence.occurrence_id,
                occurrence.status
            ),
            OccurrenceClaim::BlockedByAttention(occurrence) => bail!(
                "Loop occurrence '{}' requires acknowledgement before workflow '{}' can run manually",
                occurrence.occurrence_id,
                workflow.id
            ),
            OccurrenceClaim::BlockedByRetainedWorktree(occurrence) => bail!(
                "Retained worktree '{}' must be removed before workflow '{}' can run manually",
                occurrence.worktree.as_deref().unwrap_or("<unknown>"),
                workflow.id
            ),
        };
        match OccurrenceGuard::start(store.clone(), &occurrence, workflow.lease_ttl_seconds) {
            Ok(guard) => Ok(Self { guard }),
            Err(error) => {
                let cleanup = store
                    .abandon_unexecuted(&occurrence.occurrence_id, &occurrence.owner)
                    .err();
                match cleanup {
                    Some(cleanup) => Err(error.context(format!(
                        "Failed to abandon manual occurrence after renewal startup failed: {cleanup:#}"
                    ))),
                    None => Err(error),
                }
            }
        }
    }

    pub(super) fn renewal_failed(&self) -> bool {
        self.guard.renewal_failed()
    }

    pub(super) fn finish(self, completion: &WorkflowCompletion) -> Result<OccurrenceFinalization> {
        let retain =
            completion.outcome == WorkflowOutcome::NeedsAttention || completion.worktree.is_some();
        self.guard.finish_manual(
            OccurrenceFinish {
                outcome: match completion.outcome {
                    WorkflowOutcome::Succeeded => OccurrenceOutcome::Succeeded,
                    WorkflowOutcome::Failed => OccurrenceOutcome::Failed,
                    WorkflowOutcome::NeedsAttention => OccurrenceOutcome::NeedsAttention,
                },
                worker_receipt_id: completion.worker_receipt_id.as_deref(),
                worktree: completion.worktree.as_deref(),
                error: completion.error.as_deref(),
            },
            retain,
        )
    }
}
