use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    AppliedLimit, BoundedRows, BoundedText, CollectionDomain, LimitId, PlanLimits, RecorderEpochId,
    RecorderLimits, TimelineLimit,
};

pub const RECORDER_SCHEMA_VERSION: u64 = 1;
pub const UI_COMMAND: &str = "ui";
pub const RECORDER_ROOT_FIELDS: &[&str] = &[
    "ok",
    "command",
    "schema_version",
    "snapshot_kind",
    "generated_at_ms",
    "epoch_id",
    "repo",
    "harness",
    "current_session_id",
    "counts",
    "open_plans",
    "history",
    "failures",
    "tool_stats",
    "loops",
    "timeline",
    "timeline_show",
    "timeline_limit",
    "limits",
    "errors",
];
pub const PLAN_ROOT_FIELDS: &[&str] = &[
    "ok",
    "command",
    "schema_version",
    "snapshot_kind",
    "generated_at_ms",
    "basis_epoch",
    "detail_observed_at_ms",
    "gates_observed_at_ms",
    "decisions_observed_at_ms",
    "plan",
    "body",
    "gates",
    "decisions",
    "receipts",
    "limits",
    "errors",
];
pub const SNAPSHOT_ERROR_SCOPES: &[&str] = &[
    "repository",
    "state.sessions",
    "state.plans",
    "state.decisions",
    "state.receipts",
    "loops",
    "gates",
    "body",
];
pub const SNAPSHOT_ERROR_CODES: &[&str] = &[
    "git_observation_failed",
    "stream_open_failed",
    "stream_read_failed",
    "record_too_large",
    "record_decode_failed",
    "loop_observation_failed",
    "gate_observation_failed",
    "body_not_found",
    "body_unsafe_path",
    "body_unsafe_type",
    "body_read_failed",
    "body_invalid_utf8",
    "unsupported_platform",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotError {
    scope: String,
    code: String,
    subject_id: Option<String>,
    message: String,
}

impl SnapshotError {
    #[must_use]
    pub fn new(
        domain: CollectionDomain,
        code: SnapshotErrorCode,
        subject_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            scope: domain.as_str().to_string(),
            code: code.as_str().to_string(),
            subject_id,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl<'de> Deserialize<'de> for SnapshotError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            scope: String,
            code: String,
            subject_id: Option<String>,
            message: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        if !SNAPSHOT_ERROR_SCOPES.contains(&wire.scope.as_str()) {
            return Err(serde::de::Error::custom("unknown snapshot error scope"));
        }
        if !SNAPSHOT_ERROR_CODES.contains(&wire.code.as_str()) {
            return Err(serde::de::Error::custom("unknown snapshot error code"));
        }
        Ok(Self {
            scope: wire.scope,
            code: wire.code,
            subject_id: wire.subject_id,
            message: wire.message,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotErrorCode {
    GitObservationFailed,
    StreamOpenFailed,
    StreamReadFailed,
    RecordTooLarge,
    RecordDecodeFailed,
    LoopObservationFailed,
    GateObservationFailed,
    BodyNotFound,
    BodyUnsafePath,
    BodyUnsafeType,
    BodyReadFailed,
    BodyInvalidUtf8,
    UnsupportedPlatform,
}

impl SnapshotErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GitObservationFailed => "git_observation_failed",
            Self::StreamOpenFailed => "stream_open_failed",
            Self::StreamReadFailed => "stream_read_failed",
            Self::RecordTooLarge => "record_too_large",
            Self::RecordDecodeFailed => "record_decode_failed",
            Self::LoopObservationFailed => "loop_observation_failed",
            Self::GateObservationFailed => "gate_observation_failed",
            Self::BodyNotFound => "body_not_found",
            Self::BodyUnsafePath => "body_unsafe_path",
            Self::BodyUnsafeType => "body_unsafe_type",
            Self::BodyReadFailed => "body_read_failed",
            Self::BodyInvalidUtf8 => "body_invalid_utf8",
            Self::UnsupportedPlatform => "unsupported_platform",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Observation<T> {
    pub data: Option<T>,
    pub error: Option<SnapshotError>,
}

impl<T> Observation<T> {
    #[must_use]
    pub const fn available(data: T) -> Self {
        Self {
            data: Some(data),
            error: None,
        }
    }

    #[must_use]
    pub const fn unavailable(error: SnapshotError) -> Self {
        Self {
            data: None,
            error: Some(error),
        }
    }

    #[must_use]
    pub const fn partial(data: T, error: SnapshotError) -> Self {
        Self {
            data: Some(data),
            error: Some(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    Recorder,
    Plan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecorderSnapshot {
    pub ok: bool,
    pub command: String,
    pub schema_version: u64,
    pub snapshot_kind: SnapshotKind,
    pub generated_at_ms: u64,
    pub epoch_id: RecorderEpochId,
    pub repo: RepositoryObservation,
    pub harness: HarnessObservation,
    pub current_session_id: Option<String>,
    pub counts: RecorderCounts,
    pub open_plans: Vec<OpenPlan>,
    pub history: Vec<PlanSummary>,
    pub failures: Vec<Failure>,
    pub tool_stats: Vec<ToolStat>,
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
            snapshot_kind: SnapshotKind::Recorder,
            generated_at_ms,
            epoch_id,
            repo: RepositoryObservation::default(),
            harness: HarnessObservation::default(),
            current_session_id: None,
            counts: RecorderCounts::default(),
            open_plans: Vec::new(),
            history: Vec::new(),
            failures: Vec::new(),
            tool_stats: Vec::new(),
            loops: None,
            timeline: Vec::new(),
            timeline_show: "all".to_string(),
            timeline_limit,
            limits: RecorderLimits {
                open_plans: super::AppliedLimit {
                    applied: LimitId::OpenPlans.ceiling(),
                    omitted: Some(0),
                },
                history: super::AppliedLimit {
                    applied: LimitId::History.ceiling(),
                    omitted: Some(0),
                },
                failures: super::AppliedLimit {
                    applied: LimitId::Failures.ceiling(),
                    omitted: Some(0),
                },
                tool_stats: super::AppliedLimit {
                    applied: LimitId::ToolStats.ceiling(),
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
            LimitId::OpenPlans,
            self.open_plans.len(),
            self.limits.open_plans,
            LimitId::OpenPlans.ceiling(),
        )?;
        validate_root_rows(
            LimitId::History,
            self.history.len(),
            self.limits.history,
            LimitId::History.ceiling(),
        )?;
        validate_root_rows(
            LimitId::Failures,
            self.failures.len(),
            self.limits.failures,
            LimitId::Failures.ceiling(),
        )?;
        validate_root_rows(
            LimitId::ToolStats,
            self.tool_stats.len(),
            self.limits.tool_stats,
            LimitId::ToolStats.ceiling(),
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

        for plan in &self.open_plans {
            if let Some(gates) = &plan.gates {
                gates.validate()?;
            }
        }
        for failure in &self.failures {
            validate_text(&failure.stderr_preview, LimitId::FailureStderrChars)?;
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
    snapshot_kind: SnapshotKind,
    generated_at_ms: u64,
    epoch_id: RecorderEpochId,
    repo: RepositoryObservation,
    harness: HarnessObservation,
    current_session_id: Option<String>,
    counts: RecorderCounts,
    open_plans: Vec<OpenPlan>,
    history: Vec<PlanSummary>,
    failures: Vec<Failure>,
    tool_stats: Vec<ToolStat>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanSnapshot {
    pub ok: bool,
    pub command: String,
    pub schema_version: u64,
    pub snapshot_kind: SnapshotKind,
    pub generated_at_ms: u64,
    pub basis_epoch: RecorderEpochId,
    pub detail_observed_at_ms: u64,
    pub gates_observed_at_ms: u64,
    pub decisions_observed_at_ms: u64,
    pub plan: PlanSummary,
    pub body: Option<BoundedText>,
    pub gates: Option<GatesObservation>,
    pub decisions: Vec<Decision>,
    pub receipts: Vec<Receipt>,
    pub limits: PlanLimits,
    pub errors: Vec<SnapshotError>,
}

impl PlanSnapshot {
    fn validate(&self) -> Result<(), String> {
        validate_root_rows(
            LimitId::PlanDecisions,
            self.decisions.len(),
            self.limits.plan_decisions,
            LimitId::PlanDecisions.ceiling(),
        )?;
        validate_root_rows(
            LimitId::PlanReceipts,
            self.receipts.len(),
            self.limits.plan_receipts,
            LimitId::PlanReceipts.ceiling(),
        )?;
        if let Some(body) = &self.body {
            validate_text(body, LimitId::PlanBodyChars)?;
        }
        if let Some(gates) = &self.gates {
            gates.validate()?;
        }
        for decision in &self.decisions {
            validate_text(&decision.rationale, LimitId::TimelineDecisionRationaleChars)?;
        }
        for receipt in &self.receipts {
            validate_rows(&receipt.changed_paths, LimitId::ReceiptChangedPaths)?;
            validate_text(&receipt.stdout_preview, LimitId::ReceiptStdoutChars)?;
            validate_text(&receipt.stderr_preview, LimitId::ReceiptStderrChars)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(remote = "PlanSnapshot")]
struct PlanSnapshotWire {
    ok: bool,
    command: String,
    schema_version: u64,
    snapshot_kind: SnapshotKind,
    generated_at_ms: u64,
    basis_epoch: RecorderEpochId,
    detail_observed_at_ms: u64,
    gates_observed_at_ms: u64,
    decisions_observed_at_ms: u64,
    plan: PlanSummary,
    body: Option<BoundedText>,
    gates: Option<GatesObservation>,
    decisions: Vec<Decision>,
    receipts: Vec<Receipt>,
    limits: PlanLimits,
    errors: Vec<SnapshotError>,
}

impl Serialize for PlanSnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        PlanSnapshotWire::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for PlanSnapshot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let snapshot = PlanSnapshotWire::deserialize(deserializer)?;
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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecorderCounts {
    pub sessions: u64,
    pub session_events: u64,
    pub plans: u64,
    pub plan_events: u64,
    pub open_plans: u64,
    pub decisions: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenPlan {
    pub plan_id: String,
    pub title: String,
    pub body_path: Option<String>,
    pub opened_at_ms: Option<u64>,
    pub baseline_ref: Option<String>,
    pub baseline_oid: Option<String>,
    pub baseline_error: Option<String>,
    pub gates: Option<GatesObservation>,
    pub gates_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanSummary {
    pub plan_id: String,
    pub title: String,
    pub state: String,
    pub opened_at_ms: Option<u64>,
    pub closed_at_ms: Option<u64>,
    pub resolution: Option<String>,
    pub duration_ms: Option<u64>,
    pub baseline_ref: Option<String>,
    pub baseline_oid: Option<String>,
    pub baseline_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GatesObservation {
    pub overall: String,
    pub gates: BoundedRows<GateObservation>,
}

impl GatesObservation {
    fn validate(&self) -> Result<(), String> {
        validate_rows(&self.gates, LimitId::GateRows)?;
        for gate in self.gates.items() {
            gate.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GateObservation {
    pub id: String,
    pub tool: Option<String>,
    pub skill: Option<String>,
    pub required: bool,
    pub status: String,
    pub freshness: Option<String>,
    pub ended_at_ms: Option<u64>,
    pub diff_summary: Option<String>,
    pub changed_paths: BoundedRows<String>,
    pub matching_paths: BoundedRows<String>,
    pub findings: BoundedRows<GateFinding>,
    pub remediation: Option<Remediation>,
}

impl GateObservation {
    fn validate(&self) -> Result<(), String> {
        validate_rows(&self.changed_paths, LimitId::GateChangedPaths)?;
        validate_rows(&self.matching_paths, LimitId::GateMatchingPaths)?;
        validate_rows(&self.findings, LimitId::GateFindings)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GateFinding {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub line: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Remediation {
    pub argv: Vec<String>,
    pub display: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Failure {
    pub id: String,
    pub tool_name: String,
    pub plan_id: Option<String>,
    pub ended_at_ms: Option<u64>,
    pub exit_status: i64,
    pub stderr_preview: BoundedText,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolStat {
    pub tool: String,
    pub runs: u64,
    pub failures: u64,
    pub last_exit_status: i64,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimelineRow {
    Receipt(ReceiptTimelineRow),
    Plan(PlanTimelineRow),
    Session(SessionTimelineRow),
    Decision(DecisionTimelineRow),
}

impl TimelineRow {
    #[must_use]
    pub fn stable_identity(&self) -> &str {
        match self {
            Self::Receipt(row) => &row.stable_identity,
            Self::Plan(row) => &row.stable_identity,
            Self::Session(row) => &row.stable_identity,
            Self::Decision(row) => &row.stable_identity,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Receipt(row) => {
                if let Some(stderr) = &row.stderr_preview {
                    validate_text(stderr, LimitId::FailureStderrChars)?;
                }
            }
            Self::Decision(row) => {
                validate_text(&row.rationale, LimitId::TimelineDecisionRationaleChars)?;
            }
            Self::Plan(_) | Self::Session(_) => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReceiptTimelineRow {
    pub stable_identity: String,
    pub timestamp_ms: Option<u64>,
    pub id: String,
    pub tool_name: String,
    pub invoked_command_key: Option<String>,
    pub plan_id: Option<String>,
    pub session_id: Option<String>,
    pub exit_status: i64,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub diff_summary: Option<String>,
    pub changed_path_count: Option<u64>,
    pub stderr_preview: Option<BoundedText>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanTimelineRow {
    pub stable_identity: String,
    pub timestamp_ms: Option<u64>,
    pub id: String,
    pub event: String,
    pub plan_id: String,
    pub title: Option<String>,
    pub resolution: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionTimelineRow {
    pub stable_identity: String,
    pub timestamp_ms: Option<u64>,
    pub id: String,
    pub event: String,
    pub session_id: String,
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DecisionTimelineRow {
    pub stable_identity: String,
    pub timestamp_ms: Option<u64>,
    pub id: String,
    pub plan_id: Option<String>,
    pub title: String,
    pub selected_option: String,
    pub rationale: BoundedText,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Decision {
    pub id: String,
    pub session_id: Option<String>,
    pub plan_id: Option<String>,
    pub timestamp_ms: u64,
    pub title: String,
    pub selected_option: String,
    pub alternatives: Vec<String>,
    pub rationale: BoundedText,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Receipt {
    pub timestamp_ms: Option<u64>,
    pub id: String,
    pub tool_name: String,
    pub invoked_command_key: Option<String>,
    pub plan_id: Option<String>,
    pub session_id: Option<String>,
    pub exit_status: i64,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub diff_summary: Option<String>,
    pub changed_paths: BoundedRows<String>,
    pub stdout_preview: BoundedText,
    pub stderr_preview: BoundedText,
}
