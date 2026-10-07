//! Read-only classification of run-history records for summaries and the
//! dashboard.

use jig_contract::{RunConclusion, TargetRunResult};

use super::{EVENT_QUEUED, EVENT_TARGET_COMPLETED, RunEventRecord};

/// A run-history record as read-only views need it.
pub enum RunHistoryEvent {
    /// A run was queued. Every run has exactly one queued event.
    Queued,
    TargetCompleted(Box<CompletedTargetEvent>),
    Other,
}

/// One finished target read from run history.
pub struct CompletedTargetEvent {
    pub run_id: String,
    pub result: TargetRunResult,
}

impl CompletedTargetEvent {
    /// Whether the target failed, timed out, or was blocked. Cancelled and
    /// skipped targets are not failures.
    pub fn failed(&self) -> bool {
        matches!(
            self.result.conclusion,
            Some(RunConclusion::Failure | RunConclusion::TimedOut | RunConclusion::Blocked)
        )
    }
}

/// Classifies one run-history record. Only `target_completed` records are
/// fully decoded, and fields retired from older records are ignored.
pub fn run_history_event(record: &[u8]) -> serde_json::Result<RunHistoryEvent> {
    #[derive(serde::Deserialize)]
    struct EventKind {
        event: String,
    }
    match serde_json::from_slice::<EventKind>(record)?.event.as_str() {
        EVENT_QUEUED => Ok(RunHistoryEvent::Queued),
        EVENT_TARGET_COMPLETED => {
            let event = serde_json::from_slice::<RunEventRecord>(record)?;
            Ok(event.result.map_or(RunHistoryEvent::Other, |result| {
                RunHistoryEvent::TargetCompleted(Box::new(CompletedTargetEvent {
                    run_id: event.run_id,
                    result,
                }))
            }))
        }
        _ => Ok(RunHistoryEvent::Other),
    }
}
