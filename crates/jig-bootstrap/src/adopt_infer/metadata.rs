use std::collections::BTreeMap;

use serde_json::{Value as JsonValue, json};

#[derive(Clone, Copy, Debug)]
pub(super) enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug)]
pub(super) struct InferenceMetadata {
    pub(super) value: JsonValue,
    pub(super) sources: Vec<String>,
    pub(super) confidence: Confidence,
    pub(super) warnings: Vec<String>,
}

impl Confidence {
    pub(super) fn from_str(value: &str) -> Self {
        match value {
            "high" => Self::High,
            "medium" => Self::Medium,
            "low" => Self::Low,
            _ => {
                debug_assert!(
                    false,
                    "unknown inference confidence {value}; defaulting to medium"
                );
                Self::Medium
            }
        }
    }

    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

pub(super) fn report(metadata: &BTreeMap<String, InferenceMetadata>) -> JsonValue {
    JsonValue::Object(
        metadata
            .iter()
            .map(|(key, metadata)| {
                (
                    key.clone(),
                    json!({
                        "value": metadata.value.clone(),
                        "sources": metadata.sources.clone(),
                        "confidence": metadata.confidence.as_str(),
                        "warnings": metadata.warnings.clone(),
                    }),
                )
            })
            .collect(),
    )
}
