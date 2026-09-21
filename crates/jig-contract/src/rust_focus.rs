//! Versioned, portable Rust execution scope. No value is a shell fragment.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CargoImpactContextV1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustNextestConfigV1 {
    pub workspace_manifest: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub context: CargoImpactContextV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cargo_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nextest_profile: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustFeaturesV1 {
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub no_default_features: bool,
    #[serde(default)]
    pub all_features: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RustTargetV1 {
    Lib {},
    Bin { name: String },
    Test { name: String },
    Example { name: String },
    Bench { name: String },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RustFocusV1 {
    Automatic {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_id: Option<String>,
    },
    Explicit {
        packages: Vec<String>,
        #[serde(default)]
        targets: Vec<RustTargetV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        features: Option<RustFeaturesV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustScopeDispositionV1 {
    Full,
    Narrowed,
    BroadFallback,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedRustInputV1 {
    pub schema_version: u32,
    pub disposition: RustScopeDispositionV1,
    pub reasons: Vec<String>,
    pub packages: Vec<String>,
    pub targets: Vec<RustTargetV1>,
    pub context: CargoImpactContextV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_base: Option<String>,
    /// Literal arguments for the fixed `cargo` program, re-derived before execution.
    pub args: Vec<String>,
}

#[cfg(test)]
mod tests;
