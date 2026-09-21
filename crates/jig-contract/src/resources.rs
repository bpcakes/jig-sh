//! Versioned, opt-in scheduling declarations, independent of evidence dependencies.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CargoImpactContextV1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionResourceV1 {
    CargoV1 {
        /// Repository-relative workspace manifest; never an absolute identity.
        workspace_manifest: String,
        /// Repository-relative directory where the wrapped Cargo process runs;
        /// it may differ from the wrapper's own launch directory.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(default)]
        context: CargoImpactContextV1,
    },
    /// Attests the generated Playwright environment contract; resolution owns
    /// local server endpoints only when no external base URL is selected.
    PlaywrightServersV1 {},
}

/// Original durable evidence reused without inventing a newly executed target.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReusedTargetEvidenceV1 {
    pub receipt_id: String,
    pub run_id: String,
    pub plan_id: String,
}

#[cfg(test)]
mod tests;
