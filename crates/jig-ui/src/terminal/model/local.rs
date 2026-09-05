use time::OffsetDateTime;

use crate::dashboard::{
    AppliedLimit, BoundedRows, BoundedText, DecisionTimelineRow, ExhaustedAttempt, Failure,
    GateObservation, GatesObservation, HarnessObservation, LoopAttempt, LoopLease, LoopObservation,
    LoopStateError, LoopWorkflow, OpenPlan, PlanSummary, ReceiptTimelineRow, RecorderCounts,
    RecorderEpochId, RecorderLimits, RecorderSnapshot, ScheduledOccurrence, SessionTimelineRow,
    SnapshotError, TimelineRow, ToolStat,
};

use super::sanitize_text;

mod gates;
mod health;
pub(crate) use gates::GateSetView;
use gates::RemediationView;
use health::health_items;

#[derive(Clone, Debug)]
pub(crate) struct LocalDashboard {
    pub(crate) schema_version: u64,
    pub(crate) generated_at_ms: u64,
    pub(crate) epoch_id: RecorderEpochId,
    pub(crate) repo: LocalRepositoryView,
    pub(crate) harness: LocalHarnessView,
    pub(crate) current_session_id: Option<String>,
    pub(crate) counts: RecorderCounts,
    pub(crate) work: Vec<WorkPlanView>,
    pub(crate) failures: Vec<FailureView>,
    pub(crate) tools: Vec<ToolView>,
    pub(crate) health: Vec<HealthItemView>,
    pub(crate) timeline: Vec<TimelineItemView>,
    pub(crate) timeline_limit: usize,
    pub(crate) limits: LocalLimitsView,
    pub(crate) errors: Vec<LocalErrorView>,
}

