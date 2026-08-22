use anyhow::Result;
use serde_json::{Value, json};

use super::super::engine::ScheduledTick;
use super::super::occurrence::OccurrenceOutcome;

#[derive(Default)]
pub(super) struct DispatchStep {
    pub(super) action: Option<Value>,
    pub(super) due_count: u64,
    pub(super) executed_count: u64,
    pub(super) skipped_count: u64,
    pub(super) failed_count: u64,
}

#[derive(Default)]
pub(super) struct DispatchSummary {
    pub(super) due_count: u64,
    pub(super) executed_count: u64,
    pub(super) skipped_count: u64,
    pub(super) failed_count: u64,
    pub(super) needs_attention_count: u64,
}

impl DispatchSummary {
    pub(super) fn include(&mut self, step: &DispatchStep) {
        self.due_count += step.due_count;
        self.executed_count += step.executed_count;
        self.skipped_count += step.skipped_count;
        self.failed_count += step.failed_count;
    }

    pub(super) fn requires_attention(&self) -> bool {
        self.failed_count > 0 || self.needs_attention_count > 0
    }

    pub(super) fn status(&self) -> &'static str {
        if self.failed_count > 0 {
            "failed"
        } else if self.needs_attention_count > 0 {
            "needs_attention"
        } else if self.executed_count > 0 {
            "acted"
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

pub(super) fn begin_execution<T>(
    step: &mut DispatchStep,
    start: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let execution = start()?;
    step.executed_count = 1;
    Ok(execution)
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
    pub(super) fn from_tick(tick: &Result<ScheduledTick>) -> Self {
        match tick {
            Ok(tick) => {
                let completion = tick.completion();
                let status = tick.value().and_then(|value| value["status"].as_str());
                let outcome = if tick.error().is_some() || status == Some("failed") {
                    OccurrenceOutcome::Failed
                } else if status == Some("needs_attention") {
                    OccurrenceOutcome::NeedsAttention
                } else {
                    OccurrenceOutcome::Succeeded
                };
                Self {
                    outcome,
                    worker_receipt_id: completion.worker_receipt_id.clone(),
                    worktree: completion.worktree.clone(),
                    error: combined_error(tick.error(), completion.error.as_deref()),
                }
            }
            Err(error) => Self {
                outcome: OccurrenceOutcome::Failed,
                worker_receipt_id: None,
                worktree: None,
                error: Some(format!("{error:#}")),
            },
        }
    }
}

fn combined_error(primary: Option<&str>, completion: Option<&str>) -> Option<String> {
    match (primary, completion) {
        (Some(primary), Some(completion)) if !primary.contains(completion) => {
            Some(format!("{primary}; workflow completion: {completion}"))
        }
        (Some(primary), _) => Some(primary.to_string()),
        (None, Some(completion)) => Some(completion.to_string()),
        (None, None) => None,
    }
}
