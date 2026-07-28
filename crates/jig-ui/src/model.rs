use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub ok: bool,
    pub command: String,
    pub generated_at_ms: u64,
    pub repo: RepoView,
    pub harness: HarnessView,
    pub current_session_id: Option<String>,
    pub counts: CountsView,
    pub open_plans: Vec<OpenPlanView>,
    pub history: Vec<PlanSummary>,
    pub failures: Vec<FailureView>,
    pub tool_stats: Vec<ToolStatView>,
    pub loops: Option<LoopsView>,
    pub loops_error: Option<String>,
    pub timeline: Vec<TimelineItem>,
    pub timeline_show: String,
    pub timeline_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanSnapshot {
    pub ok: bool,
    pub command: String,
    pub generated_at_ms: u64,
    pub plan: PlanSummary,
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_error: Option<String>,
    pub gates: Option<GatesView>,
    pub gates_error: Option<String>,
    pub decisions: Vec<DecisionView>,
    pub receipts: Vec<ReceiptView>,
    pub receipts_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepoView {
    pub name: String,
    pub default_branch: String,
    pub source_commit: Option<String>,
    pub source_path: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarnessView {
    pub jig_version: String,
    pub contract_version: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CountsView {
    pub sessions: u64,
    pub session_events: u64,
    pub plans: u64,
    pub plan_events: u64,
    pub open_plans: u64,
    pub decisions: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenPlanView {
    pub plan_id: String,
    pub title: String,
    pub body_path: Option<String>,
    pub opened_at_ms: Option<u64>,
    pub gates: Option<GatesView>,
    pub gates_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanSummary {
    pub plan_id: String,
    pub title: String,
    pub state: String,
    pub opened_at_ms: Option<u64>,
    pub closed_at_ms: Option<u64>,
    pub resolution: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GatesView {
    pub overall: String,
    pub gates: Vec<GateView>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateView {
    pub id: String,
    pub tool: Option<String>,
    pub skill: Option<String>,
    pub required: bool,
    pub status: String,
    pub freshness: Option<String>,
    pub ended_at_ms: Option<u64>,
    pub diff_summary: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailureView {
    pub id: String,
    pub tool_name: String,
    pub plan_id: Option<String>,
    pub ended_at_ms: Option<u64>,
    pub exit_status: i64,
    pub stderr_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolStatView {
    pub tool: String,
    pub runs: u64,
    pub failures: u64,
    pub last_exit_status: i64,
    pub last_ended_at_ms: u64,
    pub avg_duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoopsView {
    #[serde(default)]
    pub workflows: Vec<WorkflowView>,
    #[serde(default)]
    pub leases: Vec<LeaseView>,
    #[serde(default)]
    pub needs_attention: LoopAttentionView,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowView {
    pub id: String,
    pub kind: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaseView {
    pub key: String,
    pub expires_at_ms: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoopAttentionView {
    #[serde(default)]
    pub exhausted_attempts: Vec<ExhaustedAttemptView>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExhaustedAttemptView {
    pub workflow: String,
    pub item: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TimelineItem {
    #[serde(rename = "receipt")]
    Receipt(ReceiptTimelineView),
    #[serde(rename = "plan")]
    Plan(PlanTimelineView),
    #[serde(rename = "session")]
    Session(SessionTimelineView),
    #[serde(rename = "decision")]
    Decision(DecisionTimelineView),
}
impl TimelineItem {
    pub const fn timestamp_ms(&self) -> Option<u64> {
        match self {
            Self::Receipt(v) => v.ended_at_ms,
            Self::Plan(v) => v.timestamp_ms,
            Self::Session(v) => v.timestamp_ms,
            Self::Decision(v) => v.timestamp_ms,
        }
    }
    pub fn plan_id(&self) -> Option<&str> {
        match self {
            Self::Receipt(v) => v.plan_id.as_deref(),
            Self::Plan(v) => Some(&v.plan_id),
            Self::Session(_) => None,
            Self::Decision(v) => v.plan_id.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptTimelineView {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_preview: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanTimelineView {
    pub timestamp_ms: Option<u64>,
    pub event: String,
    pub plan_id: String,
    pub title: Option<String>,
    pub resolution: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionTimelineView {
    pub timestamp_ms: Option<u64>,
    pub event: String,
    pub session_id: String,
    pub outcome: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionTimelineView {
    pub timestamp_ms: Option<u64>,
    pub id: String,
    pub plan_id: Option<String>,
    pub title: String,
    pub selected_option: String,
    pub rationale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionView {
    pub id: String,
    pub session_id: Option<String>,
    pub plan_id: Option<String>,
    pub timestamp_ms: u64,
    pub title: String,
    pub selected_option: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    pub rationale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptView {
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
    pub changed_paths: Vec<String>,
    pub stdout_preview: String,
    pub stderr_preview: String,
}
