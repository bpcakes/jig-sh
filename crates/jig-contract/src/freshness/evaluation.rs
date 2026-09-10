use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{FreshnessReason, FreshnessReasons, IdentityComponents, TargetFreshness};

/// Evaluated validity, separate from a receipt's original native deadline.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct EffectiveTimeValidityV1 {
    pub effective_valid_until_ms: Option<u64>,
    pub effective_requires_time_validity: bool,
}

impl EffectiveTimeValidityV1 {
    pub fn new(boundary: Option<u64>, required: bool) -> Self {
        Self {
            effective_valid_until_ms: boundary,
            effective_requires_time_validity: required || boundary.is_some(),
        }
    }

    /// A missing required boundary cannot be repaired by another target's
    /// deadline. It remains unverifiable through every enclosing summary.
    pub fn combine(self, other: Self) -> Self {
        let required =
            self.effective_requires_time_validity || other.effective_requires_time_validity;
        let missing = (self.effective_requires_time_validity
            && self.effective_valid_until_ms.is_none())
            || (other.effective_requires_time_validity && other.effective_valid_until_ms.is_none());
        let boundary = if missing {
            None
        } else {
            [
                self.effective_valid_until_ms,
                other.effective_valid_until_ms,
            ]
            .into_iter()
            .flatten()
            .min()
        };
        Self::new(boundary, required)
    }
}

/// Additive evaluated fields shared by CLI/MCP summaries and dashboard DTOs.
/// Historical receipt validity fields keep their original meaning.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FreshnessSummaryV1 {
    pub freshness_reasons: Vec<FreshnessReason>,
    pub freshness_reasons_total: u64,
    pub freshness_reasons_truncated: bool,
    pub effective_valid_until_ms: Option<u64>,
    pub effective_requires_time_validity: bool,
}

impl FreshnessSummaryV1 {
    pub fn new(
        reasons: FreshnessReasons,
        valid_until_ms: Option<u64>,
        requires_time_validity: bool,
    ) -> Self {
        Self {
            freshness_reasons: reasons.reasons,
            freshness_reasons_total: reasons.reasons_total,
            freshness_reasons_truncated: reasons.reasons_truncated,
            effective_valid_until_ms: valid_until_ms,
            effective_requires_time_validity: requires_time_validity,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FreshnessDetailsV1 {
    #[serde(flatten)]
    pub summary: FreshnessSummaryV1,
    pub recorded_identity: Option<IdentityComponents>,
    pub current_identity: Option<IdentityComponents>,
}

impl From<&TargetFreshness> for FreshnessDetailsV1 {
    fn from(result: &TargetFreshness) -> Self {
        Self {
            summary: FreshnessSummaryV1::new(
                result.reasons.clone(),
                result.effective_valid_until_ms,
                result.effective_requires_time_validity,
            ),
            recorded_identity: result.recorded_identity.clone(),
            current_identity: result.current_identity.clone(),
        }
    }
}
