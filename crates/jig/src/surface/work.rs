//! Explicit agent-v1 work output. These observations are never closure authority.
use jig_contract::TargetId;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkCompletion {
    pub ok: bool,
    pub schema_version: u32,
    pub command: String,
    pub plan_id: String,
    pub plan_state: String,
    pub observed_at_ms: u64,
    pub finish_ready: bool,
    pub readiness_basis: String,
    pub observation: ObservationSummary,
    pub gates: Vec<GateSummary>,
    pub gate_count: usize,
    pub gates_truncated: bool,
    pub activity: Vec<CheckActivity>,
    pub activity_count: usize,
    pub activity_truncated: bool,
    pub error: Option<String>,
    pub next_step: Option<RecoveryCommand>,
    pub evidence: RecoveryCommand,
    pub receipts: RecoveryCommand,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateSummary {
    pub id: String,
    pub kind: String,
    pub required: bool,
    pub status: String,
    pub freshness: Option<String>,
    pub reason: String,
    pub reason_truncated: bool,
    pub targets: Vec<TargetSummary>,
    pub target_count: usize,
    pub targets_truncated: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationSummary {
    pub status: String,
    pub message: String,
    pub message_truncated: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetSummary {
    pub target: TargetId,
    pub status: String,
    pub freshness: String,
    pub reason: String,
    pub reason_truncated: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckActivity {
    pub subject: String,
    pub disposition: String,
    pub status: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryCommand {
    pub argv: Vec<String>,
    pub read_only: bool,
}

pub(crate) const MAX_ROWS: usize = 50;
pub(crate) const MAX_REASON_CHARS: usize = 256;

pub(crate) fn bounded_reason(reason: &str) -> (String, bool) {
    let mut chars = reason.chars();
    let text = chars.by_ref().take(MAX_REASON_CHARS).collect();
    (text, chars.next().is_some())
}

impl RecoveryCommand {
    pub(crate) fn work(command: &str, plan_id: &str, read_only: bool) -> Self {
        Self {
            argv: vec![
                "scripts/jig".into(),
                "work".into(),
                command.into(),
                "--plan-id".into(),
                plan_id.into(),
            ],
            read_only,
        }
    }
}
