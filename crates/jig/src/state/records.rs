//! Durable state-stream record schemas and their compatibility serde contracts.
//!
//! Keep filesystem, locking, and JSONL traversal behavior out of this module.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::git_receipts::DiffStat;
use jig_contract::{RunConclusion, RunPlan, TargetId, TargetRunResult};

/// A receipt written by loop workflows. Check receipts from earlier runtimes
/// also carried target, run and freshness fields; readers ignore them.
#[derive(Debug, Serialize, serde::Deserialize)]
pub(crate) struct ReceiptRecord {
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
    /// Only on check receipts from earlier runtimes; linkage diagnosis reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run_id: Option<String>,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_run_history::plan"
    )]
    pub(super) plan: Option<RunPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) target: Option<TargetId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "legacy_run_history::result"
    )]
    pub(super) result: Option<TargetRunResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) conclusion: Option<RunConclusion>,
}

/// Run history written before receipts and target freshness were removed.
/// Planned targets and target results then carried identity, freshness and
/// receipt fields; they are dropped on read so existing journals, archives
/// and backups stay readable under the strict current contract types.
mod legacy_run_history {
    use serde::de::{Deserialize, DeserializeOwned, Deserializer, Error};
    use serde_json::Value;

    const RETIRED_PLANNED_TARGET_FIELDS: &[&str] = &["target_identity", "target_identity_error"];
    const RETIRED_TARGET_RESULT_FIELDS: &[&str] =
        &["target_freshness", "receipt_id", "reused_from"];

    pub(super) fn plan<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: DeserializeOwned,
    {
        read(deserializer, |value| {
            if let Some(targets) = value.get_mut("targets").and_then(Value::as_array_mut) {
                for target in targets {
                    strip(target, RETIRED_PLANNED_TARGET_FIELDS);
                }
            }
        })
    }

    pub(super) fn result<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: DeserializeOwned,
    {
        read(deserializer, |value| {
            strip(value, RETIRED_TARGET_RESULT_FIELDS)
        })
    }

    fn read<'de, D, T>(
        deserializer: D,
        retire: impl FnOnce(&mut Value),
    ) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: DeserializeOwned,
    {
        let Some(mut value) = Option::<Value>::deserialize(deserializer)? else {
            return Ok(None);
        };
        retire(&mut value);
        serde_json::from_value(value)
            .map(Some)
            .map_err(D::Error::custom)
    }

    fn strip(value: &mut Value, fields: &[&str]) {
        if let Some(object) = value.as_object_mut() {
            for field in fields {
                object.remove(*field);
            }
        }
    }
}
