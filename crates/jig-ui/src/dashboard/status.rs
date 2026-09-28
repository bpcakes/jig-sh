use serde::{Deserialize, Serialize};

use super::{
    LoopCodexTask, LoopLease, LoopSchedule, LoopScheduleState, LoopStateError, RecorderEpochId,
};

pub const STATUS_SCHEMA_VERSION: u64 = 3;
pub const STATUS_COMMAND: &str = "status";
pub const STATUS_ROOT_FIELDS: &[&str] = &[
    "ok",
    "command",
    "schema_version",
    "observed_at_ms",
    "outcome",
    "repository",
    "loops",
    "errors",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusOutcome {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StatusSnapshot {
    pub ok: bool,
    pub command: String,
    pub schema_version: u64,
    pub observed_at_ms: u64,
    pub outcome: StatusOutcome,
    pub repository: StatusRepositoryObservation,
    pub loops: Option<StatusLoopObservation>,
    pub errors: Vec<StatusCollectionError>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StatusLocalSnapshot {
    pub epoch_id: RecorderEpochId,
    pub observed_at_ms: u64,
    pub repository: StatusRepositoryObservation,
    pub loops: Option<StatusLoopObservation>,
    pub errors: Vec<StatusCollectionError>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusRepositoryObservation {
    pub name: String,
    pub default_branch: String,
    pub head_revision: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub dirty: Option<bool>,
    pub upstream: Option<UpstreamObservation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpstreamObservation {
    pub reference: String,
    pub ahead: u64,
    pub behind: u64,
    pub state: String,
    pub basis: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusLoopObservation {
    pub ok: bool,
    pub command: String,
    pub workflows: Vec<StatusLoopWorkflow>,
    pub leases: Vec<LoopLease>,
    pub attempts: Vec<StatusLoopAttempt>,
    pub scheduled_occurrences: Vec<StatusScheduledOccurrence>,
    pub waiting_attempts: Vec<StatusLoopAttempt>,
    pub state_error_count: u64,
    pub state_errors: Vec<LoopStateError>,
    pub needs_attention: StatusLoopAttention,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusLoopAttention {
    pub exhausted_attempts: Vec<StatusLoopAttempt>,
    pub scheduled_occurrences: Vec<StatusScheduledOccurrence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusLoopWorkflow {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub configured: bool,
    pub lease_ttl_seconds: u64,
    pub max_attempts: u32,
    pub backoff_seconds: u64,
    pub codex_home_configured: Option<String>,
    pub schedule: Option<LoopSchedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_state: Option<LoopScheduleState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_state_error: Option<String>,
    pub codex_task: Option<LoopCodexTask>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusLoopAttempt {
    pub key: String,
    pub workflow_id: String,
    pub item_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_item_version: Option<String>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_attempt_ms: u64,
    pub next_eligible_ms: u64,
    pub exhausted: bool,
    pub last_status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusScheduledOccurrence {
    pub occurrence_id: String,
    pub workflow_id: String,
    pub scheduled_at_ms: u64,
    pub owner: String,
    pub claim_expires_at_ms: u64,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses_shared_checkout: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at_ms: Option<u64>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Compatibility name for an attempt selected into the exhausted collection.
/// Selection belongs to the status producer; the observation has the same
/// fields and wire format as every other status attempt.
pub type StatusExhaustedAttempt = StatusLoopAttempt;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusCollectionError {
    pub scope: String,
    pub code: String,
    pub message: String,
}
