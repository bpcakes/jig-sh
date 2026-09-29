//! Action input declarations introduced with contract epoch 8. Jig validates
//! and reports them; it no longer records target freshness from them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ActionInputsPolicy, ActionSourceState, FieldProvenance};

/// The epoch that introduced `inputs_policy` and `source_state` declarations.
pub const TARGET_FRESHNESS_CONTRACT_VERSION: u32 = 8;
pub const WORKTREE_FRESHNESS_CONTRACT_VERSION: u32 = TARGET_FRESHNESS_CONTRACT_VERSION;

/// Which input-declaration semantics an inspected target's contract epoch
/// supports. The names are kept for wire compatibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetFreshnessPolicyModeV1 {
    /// Contract epochs before 8, which have no input declarations.
    LegacyGlobal,
    /// Contract epoch 8 and later, with `inputs_policy` and `source_state`.
    TargetFreshnessV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectedInputsPolicyV1 {
    pub effective: ActionInputsPolicy,
    pub defaulted: bool,
    pub provenance: Option<FieldProvenance>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectedSourceStateV1 {
    pub effective: ActionSourceState,
    pub defaulted: bool,
    pub provenance: Option<FieldProvenance>,
}

/// Effective input declarations exposed by catalog inspection.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetFreshnessPolicyInspectionV1 {
    pub contract_epoch: u32,
    pub mode: TargetFreshnessPolicyModeV1,
    pub inputs_policy: InspectedInputsPolicyV1,
    pub source_state: InspectedSourceStateV1,
}
