//! Durable state-stream record schemas and their compatibility serde contracts.
//!
//! Keep filesystem, locking, and JSONL traversal behavior out of this module.

use serde::{Deserialize, Serialize};

use jig_contract::{RunConclusion, RunPlan, TargetId, TargetRunResult};

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
