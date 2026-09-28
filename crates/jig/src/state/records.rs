//! Durable state-stream record schemas and their compatibility serde contracts.
//!
//! Keep filesystem, locking, and JSONL traversal behavior out of this module.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::git_receipts::DiffStat;
use jig_contract::{Finding, RunConclusion, RunPlan, TargetId, TargetRunResult};

#[derive(Debug, Serialize, serde::Deserialize)]
pub(crate) struct ReceiptRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_freshness: Option<jig_contract::freshness::TargetFreshnessMetadata>,
    pub(crate) id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: String,
    pub(crate) args: Value,
    #[serde(default)]
    pub(crate) invoked_command_key: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) exit_status: i32,
    pub(crate) stdout_preview: String,
    pub(crate) stderr_preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) evidence: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target: Option<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) config_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finding_count: Option<u64>,
    #[serde(default)]
    pub(crate) findings_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) findings_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) evaluated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) valid_until_ms: Option<u64>,
    pub(crate) changed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) changed_path_count: Option<usize>,
    #[serde(default)]
    pub(crate) changed_paths_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) changed_paths_digest: Option<String>,
    pub(crate) diff_stat: DiffStat,
    #[serde(default)]
    pub(crate) git_status_error: Option<String>,
    #[serde(default)]
    pub(crate) git_diff_stat_error: Option<String>,
    #[serde(default)]
    pub(crate) worktree_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) worktree_fingerprint_error: Option<String>,
}

/// One append-only transition in a durable target run.
///
/// This intentionally uses a string event name and optional payloads rather
/// than a tagged enum so readers can skip events written by newer runtimes.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RunEventRecord {
    pub(super) id: String,
    pub(super) run_id: String,
    pub(super) event: String,
    pub(super) timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) work_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) plan: Option<RunPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) target: Option<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) result: Option<TargetRunResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) conclusion: Option<RunConclusion>,
}
