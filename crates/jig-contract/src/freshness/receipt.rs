use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{
    FreshnessReason, MAX_FRESHNESS_DIAGNOSTIC_BYTES, MAX_FRESHNESS_REASON_PREVIEWS,
    TargetIdentityV1,
};
use crate::{RunConclusion, TargetId};

/// An unknown metadata version stays readable and round-trips unchanged. A
/// malformed object claiming the known version follows the journal's ordinary
/// deserialization error policy, rather than becoming a partial positive proof.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq)]
#[schemars(untagged)]
pub enum TargetFreshnessMetadata {
    V1(Box<TargetFreshnessV1>),
    Unsupported(Value),
}

impl Serialize for TargetFreshnessMetadata {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::V1(value) => value.serialize(serializer),
            Self::Unsupported(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for TargetFreshnessMetadata {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.get("schema_version").and_then(Value::as_u64) == Some(1) {
            serde_json::from_value(value)
                .map(|value| Self::V1(Box::new(value)))
                .map_err(serde::de::Error::custom)
        } else {
            Ok(Self::Unsupported(value))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct TargetFreshnessV1 {
    pub schema_version: u32,
    pub contract_epoch: u32,
    pub effective_valid_until_ms: Option<u64>,
    pub effective_requires_time_validity: bool,
    pub global_execution_proof: GlobalExecutionProofV1,
    #[serde(flatten)]
    pub state: TargetFreshnessStateV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TargetFreshnessStateV1 {
    Complete {
        identity: Box<TargetIdentityV1>,
        dependency_execution_proof: Vec<DependencyExecutionProofV1>,
    },
    Incomplete {
        #[serde(flatten)]
        reasons: FreshnessReasons,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GlobalExecutionProofV1 {
    Unchanged {
        before_source_digest: String,
        after_source_digest: String,
    },
    Mutated,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DependencyExecutionProofV1 {
    pub target: TargetId,
    pub receipt_id: String,
    pub run_id: String,
    pub plan_id: String,
    pub identity_digest: String,
    pub conclusion: RunConclusion,
    pub effective_valid_until_ms: Option<u64>,
    pub effective_requires_time_validity: bool,
}

/// Diagnostic limits apply to encoded UTF-8 bytes, not characters. Authority
/// arrays are separate and may never be truncated to fit this preview budget.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FreshnessReasons {
    pub reasons: Vec<FreshnessReason>,
    /// Diagnostic occurrences, including duplicates; the preview is deduplicated.
    pub reasons_total: u64,
    pub reasons_truncated: bool,
}

impl FreshnessReasons {
    pub fn one(reason: FreshnessReason) -> Self {
        let mut reasons = Self::default();
        reasons.push(reason);
        reasons
    }

    pub fn push(&mut self, reason: FreshnessReason) {
        self.reasons_total = self.reasons_total.saturating_add(1);
        if self.reasons.contains(&reason) {
            return;
        }
        if self.reasons.len() >= MAX_FRESHNESS_REASON_PREVIEWS || self.reasons_truncated {
            self.reasons_truncated = true;
            return;
        }
        self.reasons.push(reason);
        if serde_json::to_vec(&self.reasons)
            .expect("freshness reasons are JSON values")
            .len()
            > MAX_FRESHNESS_DIAGNOSTIC_BYTES
        {
            self.reasons.pop();
            self.reasons_truncated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::freshness::FreshnessReasonCode;
    use serde_json::json;

    #[test]
    fn reason_occurrences_are_counted_consistently_before_and_after_truncation() {
        let retained = FreshnessReason {
            code: FreshnessReasonCode::CollectionLimit,
            target: None,
            path: None,
        };
        let mut reasons = FreshnessReasons::default();
        reasons.push(retained.clone());
        reasons.push(retained.clone());
        assert_eq!(reasons.reasons_total, 2);
        assert_eq!(reasons.reasons, vec![retained]);
        assert!(!reasons.reasons_truncated);
        let omitted = FreshnessReason {
            code: FreshnessReasonCode::DirectInputChanged,
            target: None,
            path: Some("x".repeat(MAX_FRESHNESS_DIAGNOSTIC_BYTES)),
        };
        reasons.push(omitted.clone());
        reasons.push(omitted);
        assert_eq!(reasons.reasons_total, 4);
        assert_eq!(reasons.reasons.len(), 1);
        assert!(reasons.reasons_truncated);
        assert_eq!(serde_json::to_value(&reasons).unwrap()["reasons_total"], 4);
    }

    #[test]
    fn unknown_metadata_round_trips_without_inventing_v1_authority() {
        let raw = json!({"schema_version": 2, "state": "future", "new_proof": [1, 2, 3]});
        let parsed: TargetFreshnessMetadata = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(parsed, TargetFreshnessMetadata::Unsupported(_)));
        assert_eq!(serde_json::to_value(parsed).unwrap(), raw);
        assert!(
            serde_json::from_value::<TargetFreshnessMetadata>(json!({"schema_version":1})).is_err()
        );
    }

    #[test]
    fn incomplete_metadata_has_no_identity_and_unicode_reasons_stay_bounded() {
        let mut reasons = FreshnessReasons::default();
        for index in 0..110 {
            reasons.push(FreshnessReason {
                code: FreshnessReasonCode::DirectInputChanged,
                target: Some("web:test".parse().unwrap()),
                path: Some(format!("apps/web/{index}-{}.ts", "é".repeat(200))),
            });
        }
        assert_eq!(reasons.reasons_total, 110);
        assert!(reasons.reasons_truncated);
        assert!(serde_json::to_vec(&reasons.reasons).unwrap().len() <= 4_000);
        let metadata = TargetFreshnessMetadata::V1(Box::new(TargetFreshnessV1 {
            schema_version: 1,
            contract_epoch: 9,
            effective_valid_until_ms: None,
            effective_requires_time_validity: false,
            global_execution_proof: GlobalExecutionProofV1::Unknown,
            state: TargetFreshnessStateV1::Incomplete { reasons },
        }));
        let raw = serde_json::to_value(&metadata).unwrap();
        assert_eq!(raw["state"], "incomplete");
        assert!(raw.get("identity").is_none());
        assert_eq!(
            serde_json::from_value::<TargetFreshnessMetadata>(raw).unwrap(),
            metadata
        );
    }
}
