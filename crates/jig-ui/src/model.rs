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
    /// Deprecated v2/v3 generated product pin; absent for contract v4 repos.
    #[serde(default)]
    pub jig_version: Option<String>,
    #[serde(default)]
    pub runtime_version: String,
    pub contract_version: u64,
}

impl HarnessView {
    /// Returns the executing runtime version, falling back to the legacy
    /// generated product pin when deserializing a pre-v4 snapshot.
    pub fn display_runtime_version(&self) -> &str {
        if self.runtime_version.is_empty() {
            self.jig_version.as_deref().unwrap_or("-")
        } else {
            &self.runtime_version
        }
    }
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
    #[serde(default)]
    pub baseline_ref: Option<String>,
    #[serde(default)]
    pub baseline_oid: Option<String>,
    #[serde(default)]
    pub baseline_error: Option<String>,
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
    #[serde(default)]
    pub baseline_ref: Option<String>,
    #[serde(default)]
    pub baseline_oid: Option<String>,
    #[serde(default)]
    pub baseline_error: Option<String>,
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
    pub scheduled_occurrences: Vec<ScheduledOccurrenceView>,
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
    #[serde(default)]
    pub schedule: Option<WorkflowScheduleView>,
    #[serde(default)]
    pub schedule_state: Option<WorkflowScheduleStateView>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowScheduleView {
    pub cron: String,
    pub timezone: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowScheduleStateView {
    pub due_at_ms: Option<u64>,
    pub next_at_ms: u64,
    pub last_scheduled_at_ms: Option<u64>,
    pub last_status: Option<String>,
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
    #[serde(default)]
    pub scheduled_occurrences: Vec<ScheduledOccurrenceView>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExhaustedAttemptView {
    pub workflow_id: String,
    pub item_key: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduledOccurrenceView {
    pub occurrence_id: String,
    pub workflow_id: String,
    pub scheduled_at_ms: u64,
    #[serde(default)]
    pub started_at_ms: u64,
    pub status: String,
    pub finished_at_ms: Option<u64>,
    pub worker_receipt_id: Option<String>,
    pub worktree: Option<String>,
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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

#[cfg(test)]
mod tests {
    use super::{HarnessView, LoopsView};

    #[test]
    fn legacy_snapshot_uses_product_version_as_runtime_display_fallback() {
        let legacy: HarnessView = serde_json::from_value(serde_json::json!({
            "jig_version": "0.1.0",
            "contract_version": 3
        }))
        .unwrap();
        assert_eq!(legacy.display_runtime_version(), "0.1.0");

        let current: HarnessView = serde_json::from_value(serde_json::json!({
            "jig_version": "legacy-pin",
            "runtime_version": "0.2.0",
            "contract_version": 4
        }))
        .unwrap();
        assert_eq!(current.display_runtime_version(), "0.2.0");
    }

    #[test]
    fn loops_view_accepts_runtime_exhausted_attempt_fields() {
        let loops: LoopsView = serde_json::from_value(serde_json::json!({
            "needs_attention": {"exhausted_attempts": [{
                "workflow_id": "pr-manager",
                "item_key": "pr-17",
                "attempts": 3,
            }]},
        }))
        .unwrap();
        let attempt = &loops.needs_attention.exhausted_attempts[0];
        assert_eq!(attempt.workflow_id, "pr-manager");
        assert_eq!(attempt.item_key, "pr-17");
        assert_eq!(attempt.extra["attempts"], 3);
    }
}
