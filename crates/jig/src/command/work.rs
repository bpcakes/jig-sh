//! Structured work command DTOs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_REFINE_MAX_ITERATIONS: usize = 1;

#[derive(Debug)]
pub(crate) enum WorkCommand {
    Goal(WorkGoalRequest),
    Start(WorkStartRequest),
    Append(WorkAppendRequest),
    Check(WorkCheckRequest),
    Gates(WorkGatesRequest),
    Evidence(WorkEvidenceRequest),
    Review(WorkReviewRequest),
    Refine(WorkRefineRequest),
    Decide(WorkDecisionRequest),
    Receipts(WorkReceiptsRequest),
    Status,
    Finish(WorkFinishRequest),
    Retire(WorkRetireRequest),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkCheckPhase {
    Iteration,
    Final,
}

impl WorkCheckPhase {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Iteration => "iteration",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkGoalRequest {
    pub(crate) objective: String,
    pub(crate) success: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) validations: Vec<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) constraints: Vec<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) checkpoints: Vec<String>,
    pub(crate) title: Option<String>,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkStartRequest {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
    pub(crate) base: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkAppendRequest {
    pub(crate) plan_id: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkCheckRequest {
    #[serde(default, deserialize_with = "deserialize_rust_focus")]
    pub(crate) rust_focus:
        std::collections::BTreeMap<jig_contract::TargetId, jig_contract::RustFocusV1>,
    #[serde(skip)]
    pub(crate) projection: crate::surface::ResponseSurface,
    pub(crate) plan_id: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) gates: Vec<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) tools: Vec<String>,
    #[serde(default)]
    pub(crate) phase: Option<WorkCheckPhase>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) explain: bool,
}

fn deserialize_rust_focus<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<jig_contract::TargetId, jig_contract::RustFocusV1>, D::Error>
{
    use serde::de::{Error, MapAccess, Visitor};
    struct FocusMap;
    impl<'de> Visitor<'de> for FocusMap {
        type Value = std::collections::BTreeMap<jig_contract::TargetId, jig_contract::RustFocusV1>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map of target selectors to typed Rust focus, or null")
        }
        fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
            Ok(Default::default())
        }
        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_map(self)
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut result = Self::Value::new();
            while let Some((target, focus)) =
                map.next_entry::<String, jig_contract::RustFocusV1>()?
            {
                if result.len() >= 32 {
                    return Err(A::Error::custom(
                        "at most 32 Rust focus targets are supported",
                    ));
                }
                let target = target.parse().map_err(A::Error::custom)?;
                if result.insert(target, focus).is_some() {
                    return Err(A::Error::custom("duplicate Rust focus target"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_option(FocusMap)
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkGatesRequest {
    #[serde(skip)]
    pub(crate) projection: crate::surface::ResponseSurface,
    pub(crate) plan_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_freshness_timeout_ms")]
    pub(crate) freshness_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkEvidenceRequest {
    #[serde(skip)]
    pub(crate) projection: crate::surface::ResponseSurface,
    pub(crate) plan_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_freshness_timeout_ms")]
    pub(crate) freshness_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkReviewRequest {
    pub(crate) plan_id: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) gates: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkRefineRequest {
    pub(crate) plan_id: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) gates: Vec<String>,
    #[serde(default = "default_refine_max_iterations")]
    pub(crate) max_iterations: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkDecisionRequest {
    pub(crate) title: String,
    pub(crate) selected_option: String,
    pub(crate) rationale: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) alternatives: Vec<String>,
    pub(crate) plan_id: Option<String>,
}

const fn default_refine_max_iterations() -> usize {
    DEFAULT_REFINE_MAX_ITERATIONS
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkReceiptsRequest {
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) failed_only: bool,
    // `usize::default()` is 0, but a null receipt limit should keep the
    // public default instead of asking for zero rows.
    #[serde(
        default = "crate::serde_helpers::default_receipts_limit",
        deserialize_with = "crate::serde_helpers::null_as_default_receipts_limit"
    )]
    pub(crate) limit: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkFinishRequest {
    pub(crate) plan_id: String,
    pub(crate) resolution: Option<String>,
    pub(crate) outcome: Option<String>,
}

/// Explicit non-success retirement of an open work plan.
///
/// `disposition` and `reason` are validated in the runtime so CLI and MCP
/// callers reject the same inputs with the same messages.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkRetireRequest {
    pub(crate) plan_id: String,
    pub(crate) disposition: String,
    pub(crate) reason: String,
    pub(crate) superseded_by: Option<String>,
}

fn deserialize_freshness_timeout_ms<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if (1..=30_000).contains(&value) {
        Ok(Some(value))
    } else {
        Err(serde::de::Error::custom(
            "freshness_timeout_ms must be an integer from 1 through 30000",
        ))
    }
}
