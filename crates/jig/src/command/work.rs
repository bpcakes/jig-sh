//! Structured work command DTOs.

use std::path::PathBuf;

use serde::Deserialize;

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
    pub(crate) plan_id: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) gates: Vec<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkGatesRequest {
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkEvidenceRequest {
    pub(crate) plan_id: Option<String>,
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