impl From<RecorderSnapshot> for LocalDashboard {
    fn from(snapshot: RecorderSnapshot) -> Self {
        let failures = snapshot
            .failures
            .into_iter()
            .map(FailureView::from)
            .collect::<Vec<_>>();
        let tools = snapshot
            .tool_stats
            .into_iter()
            .map(ToolView::from)
            .collect::<Vec<_>>();
        let mut work = snapshot
            .open_plans
            .into_iter()
            .map(WorkPlanView::from)
            .collect::<Vec<_>>();
        work.extend(snapshot.history.into_iter().map(WorkPlanView::from));
        let health = health_items(&failures, &tools, snapshot.loops.as_ref());
        Self {
            schema_version: snapshot.schema_version,
            generated_at_ms: snapshot.generated_at_ms,
            epoch_id: snapshot.epoch_id,
            repo: snapshot.repo.into(),
            harness: snapshot.harness.into(),
            current_session_id: snapshot.current_session_id.as_deref().map(sanitize_text),
            counts: snapshot.counts,
            work,
            failures,
            tools,
            health,
            timeline: snapshot
                .timeline
                .into_iter()
                .map(TimelineItemView::from)
                .collect(),
            timeline_limit: snapshot.timeline_limit,
            limits: snapshot.limits.into(),
            errors: snapshot.errors.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalRepositoryView {
    pub(crate) name: String,
    pub(crate) default_branch: String,
    pub(crate) source_commit: Option<String>,
    pub(crate) source_path: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) detached: bool,
}

impl From<crate::dashboard::RepositoryObservation> for LocalRepositoryView {
    fn from(repo: crate::dashboard::RepositoryObservation) -> Self {
        Self {
            name: sanitize_text(&repo.name),
            default_branch: sanitize_text(&repo.default_branch),
            source_commit: repo.source_commit.as_deref().map(sanitize_text),
            source_path: repo.source_path.as_deref().map(sanitize_text),
            branch: repo.branch.as_deref().map(sanitize_text),
            detached: repo.detached,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalHarnessView {
    pub(crate) runtime_version: String,
    pub(crate) contract_version: u64,
}

impl From<HarnessObservation> for LocalHarnessView {
    fn from(harness: HarnessObservation) -> Self {
        let runtime = if harness.runtime_version.is_empty() {
            harness.jig_version.unwrap_or_else(|| "-".to_string())
        } else {
            harness.runtime_version
        };
        Self {
            runtime_version: sanitize_text(&runtime),
            contract_version: harness.contract_version,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkState {
    Open,
    Closed,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkPlanView {
    pub(crate) plan_id: String,
    pub(crate) display_plan_id: String,
    pub(crate) title: String,
    pub(crate) state: WorkState,
    pub(crate) state_label: String,
    pub(crate) opened_at: String,
    pub(crate) closed_at: String,
    pub(crate) resolution: Option<String>,
    pub(crate) duration: String,
    pub(crate) baseline_ref: Option<String>,
    pub(crate) baseline_oid: Option<String>,
    pub(crate) baseline_error: Option<String>,
    pub(crate) gates: Option<GateSummarySetView>,
    pub(crate) gates_error: Option<String>,
}

impl From<OpenPlan> for WorkPlanView {
    fn from(plan: OpenPlan) -> Self {
        Self {
            display_plan_id: sanitize_text(&plan.plan_id),
            plan_id: plan.plan_id,
            title: sanitize_text(&plan.title),
            state: WorkState::Open,
            state_label: "open".to_string(),
            opened_at: format_timestamp(plan.opened_at_ms),
            closed_at: "—".to_string(),
            resolution: None,
            duration: "—".to_string(),
            baseline_ref: plan.baseline_ref.as_deref().map(sanitize_text),
            baseline_oid: plan.baseline_oid.as_deref().map(sanitize_text),
            baseline_error: plan.baseline_error.as_deref().map(sanitize_text),
            gates: plan.gates.map(Into::into),
            gates_error: plan.gates_error.as_deref().map(sanitize_text),
        }
    }
}

impl From<PlanSummary> for WorkPlanView {
    fn from(plan: PlanSummary) -> Self {
        Self {
            display_plan_id: sanitize_text(&plan.plan_id),
            plan_id: plan.plan_id,
            title: sanitize_text(&plan.title),
            state: WorkState::Closed,
            state_label: sanitize_text(&plan.state),
            opened_at: format_timestamp(plan.opened_at_ms),
            closed_at: format_timestamp(plan.closed_at_ms),
            resolution: plan.resolution.as_deref().map(sanitize_text),
            duration: format_duration(plan.duration_ms),
            baseline_ref: plan.baseline_ref.as_deref().map(sanitize_text),
            baseline_oid: plan.baseline_oid.as_deref().map(sanitize_text),
            baseline_error: plan.baseline_error.as_deref().map(sanitize_text),
            gates: None,
            gates_error: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GateSummarySetView {
    pub(crate) overall: String,
    pub(crate) gates: Vec<GateSummaryView>,
    pub(crate) limit: LimitView,
}

impl From<GatesObservation> for GateSummarySetView {
    fn from(gates: GatesObservation) -> Self {
        Self {
            overall: sanitize_text(&gates.overall),
            limit: LimitView::from_rows(&gates.gates),
            gates: gates
                .gates
                .items()
                .iter()
                .map(GateSummaryView::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GateSummaryView {
    pub(crate) id: String,
    pub(crate) subject: String,
    pub(crate) status: String,
    pub(crate) freshness: String,
    pub(crate) ended_at: String,
}

impl From<&GateObservation> for GateSummaryView {
    fn from(gate: &GateObservation) -> Self {
        Self {
            id: sanitize_text(&gate.id),
            subject: gate
                .tool
                .as_deref()
                .or(gate.skill.as_deref())
                .map(sanitize_text)
                .unwrap_or_else(|| "—".to_string()),
            status: sanitize_text(&gate.status),
            freshness: gate
                .freshness
                .as_deref()
                .map(sanitize_text)
                .unwrap_or_else(|| "—".to_string()),
            ended_at: format_timestamp(gate.ended_at_ms),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FailureView {
    pub(crate) id: String,
    pub(crate) display_id: String,
    pub(crate) tool: String,
    pub(crate) display_plan_id: Option<String>,
    pub(crate) ended_at: String,
    pub(crate) exit_status: i64,
    pub(crate) stderr: TextView,
}

impl From<Failure> for FailureView {
    fn from(failure: Failure) -> Self {
        Self {
            display_id: sanitize_text(&failure.id),
            id: failure.id,
            tool: sanitize_text(&failure.tool_name),
            display_plan_id: failure.plan_id.as_deref().map(sanitize_text),
            ended_at: format_timestamp(failure.ended_at_ms),
            exit_status: failure.exit_status,
            stderr: failure.stderr_preview.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ToolView {
    pub(crate) raw_tool: String,
    pub(crate) tool: String,
    pub(crate) runs: u64,
    pub(crate) failures: u64,
    pub(crate) last_status: String,
    pub(crate) last_ended_at: String,
    pub(crate) average: String,
}

impl From<ToolStat> for ToolView {
    fn from(tool: ToolStat) -> Self {
        Self {
            tool: sanitize_text(&tool.tool),
            raw_tool: tool.tool,
            runs: tool.runs,
            failures: tool.failures,
            last_status: if tool.last_exit_status == 0 {
                "pass".to_string()
            } else {
                format!("exit {}", tool.last_exit_status)
            },
            last_ended_at: format_timestamp(Some(tool.last_ended_at_ms)),
            average: format_duration(Some(tool.avg_duration_ms)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimelineFilter {
    All,
    Receipts,
    Failures,
    Plans,
    Sessions,
    Decisions,
}

impl TimelineFilter {
    pub(crate) const ALL: [Self; 6] = [
        Self::All,
        Self::Receipts,
        Self::Failures,
        Self::Plans,
        Self::Sessions,
        Self::Decisions,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Receipts => "receipts",
            Self::Failures => "failures",
            Self::Plans => "plans",
            Self::Sessions => "sessions",
            Self::Decisions => "decisions",
        }
    }

    pub(crate) fn matches(self, row: &TimelineItemView) -> bool {
        match self {
            Self::All => true,
            Self::Receipts => row.kind == TimelineKind::Receipt,
            Self::Failures => row.kind == TimelineKind::Receipt && row.failed_receipt,
            Self::Plans => row.kind == TimelineKind::Plan,
            Self::Sessions => row.kind == TimelineKind::Session,
            Self::Decisions => row.kind == TimelineKind::Decision,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimelineKind {
    Receipt,
    Plan,
    Session,
    Decision,
}

impl TimelineKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::Plan => "plan",
            Self::Session => "session",
            Self::Decision => "decision",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineItemView {
    pub(crate) identity: String,
    pub(crate) display_identity: String,
    pub(crate) kind: TimelineKind,
    pub(crate) timestamp: String,
    pub(crate) plan_id: Option<String>,
    pub(crate) primary: String,
    pub(crate) secondary: String,
    pub(crate) failed_receipt: bool,
    pub(crate) detail: DetailDocument,
}

impl From<TimelineRow> for TimelineItemView {
    fn from(row: TimelineRow) -> Self {
        match row {
            TimelineRow::Receipt(row) => receipt_timeline(row),
            TimelineRow::Plan(row) => {
                let mut lines = vec![
                    field("Event", &row.event),
                    field("Plan", &row.plan_id),
                    field("Record", &row.id),
                ];
                push_optional(&mut lines, "Title", row.title.as_deref());
                push_optional(&mut lines, "Resolution", row.resolution.as_deref());
                timeline_view(TimelineDraft {
                    identity: row.stable_identity,
                    kind: TimelineKind::Plan,
                    timestamp_ms: row.timestamp_ms,
                    plan_id: Some(row.plan_id.clone()),
                    primary: format!(
                        "{} {}",
                        sanitize_text(&row.event),
                        sanitize_text(&row.plan_id)
                    ),
                    secondary: row.title.as_deref().map(sanitize_text).unwrap_or_default(),
                    failed_receipt: false,
                    detail: DetailDocument::new("Plan event", lines),
                })
            }
            TimelineRow::Session(row) => session_timeline(row),
            TimelineRow::Decision(row) => decision_timeline(row),
        }
    }
}

fn receipt_timeline(row: ReceiptTimelineRow) -> TimelineItemView {
    let failed = row.exit_status != 0;
    let mut lines = vec![
        field("Receipt", &row.id),
        field("Tool", &row.tool_name),
        format!("Exit: {}", row.exit_status),
        format!("Started: {}", format_timestamp(row.started_at_ms)),
        format!("Ended: {}", format_timestamp(row.ended_at_ms)),
        format!("Duration: {}", format_duration(row.duration_ms)),
    ];
    push_optional(
        &mut lines,
        "Command key",
        row.invoked_command_key.as_deref(),
    );
    push_optional(&mut lines, "Plan", row.plan_id.as_deref());
    push_optional(&mut lines, "Session", row.session_id.as_deref());
    push_optional(&mut lines, "Diff", row.diff_summary.as_deref());
    if let Some(count) = row.changed_path_count {
        lines.push(format!("Changed paths: {count}"));
    }
    if let Some(stderr) = row.stderr_preview {
        append_text(&mut lines, "Stderr", &stderr.into());
    }
    timeline_view(TimelineDraft {
        identity: row.stable_identity,
        kind: TimelineKind::Receipt,
        timestamp_ms: row.timestamp_ms,
        plan_id: row.plan_id,
        primary: format!(
            "{} {}",
            sanitize_text(&row.tool_name),
            if failed {
                format!("exit {}", row.exit_status)
            } else {
                "pass".to_string()
            }
        ),
        secondary: row
            .diff_summary
            .as_deref()
            .map(sanitize_text)
            .unwrap_or_default(),
        failed_receipt: failed,
        detail: DetailDocument::new("Receipt event", lines),
    })
}

fn session_timeline(row: SessionTimelineRow) -> TimelineItemView {
    let mut lines = vec![
        field("Session", &row.session_id),
        field("Event", &row.event),
        field("Record", &row.id),
    ];
    push_optional(&mut lines, "Outcome", row.outcome.as_deref());
    timeline_view(TimelineDraft {
        identity: row.stable_identity,
        kind: TimelineKind::Session,
        timestamp_ms: row.timestamp_ms,
        plan_id: None,
        primary: format!(
            "{} {}",
            sanitize_text(&row.event),
            sanitize_text(&row.session_id)
        ),
        secondary: row
            .outcome
            .as_deref()
            .map(sanitize_text)
            .unwrap_or_default(),
        failed_receipt: false,
        detail: DetailDocument::new("Session event", lines),
    })
}

fn decision_timeline(row: DecisionTimelineRow) -> TimelineItemView {
    let rationale: TextView = row.rationale.into();
    let mut lines = vec![
        field("Decision", &row.id),
        field("Title", &row.title),
        field("Selected", &row.selected_option),
    ];
    push_optional(&mut lines, "Plan", row.plan_id.as_deref());
    append_text(&mut lines, "Rationale", &rationale);
    timeline_view(TimelineDraft {
        identity: row.stable_identity,
        kind: TimelineKind::Decision,
        timestamp_ms: row.timestamp_ms,
        plan_id: row.plan_id,
        primary: format!(
            "{} → {}",
            sanitize_text(&row.title),
            sanitize_text(&row.selected_option)
        ),
        secondary: rationale.lines.join(" · "),
        failed_receipt: false,
        detail: DetailDocument::new("Decision event", lines),
    })
}

struct TimelineDraft {
    identity: String,
    kind: TimelineKind,
    timestamp_ms: Option<u64>,
    plan_id: Option<String>,
    primary: String,
    secondary: String,
    failed_receipt: bool,
    detail: DetailDocument,
}

fn timeline_view(draft: TimelineDraft) -> TimelineItemView {
    TimelineItemView {
        display_identity: sanitize_text(&draft.identity),
        identity: draft.identity,
        kind: draft.kind,
        timestamp: format_timestamp(draft.timestamp_ms),
        plan_id: draft.plan_id,
        primary: draft.primary,
        secondary: draft.secondary,
        failed_receipt: draft.failed_receipt,
        detail: draft.detail,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HealthItemView {
    pub(crate) identity: String,
    pub(crate) section: &'static str,
    pub(crate) primary: String,
    pub(crate) secondary: String,
    pub(crate) detail: DetailDocument,
}

#[derive(Clone, Debug)]
pub(crate) struct DetailDocument {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
}

impl DetailDocument {
    pub(crate) fn new(title: &str, lines: Vec<String>) -> Self {
        Self {
            title: title.to_string(),
            lines,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextView {
    pub(crate) lines: Vec<String>,
    pub(crate) limit: LimitView,
}

impl From<BoundedText> for TextView {
    fn from(text: BoundedText) -> Self {
        let sanitized = sanitize_multiline(text.text());
        let lines = sanitized.lines().map(ToOwned::to_owned).collect();
        Self {
            lines,
            limit: LimitView {
                applied: text.applied_chars(),
                omitted: text.omitted_chars(),
            },
        }
    }
}

fn sanitize_multiline(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| sanitize_text(&line.replace('\t', "    ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LimitView {
    pub(crate) applied: usize,
    pub(crate) omitted: Option<usize>,
}

impl LimitView {
    pub(crate) fn from_rows<T>(rows: &BoundedRows<T>) -> Self {
        Self {
            applied: rows.applied(),
            omitted: rows.omitted(),
        }
    }

    pub(crate) fn label(self, noun: &str) -> String {
        match self.omitted {
            Some(0) => format!("limit {} {noun}; none omitted", self.applied),
            Some(count) => format!("limit {} {noun}; {count} omitted", self.applied),
            None => format!("limit {} {noun}; omitted count unknown", self.applied),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalLimitsView {
    pub(crate) open_plans: LimitView,
    pub(crate) history: LimitView,
    pub(crate) failures: LimitView,
    pub(crate) tools: LimitView,
    pub(crate) timeline: LimitView,
}

impl From<RecorderLimits> for LocalLimitsView {
    fn from(limits: RecorderLimits) -> Self {
        Self {
            open_plans: limits.open_plans.into(),
            history: limits.history.into(),
            failures: limits.failures.into(),
            tools: limits.tool_stats.into(),
            timeline: limits.timeline.into(),
        }
    }
}

impl From<AppliedLimit> for LimitView {
    fn from(limit: AppliedLimit) -> Self {
        Self {
            applied: limit.applied,
            omitted: limit.omitted,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LocalErrorView {
    pub(crate) scope: String,
    pub(crate) code: String,
    pub(crate) subject: Option<String>,
    pub(crate) message: String,
}

impl From<SnapshotError> for LocalErrorView {
    fn from(error: SnapshotError) -> Self {
        Self {
            scope: sanitize_text(error.scope()),
            code: sanitize_text(error.code()),
            subject: error.subject_id().map(sanitize_text),
            message: sanitize_text(error.message()),
        }
    }
}

pub(crate) fn format_timestamp(timestamp_ms: Option<u64>) -> String {
    let Some(ms) = timestamp_ms else {
        return "—".to_string();
    };
    let Ok(seconds) = i64::try_from(ms / 1_000) else {
        return format!("{ms}ms");
    };
    let Ok(time) = OffsetDateTime::from_unix_timestamp(seconds) else {
        return format!("{ms}ms");
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

pub(crate) fn format_duration(duration_ms: Option<u64>) -> String {
    match duration_ms {
        None => "—".to_string(),
        Some(ms) if ms < 1_000 => format!("{ms}ms"),
        Some(ms) if ms < 60_000 => format!("{:.1}s", ms as f64 / 1_000.0),
        Some(ms) => format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1_000),
    }
}

pub(super) fn sanitize_rows(rows: &BoundedRows<String>) -> Vec<String> {
    rows.items()
        .iter()
        .map(|value| sanitize_text(value))
        .collect()
}

fn field(label: &str, value: &str) -> String {
    format!("{label}: {}", sanitize_text(value))
}

fn push_optional(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        lines.push(field(label, value));
    }
}

fn append_text(lines: &mut Vec<String>, label: &str, text: &TextView) {
    lines.push(format!("{label}:"));
    lines.extend(text.lines.iter().cloned());
    lines.push(text.limit.label("characters"));
}
