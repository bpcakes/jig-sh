use serde_json::{Value, json};

use super::super::engine::ScheduledTick;
#[cfg(test)]
use super::super::engine::WorkflowLeaseDisposition;
use super::super::occurrence::OccurrenceOutcome;
#[cfg(test)]
use super::super::workflow::WorkflowExecution;
use super::super::workflow::{RepositoryRevisionState, WorkflowOutcome};

#[derive(Default)]
pub(super) struct DispatchStep {
    pub(super) action: Option<Value>,
    pub(super) state_errors: Vec<Value>,
    pub(super) due_count: u64,
    pub(super) executed_count: u64,
    pub(super) deferred_count: u64,
    pub(super) skipped_count: u64,
    pub(super) failed_count: u64,
    pub(super) repository_revision: RepositoryRevisionState,
}

#[derive(Default)]
pub(super) struct DispatchSummary {
    pub(super) due_count: u64,
    pub(super) executed_count: u64,
    pub(super) deferred_count: u64,
    pub(super) skipped_count: u64,
    pub(super) failed_count: u64,
    pub(super) needs_attention_count: u64,
    pub(super) exhausted_attempt_count: u64,
    pub(super) state_error_count: u64,
    pub(super) state_errors: Vec<Value>,
    pub(super) repository_revision_changed: bool,
}

impl DispatchSummary {
    pub(super) fn include(&mut self, step: &DispatchStep) {
        self.due_count += step.due_count;
        self.executed_count += step.executed_count;
        self.deferred_count += step.deferred_count;
        self.skipped_count += step.skipped_count;
        self.failed_count += step.failed_count;
        self.repository_revision_changed |= step.repository_revision.changed();
        self.include_state_errors(step.state_errors.iter().cloned());
    }

    pub(super) fn include_state_errors(&mut self, errors: impl IntoIterator<Item = Value>) {
        for error in errors {
            if !self.state_errors.contains(&error) {
                self.state_errors.push(error);
            }
        }
        self.state_error_count = u64::try_from(self.state_errors.len()).unwrap_or(u64::MAX);
    }

    pub(super) fn state_error_text(&self) -> String {
        self.state_errors
            .iter()
            .filter_map(|error| error["error"].as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub(super) fn status(&self) -> &'static str {
        if self.failed_count > 0 || self.state_error_count > 0 {
            "failed"
        } else if self.needs_attention_count > 0 || self.exhausted_attempt_count > 0 {
            "needs_attention"
        } else if self.executed_count > 0 {
            "acted"
        } else if self.deferred_count > 0 {
            "deferred"
        } else {
            "idle"
        }
    }
}

impl DispatchStep {
    pub(super) fn action(action: Value) -> Self {
        Self {
            action: Some(action),
            ..Self::default()
        }
    }

