use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::super::occurrence::{
    OccurrenceAttentionScope, OccurrenceClaim, OccurrenceFinalization, OccurrenceFinish,
    OccurrenceGuard, OccurrenceOutcome, OccurrenceStore, OccurrenceWorktreeReservation,
    ScheduleOccurrence,
};
use super::super::workflow::{
    CodexTaskCheckout, ResolvedWorkflow, UnexecutedReason, WorkflowCompletion, WorkflowExecution,
    WorkflowOutcome,
};

pub(super) struct ManualOccurrenceGuard {
    guard: OccurrenceGuard,
}

pub(super) enum ManualOccurrenceStart {
    Acquired(ManualOccurrenceGuard),
    Blocked {
        occurrence: ScheduleOccurrence,
        error: String,
    },
    Waiting {
        occurrence: ScheduleOccurrence,
        error: String,
    },
}

impl ManualOccurrenceStart {
    pub(super) fn prepare_tick(
        self,
        actions: &mut Vec<Value>,
        completion: &mut WorkflowCompletion,
    ) -> (Option<ManualOccurrenceGuard>, Option<ScheduleOccurrence>) {
        match self {
            Self::Acquired(guard) => (Some(guard), None),
            Self::Blocked { occurrence, error } => {
                actions.push(json!({
                    "kind": "manual_occurrence",
                    "status": "needs_attention",
                    "reason": "manual_occurrence_blocked",
                    "occurrence": &occurrence,
                    "error": &error,
                }));
                *completion = WorkflowCompletion {
                    outcome: WorkflowOutcome::NeedsAttention,
                    execution: WorkflowExecution::Unexecuted(UnexecutedReason::BlockedByAttention),
                    error: Some(error),
                    ..WorkflowCompletion::default()
                };
                (None, Some(occurrence))
            }
            Self::Waiting { occurrence, error } => {
                actions.push(json!({
                    "kind": "manual_occurrence",
                    "status": "waiting",
                    "reason": "manual_occurrence_running",
                    "occurrence": &occurrence,
                    "error": &error,
                }));
                *completion = WorkflowCompletion {
                    execution: WorkflowExecution::Unexecuted(
                        UnexecutedReason::BlockedByActiveOccurrence,
                    ),
                    error: Some(error),
                    ..WorkflowCompletion::default()
                };
                (None, Some(occurrence))
            }
        }
    }
}

impl ManualOccurrenceGuard {
    pub(super) fn worktree_reservation(&self) -> OccurrenceWorktreeReservation {
        self.guard.worktree_reservation()
    }

    pub(super) fn start(
        workflow: &ResolvedWorkflow,
        item_key: &str,
        ctx: &crate::context::RepoContext,
    ) -> Result<ManualOccurrenceStart> {
        let block_retained_worktree = workflow.blocks_on_retained_worktree();
        let attention_scope = if workflow
            .codex_task
            .as_ref()
            .is_some_and(|task| task.checkout == CodexTaskCheckout::Repo)
        {
            OccurrenceAttentionScope::SharedRepository
        } else {
            OccurrenceAttentionScope::Workflow
        };
        let mut store = OccurrenceStore::new(ctx);
        let claim = store.claim_manual(
            &workflow.id,
            item_key,
            workflow.lease_ttl_seconds,
            attention_scope,
            block_retained_worktree,
        )?;
        let occurrence = match claim {
            OccurrenceClaim::Acquired(occurrence) => occurrence,
            OccurrenceClaim::AlreadyRecorded(occurrence) => bail!(
                "Manual loop occurrence '{}' is already {}",
                occurrence.occurrence_id,
                occurrence.status
            ),
            OccurrenceClaim::BlockedByAttention(occurrence) => {
                return Ok(ManualOccurrenceStart::Blocked {
                    error: format!(
                        "Loop occurrence '{}' requires acknowledgement before workflow '{}' can run manually",
                        occurrence.occurrence_id, workflow.id
                    ),
                    occurrence,
                });
            }
            OccurrenceClaim::BlockedByRunning(occurrence) => {
                return Ok(ManualOccurrenceStart::Waiting {
                    error: format!(
                        "Loop occurrence '{}' is still running and blocks workflow '{}'",
                        occurrence.occurrence_id, workflow.id
                    ),
                    occurrence,
                });
            }
            OccurrenceClaim::BlockedByRetainedWorktree(occurrence) => {
                return Ok(ManualOccurrenceStart::Blocked {
                    error: format!(
                        "Retained worktree '{}' must be removed before workflow '{}' can run manually",
                        occurrence.worktree.as_deref().unwrap_or("<unknown>"),
                        workflow.id
                    ),
                    occurrence,
                });
            }
        };
        match OccurrenceGuard::start(store.clone(), &occurrence, workflow.lease_ttl_seconds) {
            Ok(guard) => Ok(ManualOccurrenceStart::Acquired(Self { guard })),
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

    pub(super) fn completion_requires_retention(completion: &WorkflowCompletion) -> bool {
        completion.outcome == WorkflowOutcome::NeedsAttention || completion.worktree.is_some()
    }

    pub(super) fn stage_tick(
        &mut self,
        completion: &mut WorkflowCompletion,
    ) -> Result<Option<ScheduleOccurrence>> {
        let occurrence = self.guard.stage_manual(occurrence_finish(completion))?;
        if occurrence.status.as_str() == "running" {
            return Ok(None);
        }
        apply_occurrence_attention(completion, &occurrence);
        Ok(Some(occurrence))
    }

    pub(super) fn finish(self, completion: &WorkflowCompletion) -> Result<OccurrenceFinalization> {
        self.guard.finish_manual(
            occurrence_finish(completion),
            Self::completion_requires_retention(completion),
        )
    }

    pub(super) fn complete_tick(
        self,
        completion: &mut WorkflowCompletion,
    ) -> (Option<ScheduleOccurrence>, Option<String>) {
        match self.finish(completion) {
            Ok(finalization) => {
                let error = finalization
                    .renewal_error
                    .map(|error| format!("Manual occurrence renewal failed: {error}"));
                let occurrence = finalization.occurrence;
                if occurrence.status.as_str() == "running" {
                    return (None, error);
                }
                apply_occurrence_attention(completion, &occurrence);
                (Some(occurrence), error)
            }
            Err(error) => (
                None,
                Some(format!(
                    "Failed to finalize manual loop occurrence: {error:#}"
                )),
            ),
        }
    }
}

fn occurrence_finish(completion: &WorkflowCompletion) -> OccurrenceFinish<'_> {
    OccurrenceFinish {
        outcome: match completion.outcome {
            WorkflowOutcome::Succeeded => OccurrenceOutcome::Succeeded,
            WorkflowOutcome::Failed => OccurrenceOutcome::Failed,
            WorkflowOutcome::NeedsAttention => OccurrenceOutcome::NeedsAttention,
        },
        worker_receipt_id: completion.worker_receipt_id.as_deref(),
        worktree: completion.worktree.as_deref(),
        error: completion.error.as_deref(),
    }
}

fn apply_occurrence_attention(
    completion: &mut WorkflowCompletion,
    occurrence: &ScheduleOccurrence,
) {
    if occurrence.requires_attention_at(crate::state::now_ms()) {
        completion.outcome = WorkflowOutcome::NeedsAttention;
        if completion.error.is_none() {
            completion.error = occurrence.error.clone();
        }
    }
}
