use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

use super::{
    LoopCodexTask, LoopLease, LoopSchedule, LoopScheduleState, LoopStateError, RecorderEpochId,
};

pub const STATUS_SCHEMA_VERSION: u64 = 2;
pub const STATUS_COMMAND: &str = "status";
pub const STATUS_ROOT_FIELDS: &[&str] = &[
    "ok",
    "command",
    "schema_version",
    "observed_at_ms",
    "outcome",
    "repository",
    "work",
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
    pub work: StatusWorkSnapshot,
    pub loops: Option<StatusLoopObservation>,
    pub errors: Vec<StatusCollectionError>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StatusLocalSnapshot {
    pub epoch_id: RecorderEpochId,
    pub observed_at_ms: u64,
    pub repository: StatusRepositoryObservation,
    pub work: StatusWorkSnapshot,
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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusWorkSnapshot {
    pub state: Option<StatusStateSnapshot>,
    pub gates: Vec<StatusPlanGates>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusStateSnapshot {
    pub ok: bool,
    pub repo: StatusStateRepository,
    pub current_session_id: Option<String>,
    pub counts: StatusStateCounts,
    pub open_plans: Vec<StatusOpenPlan>,
    pub recent_receipts: Vec<StatusReceiptSummary>,
    pub recent_decisions: Vec<StatusDecisionSummary>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusStateRepository {
    pub name: String,
    pub default_branch: String,
    pub source_commit: Option<String>,
    pub source_path: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusStateCounts {
    pub sessions: u64,
    pub session_events: u64,
    pub plans: u64,
    pub plan_events: u64,
    pub open_plans: u64,
    pub receipts: u64,
    pub failed_receipts: u64,
    pub decisions: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusReceiptSummary {
    pub id: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub invoked_command_key: Option<String>,
    pub plan_id: Option<String>,
    pub exit_status: i64,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub diff_summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusDecisionSummary {
    pub id: String,
    pub session_id: Option<String>,
    pub plan_id: Option<String>,
    pub title: String,
    pub selected_option: String,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusOpenPlan {
    pub plan_id: String,
    pub title: String,
    pub body_path: Option<String>,
    pub baseline: Option<StatusPlanBaseline>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusPlanBaseline {
    pub requested_ref: String,
    pub commit_oid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_tree_oid: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusPlanGates {
    pub plan_id: String,
    pub snapshot: Option<StatusGateReport>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusGateReport {
    pub ok: bool,
    pub gates_ok: bool,
    pub plan_id: String,
    pub plan_state: String,
    pub plan_baseline: Option<StatusPlanBaseline>,
    pub current_worktree_fingerprint: Option<String>,
    pub current_worktree_fingerprint_error: Option<String>,
    pub gates: Vec<StatusGate>,
    pub missing_required: Vec<String>,
    pub failed_required: Vec<String>,
    pub stale_required: Vec<String>,
    pub unknown_required: Vec<String>,
    pub unsupported_required: Vec<String>,
    pub overall: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatusGate {
    Check(Box<StatusCheckGate>),
    Evidence(Box<StatusEvidenceGate>),
    CodexReview(Box<StatusCodexReviewGate>),
    Unsupported(StatusUnsupportedGate),
}

impl Serialize for StatusGate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Tagged<'a, T> {
            kind: &'a str,
            #[serde(flatten)]
            fields: &'a T,
        }

        match self {
            Self::Check(fields) => Tagged {
                kind: "check",
                fields,
            }
            .serialize(serializer),
            Self::Evidence(fields) => Tagged {
                kind: "evidence",
                fields,
            }
            .serialize(serializer),
            Self::CodexReview(fields) => Tagged {
                kind: "codex_review",
                fields,
            }
            .serialize(serializer),
            Self::Unsupported(fields) => {
                let mut map = serializer.serialize_map(Some(
                    4 + usize::from(fields.reason.is_some()) + fields.extensions.len(),
                ))?;
                map.serialize_entry("kind", &fields.kind)?;
                map.serialize_entry("id", &fields.id)?;
                map.serialize_entry("required", &fields.required)?;
                map.serialize_entry("status", &fields.status)?;
                if let Some(reason) = &fields.reason {
                    map.serialize_entry("reason", reason)?;
                }
                for (key, value) in &fields.extensions {
                    if !matches!(
                        key.as_str(),
                        "kind" | "id" | "required" | "status" | "reason"
                    ) {
                        map.serialize_entry(key, value)?;
                    }
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for StatusGate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("status gate is missing string field 'kind'"))?
            .to_string();
        match kind.as_str() {
            "check" => serde_json::from_value::<StatusCheckGate>(value.clone())
                .map(|gate| Self::Check(Box::new(gate)))
                .or_else(|_| decode_unsupported(value, &kind).map(Self::Unsupported)),
            "evidence" => serde_json::from_value::<StatusEvidenceGate>(value.clone())
                .map(|gate| Self::Evidence(Box::new(gate)))
                .or_else(|_| decode_unsupported(value, &kind).map(Self::Unsupported)),
            "codex_review" => serde_json::from_value::<StatusCodexReviewGate>(value.clone())
                .map(|gate| Self::CodexReview(Box::new(gate)))
                .or_else(|_| decode_unsupported(value, &kind).map(Self::Unsupported)),
            _ => decode_unsupported(value, &kind).map(Self::Unsupported),
        }
    }
}

fn decode_unsupported<E: serde::de::Error>(
    mut value: Value,
    kind: &str,
) -> Result<StatusUnsupportedGate, E> {
    #[derive(Deserialize)]
    struct UnsupportedWire {
        id: String,
        required: bool,
        status: String,
        #[serde(default)]
        reason: Option<String>,
        #[serde(flatten)]
        extensions: BTreeMap<String, Value>,
    }

    value
        .as_object_mut()
        .ok_or_else(|| E::custom("status gate must be an object"))?
        .remove("kind");
    let fields: UnsupportedWire = serde_json::from_value(value).map_err(E::custom)?;
    Ok(StatusUnsupportedGate {
        kind: kind.to_string(),
        id: fields.id,
        required: fields.required,
        status: fields.status,
        reason: fields.reason,
        extensions: fields.extensions,
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusCheckGate {
    pub id: String,
    pub required: bool,
    pub tool: String,
    pub status: String,
    pub receipt_id: Option<String>,
    pub freshness_receipt_id: Option<String>,
    pub exit_status: Option<i32>,
    pub ended_at_ms: Option<u64>,
    pub freshness: String,
    pub freshness_reason: String,
    pub changed_paths: Vec<String>,
    pub changed_path_count: usize,
    pub changed_paths_truncated: bool,
    pub changed_paths_digest: Option<String>,
    pub diff_summary: Option<String>,
    pub receipt_worktree_fingerprint_error: Option<String>,
    pub current_worktree_fingerprint_error: Option<String>,
    pub evidence_status: Option<String>,
    pub receipt_applicability: Option<String>,
    pub applicability: Option<String>,
    pub applicability_reason: Option<String>,
    pub applicability_error: Option<String>,
    pub paths: Option<Vec<String>>,
    pub paths_ignore: Vec<String>,
    pub reuse: bool,
    pub forced: Option<bool>,
    pub baseline_oid: Option<String>,
    pub receipt_baseline_oid: Option<String>,
    pub gate_signature: Option<String>,
    pub receipt_gate_signature: Option<String>,
    pub scope_fingerprint: Option<String>,
    pub receipt_scope_fingerprint: Option<String>,
    pub matching_paths: Vec<String>,
    pub matching_path_count: usize,
    pub matching_paths_truncated: bool,
    pub matching_paths_digest: Option<String>,
    pub source_plan_id: Option<String>,
    pub source_batch_receipt_id: Option<String>,
    pub source_tool_receipt_id: Option<String>,
    pub valid_until_ms: Option<u64>,
    pub requires_time_validity: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusEvidenceGate {
    pub id: String,
    pub required: bool,
    pub target: Option<String>,
    pub profile: Option<String>,
    pub conclusion: String,
    pub status: String,
    pub run_id: Option<String>,
    pub freshness: String,
    pub freshness_reason: String,
    pub targets: Vec<StatusEvidenceTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusEvidenceTarget {
    pub target: jig_contract::TargetId,
    pub status: String,
    pub receipt_id: Option<String>,
    pub run_id: Option<String>,
    pub exit_status: Option<i32>,
    pub ended_at_ms: Option<u64>,
    pub config_digest: Option<String>,
    pub expected_config_digest: String,
    pub input_digest: Option<String>,
    pub expected_input_digest: Option<String>,
    pub freshness: String,
    pub freshness_reason: String,
    pub changed_paths: Vec<String>,
    pub changed_path_count: usize,
    pub changed_paths_truncated: bool,
    pub changed_paths_digest: Option<String>,
    pub diff_summary: Option<String>,
    pub receipt_worktree_fingerprint_error: Option<String>,
    pub current_worktree_fingerprint_error: Option<String>,
    pub valid_until_ms: Option<u64>,
    pub requires_time_validity: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusCodexReviewGate {
    pub id: String,
    pub required: bool,
    pub skill: String,
    pub status: String,
    pub receipt_id: Option<String>,
    pub exit_status: Option<i32>,
    pub ended_at_ms: Option<u64>,
    pub freshness: String,
    pub freshness_reason: String,
    pub changed_paths: Vec<String>,
    pub changed_path_count: usize,
    pub changed_paths_truncated: bool,
    pub changed_paths_digest: Option<String>,
    pub diff_summary: Option<String>,
    pub finding_count: Option<usize>,
    pub actionable_count: Option<usize>,
    pub retained_finding_count: Option<usize>,
    pub retained_actionable_count: Option<usize>,
    pub findings_truncated: Option<bool>,
    pub actionable_findings_truncated: Option<bool>,
    pub threshold: Option<String>,
    pub parse_error: Option<String>,
    pub receipt_worktree_fingerprint_error: Option<String>,
    pub current_worktree_fingerprint_error: Option<String>,
    pub valid_until_ms: Option<u64>,
    pub requires_time_validity: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusUnsupportedGate {
    pub kind: String,
    pub id: String,
    pub required: bool,
    pub status: String,
    pub reason: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
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
    pub exhausted_attempts: Vec<StatusExhaustedAttempt>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusExhaustedAttempt {
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
pub struct StatusCollectionError {
    pub scope: String,
    pub code: String,
    pub message: String,
}