    pub(super) fn failure(workflow_id: &str, error: impl std::fmt::Display) -> Self {
        Self {
            action: Some(json!({
                "workflow_id": workflow_id,
                "status": "failed",
                "error": error.to_string(),
            })),
            failed_count: 1,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RunTickDisposition {
    Continue,
    Failed,
    Stop(&'static str),
}

#[derive(Default)]
pub(super) struct RunSummary {
    failed: bool,
    stop_status: Option<&'static str>,
}

impl RunSummary {
    pub(super) fn observe(&mut self, disposition: RunTickDisposition) -> bool {
        match disposition {
            RunTickDisposition::Continue => false,
            RunTickDisposition::Failed => {
                self.failed = true;
                false
            }
            RunTickDisposition::Stop(status) => {
                self.stop_status = Some(status);
                true
            }
        }
    }

    pub(super) fn status(&self) -> &'static str {
        if self.failed {
            "failed"
        } else {
            self.stop_status.unwrap_or("max_ticks_reached")
        }
    }
}

impl RunTickDisposition {
    pub(super) fn from_tick(tick: &Value) -> Self {
        match tick["status"].as_str() {
            Some("failed") => Self::Failed,
            Some("waiting") => Self::Stop("waiting"),
            Some("disabled") => Self::Stop("disabled"),
            Some("needs_attention") => Self::Stop("needs_attention"),
            _ if tick["idle"].as_bool() == Some(true) => Self::Stop("idle"),
            _ => Self::Continue,
        }
    }
}

pub(super) struct TerminalDetails {
    pub(super) outcome: OccurrenceOutcome,
    pub(super) worker_receipt_id: Option<String>,
    pub(super) worktree: Option<String>,
    pub(super) error: Option<String>,
}

impl TerminalDetails {
    pub(super) fn from_tick(tick: &ScheduledTick) -> Self {
        let completion = tick.completion();
        let post_work_error = tick.post_work_error();
        let outcome = match (completion.outcome, post_work_error) {
            (WorkflowOutcome::Succeeded, Some(_)) => OccurrenceOutcome::NeedsAttention,
            (WorkflowOutcome::NeedsAttention, _) => OccurrenceOutcome::NeedsAttention,
            (WorkflowOutcome::Succeeded, None) => OccurrenceOutcome::Succeeded,
            (WorkflowOutcome::Failed, _) => OccurrenceOutcome::Failed,
        };
        Self {
            outcome,
            worker_receipt_id: completion.worker_receipt_id.clone(),
            worktree: completion.worktree.clone(),
            error: completion
                .error
                .clone()
                .or_else(|| post_work_error.map(str::to_string)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::workflow::WorkflowCompletion;
    use super::*;

    #[test]
    fn dispatch_summary_preserves_distinct_state_error_events_of_the_same_kind() {
        let first = json!({
            "kind": "tick",
            "error": "first failure",
            "workflow_id": "first-workflow",
        });
        let second = json!({
            "kind": "tick",
            "error": "second failure",
            "workflow_id": "second-workflow",
        });
        let mut summary = DispatchSummary::default();

        summary.include_state_errors([first.clone(), second.clone(), first]);

        assert_eq!(summary.state_error_count, 2);
        assert_eq!(
            summary.state_errors,
            vec![
                json!({
                    "kind": "tick",
                    "error": "first failure",
                    "workflow_id": "first-workflow",
                }),
                second,
            ]
        );
    }

    #[test]
    fn scheduled_tick_preserves_needs_attention_as_an_occurrence_outcome() {
        let tick = ScheduledTick::Reported {
            value: json!({"status": "acted"}),
            completion: WorkflowCompletion {
                outcome: WorkflowOutcome::NeedsAttention,
                ..WorkflowCompletion::default()
            },
            lease_disposition: WorkflowLeaseDisposition::Acquired,
            state_errors: Vec::new(),
        };

        let details = TerminalDetails::from_tick(&tick);

        assert_eq!(details.outcome, OccurrenceOutcome::NeedsAttention);
    }

    #[test]
    fn scheduled_tick_error_keeps_worker_completion_evidence() {
        let tick = ScheduledTick::Errored {
            value: Some(json!({"status": "failed"})),
            completion: WorkflowCompletion {
                outcome: WorkflowOutcome::Failed,
                execution: WorkflowExecution::Executed,
                repository_revision: RepositoryRevisionState::NotApplicable,
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: Some("/tmp/retained-worktree".into()),
                error: Some("worker failed".into()),
            },
            lease_disposition: WorkflowLeaseDisposition::Acquired,
            state_errors: Vec::new(),
            error: "tick receipt failed".into(),
            post_work_error: Some("tick receipt failed".into()),
        };

        let details = TerminalDetails::from_tick(&tick);

        assert_eq!(details.outcome, OccurrenceOutcome::Failed);
        assert_eq!(details.worker_receipt_id.as_deref(), Some("receipt-worker"));
        assert_eq!(details.worktree.as_deref(), Some("/tmp/retained-worktree"));
        assert_eq!(details.error.as_deref(), Some("worker failed"));
    }

    #[test]
    fn scheduled_tick_error_cannot_downgrade_ambiguous_worker_completion() {
        let tick = ScheduledTick::Errored {
            value: Some(json!({"status": "failed"})),
            completion: WorkflowCompletion {
                outcome: WorkflowOutcome::NeedsAttention,
                error: Some("worker push may have completed".into()),
                ..WorkflowCompletion::default()
            },
            lease_disposition: WorkflowLeaseDisposition::Acquired,
            state_errors: Vec::new(),
            error: "tick receipt failed".into(),
            post_work_error: Some("tick receipt failed".into()),
        };

        let details = TerminalDetails::from_tick(&tick);

        assert_eq!(details.outcome, OccurrenceOutcome::NeedsAttention);
        assert_eq!(
            details.error.as_deref(),
            Some("worker push may have completed")
        );
    }

    #[test]
    fn post_work_tick_error_requires_attention_after_successful_worker_completion() {
        let tick = ScheduledTick::Errored {
            value: Some(json!({"status": "failed"})),
            completion: WorkflowCompletion {
                outcome: WorkflowOutcome::Succeeded,
                execution: WorkflowExecution::Executed,
                repository_revision: RepositoryRevisionState::NotApplicable,
                worker_receipt_id: Some("receipt-worker".into()),
                worktree: Some("/tmp/retained-worktree".into()),
                error: None,
            },
            lease_disposition: WorkflowLeaseDisposition::Acquired,
            state_errors: Vec::new(),
            error: "attempt state could not be observed".into(),
            post_work_error: Some("attempt state could not be observed".into()),
        };

        let details = TerminalDetails::from_tick(&tick);

        assert_eq!(details.outcome, OccurrenceOutcome::NeedsAttention);
        assert_eq!(details.worker_receipt_id.as_deref(), Some("receipt-worker"));
        assert_eq!(details.worktree.as_deref(), Some("/tmp/retained-worktree"));
        assert_eq!(
            details.error.as_deref(),
            Some("attempt state could not be observed")
        );
    }
}
