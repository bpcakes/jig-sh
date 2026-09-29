use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use self::errors::{
    Observation, SNAPSHOT_ERROR_CODES, SNAPSHOT_ERROR_SCOPES, SnapshotError, SnapshotErrorCode,
};
use super::{
    AppliedLimit, BoundedRows, BoundedText, LimitId, RecorderEpochId, RecorderLimits, TimelineLimit,
};

mod errors;

pub const RECORDER_SCHEMA_VERSION: u64 = 3;
pub const UI_COMMAND: &str = "ui";
pub const RECORDER_ROOT_FIELDS: &[&str] = &[
    "ok",
    "command",
    "schema_version",
    "generated_at_ms",
    "epoch_id",
    "repo",
    "harness",
    "failures",
    "target_stats",
    "loops",
    "timeline",
    "timeline_show",
    "timeline_limit",
    "limits",
    "errors",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecorderSnapshot {
    pub ok: bool,
    pub command: String,
    pub schema_version: u64,
    pub generated_at_ms: u64,
    pub epoch_id: RecorderEpochId,
    pub repo: RepositoryObservation,
    pub harness: HarnessObservation,
    pub failures: Vec<Failure>,
    pub target_stats: Vec<TargetStat>,
    pub loops: Option<LoopObservation>,
    pub timeline: Vec<TimelineRow>,
    pub timeline_show: String,
    pub timeline_limit: usize,
    pub limits: RecorderLimits,
    pub errors: Vec<SnapshotError>,
}

impl RecorderSnapshot {
    #[must_use]
    pub fn new(
        epoch_id: RecorderEpochId,
        generated_at_ms: u64,
        timeline_limit: TimelineLimit,
    ) -> Self {
        let timeline_limit = timeline_limit.get();
        Self {
            ok: true,
            command: UI_COMMAND.to_string(),
            schema_version: RECORDER_SCHEMA_VERSION,
            generated_at_ms,
            epoch_id,
            repo: RepositoryObservation::default(),
            harness: HarnessObservation::default(),
            failures: Vec::new(),
            target_stats: Vec::new(),
            loops: None,
            timeline: Vec::new(),
            timeline_show: "all".to_string(),
            timeline_limit,
            limits: RecorderLimits {
                failures: super::AppliedLimit {
                    applied: LimitId::Failures.ceiling(),
                    omitted: Some(0),
                },
                target_stats: super::AppliedLimit {
                    applied: LimitId::TargetStats.ceiling(),
                    omitted: Some(0),
                },
                timeline: super::AppliedLimit {
                    applied: timeline_limit,
                    omitted: Some(0),
                },
            },
            errors: Vec::new(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        validate_root_rows(
            LimitId::Failures,
            self.failures.len(),
            self.limits.failures,
            LimitId::Failures.ceiling(),
        )?;
        validate_root_rows(
            LimitId::TargetStats,
            self.target_stats.len(),
            self.limits.target_stats,
            LimitId::TargetStats.ceiling(),
        )?;
        if self.timeline_limit == 0 || self.timeline_limit > super::MAX_TIMELINE_ROWS {
            return Err(format!(
                "invalid recorder timeline limit {}",
                self.timeline_limit
            ));
        }
        if self.limits.timeline.applied != self.timeline_limit {
            return Err("recorder timeline field and limit metadata differ".to_string());
        }
        validate_root_rows(
            LimitId::Timeline,
            self.timeline.len(),
            self.limits.timeline,
            self.timeline_limit,
        )?;

        for failure in &self.failures {
            validate_text(&failure.output_tail, LimitId::FailureOutputChars)?;
        }
        if let Some(loops) = &self.loops {
            loops.validate()?;
        }
        for row in &self.timeline {
            row.validate()?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(remote = "RecorderSnapshot")]
struct RecorderSnapshotWire {
    ok: bool,
    command: String,
    schema_version: u64,
    generated_at_ms: u64,
    epoch_id: RecorderEpochId,
    repo: RepositoryObservation,
    harness: HarnessObservation,
    failures: Vec<Failure>,
    target_stats: Vec<TargetStat>,
    loops: Option<LoopObservation>,
    timeline: Vec<TimelineRow>,
    timeline_show: String,
    timeline_limit: usize,
    limits: RecorderLimits,
    errors: Vec<SnapshotError>,
}

impl Serialize for RecorderSnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        RecorderSnapshotWire::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RecorderSnapshot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let snapshot = RecorderSnapshotWire::deserialize(deserializer)?;
        snapshot.validate().map_err(serde::de::Error::custom)?;
        Ok(snapshot)
    }
}

fn validate_root_rows(
    id: LimitId,
    retained: usize,
    limit: AppliedLimit,
    expected_applied: usize,
) -> Result<(), String> {
    let id = id.as_str();
    if limit.applied != expected_applied {
        return Err(format!(
            "{id} applied limit {} differs from required {expected_applied}",
            limit.applied
        ));
    }
    if retained > limit.applied {
        return Err(format!(
            "{id} retained {retained} rows exceeds applied limit {}",
            limit.applied
        ));
    }
    if limit
        .omitted
        .is_some_and(|omitted| omitted > 0 && retained < limit.applied)
    {
        return Err(format!(
            "{id} reports omitted rows before filling its applied limit"
        ));
    }
    Ok(())
}

fn validate_rows<T>(rows: &BoundedRows<T>, id: LimitId) -> Result<(), String> {
    rows.validate_for_limit(id)
        .map_err(|error| error.to_string())
}

fn validate_text(text: &BoundedText, id: LimitId) -> Result<(), String> {
    text.validate_for_limit(id)
        .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoryObservation {
    pub name: String,
    pub default_branch: String,
    pub source_commit: Option<String>,
    pub source_path: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HarnessObservation {
    pub jig_version: Option<String>,
    pub runtime_version: String,
    pub contract_version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Remediation {
    pub argv: Vec<String>,
    pub display: String,
}

/// A target result from run history that did not succeed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Failure {
    pub run_id: String,
    pub target: String,
    pub conclusion: String,
    pub exit_code: Option<i64>,
    pub ended_at_ms: Option<u64>,
    pub output_tail: BoundedText,
}

/// Run-history aggregate for one target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TargetStat {
    pub target: String,
    pub runs: u64,
    pub failures: u64,
    pub last_conclusion: Option<String>,
    pub last_ended_at_ms: u64,
    pub avg_duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopObservation {
    pub ok: bool,
    pub command: String,
    pub workflows: BoundedRows<LoopWorkflow>,
    pub leases: BoundedRows<LoopLease>,
    pub attempts: BoundedRows<LoopAttempt>,
    pub scheduled_occurrences: BoundedRows<ScheduledOccurrence>,
    pub waiting_attempts: BoundedRows<LoopAttempt>,
    pub state_error_count: u64,
    pub state_errors: Vec<LoopStateError>,
    pub needs_attention: LoopAttention,
}

impl LoopObservation {
    fn validate(&self) -> Result<(), String> {
        validate_rows(&self.workflows, LimitId::LoopWorkflows)?;
        validate_rows(&self.leases, LimitId::LoopLeases)?;
        validate_rows(&self.attempts, LimitId::LoopAttempts)?;
        validate_rows(
            &self.scheduled_occurrences,
            LimitId::LoopScheduledOccurrences,
        )?;
        validate_rows(&self.waiting_attempts, LimitId::LoopWaitingAttempts)?;
        validate_rows(
            &self.needs_attention.exhausted_attempts,
            LimitId::LoopExhaustedAttempts,
        )?;
        validate_rows(
            &self.needs_attention.scheduled_occurrences,
            LimitId::LoopScheduledOccurrences,
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopWorkflow {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    pub configured: bool,
    pub lease_ttl_seconds: u64,
    pub max_attempts: u32,
    pub backoff_seconds: u64,
    pub codex_home_configured: Option<String>,
    pub schedule: Option<LoopSchedule>,
    pub schedule_state: Option<LoopScheduleState>,
    pub schedule_state_error: Option<String>,
    pub codex_task: Option<LoopCodexTask>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopSchedule {
    pub cron: String,
    pub timezone: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopScheduleState {
    pub due_at_ms: Option<u64>,
    pub next_at_ms: u64,
    pub last_scheduled_at_ms: Option<u64>,
    pub last_status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopCodexTask {
    pub prompt_file: String,
    pub model: Option<String>,
    pub sandbox: String,
    pub checkout: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopLease {
    pub key: String,
    pub owner: String,
    pub acquired_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopAttempt {
    pub key: String,
    pub workflow_id: String,
    pub item_key: String,
    pub item_version: Option<String>,
    pub observed_item_version: Option<String>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_attempt_ms: u64,
    pub next_eligible_ms: u64,
    pub exhausted: bool,
    pub last_status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopAttention {
    pub exhausted_attempts: BoundedRows<ExhaustedAttempt>,
    pub scheduled_occurrences: BoundedRows<ScheduledOccurrence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExhaustedAttempt {
    pub key: String,
    pub workflow_id: String,
    pub item_key: String,
    pub item_version: Option<String>,
    pub observed_item_version: Option<String>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_attempt_ms: u64,
    pub next_eligible_ms: u64,
    pub exhausted: bool,
    pub last_status: String,
    pub remediation: Option<Remediation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScheduledOccurrence {
    pub occurrence_id: String,
    pub workflow_id: String,
    pub scheduled_at_ms: u64,
    pub owner: String,
    pub claim_expires_at_ms: u64,
    pub started_at_ms: u64,
    pub uses_shared_checkout: Option<bool>,
    pub finished_at_ms: Option<u64>,
    pub acknowledged_at_ms: Option<u64>,
    pub status: String,
    pub worker_receipt_id: Option<String>,
    pub worktree: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoopStateError {
    pub kind: String,
    pub workflow_id: Option<String>,
    pub error: String,
}

/// One target result from run history.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineRow {
    pub stable_identity: String,
    pub timestamp_ms: Option<u64>,
    pub run_id: String,
    pub target: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub exit_code: Option<i64>,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub finding_count: Option<u64>,
    /// Present only for a target that did not succeed.
    pub output_tail: Option<BoundedText>,
}

impl TimelineRow {
    #[must_use]
    pub fn stable_identity(&self) -> &str {
        &self.stable_identity
    }

    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.conclusion.as_deref() == Some("success")
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(output) = &self.output_tail {
            validate_text(output, LimitId::FailureOutputChars)?;
        }
        Ok(())
    }
}
