//! Additive target authority for the conservative freshness epoch. These are
//! equality tokens, not artifact cache keys or external-state attestations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ActionInputsPolicy, ActionSourceState, FieldProvenance, TargetId};

mod evaluation;
mod receipt;
pub use evaluation::{EffectiveTimeValidityV1, FreshnessDetailsV1, FreshnessSummaryV1};
pub use receipt::{
    DependencyExecutionProofV1, FreshnessReasons, GlobalExecutionProofV1, TargetFreshnessMetadata,
    TargetFreshnessStateV1, TargetFreshnessV1,
};

/// The 0.4.0 contract cutover ships target freshness and source-state
/// authority together.  The names distinguish the feature gates, not epochs.
pub const TARGET_FRESHNESS_CONTRACT_VERSION: u32 = 8;
pub const WORKTREE_FRESHNESS_CONTRACT_VERSION: u32 = TARGET_FRESHNESS_CONTRACT_VERSION;

/// The original unreleased receipt format added `source_state` at epoch 10.
/// Consolidated epoch 8 also writes it, while authentic epoch-9 receipts do not.
pub const SOURCE_STATE_IDENTITY_CONTRACT_VERSION: u32 = 10;

pub const fn supported_freshness_epoch(epoch: u32) -> bool {
    // v9 and v10 were never released. Keep their already-recorded local
    // receipts readable after the 0.4.0 v8 consolidation. Repository contract
    // v11 is reserved for durable work links, and its receipts retain the v8
    // freshness contract with source-state authority.
    matches!(epoch, TARGET_FRESHNESS_CONTRACT_VERSION..=11)
}

pub const fn freshness_identity_includes_source_state(epoch: u32) -> bool {
    epoch == WORKTREE_FRESHNESS_CONTRACT_VERSION || epoch >= SOURCE_STATE_IDENTITY_CONTRACT_VERSION
}

pub const TARGET_IDENTITY_SCHEMA_VERSION: u32 = 1;
pub const TARGET_IDENTITY_DOMAIN: &str = "jig-target-identity-v1";
pub const MAX_FRESHNESS_REASON_PREVIEWS: usize = 100;
pub const MAX_FRESHNESS_DIAGNOSTIC_BYTES: usize = 4_000;

/// The receipt-reuse semantics available to an inspected target's contract
/// epoch. This reports configured policy only; it does not evaluate a receipt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetFreshnessPolicyModeV1 {
    /// Contract epochs before target-scoped freshness use their conservative
    /// global evidence rules.
    LegacyGlobal,
    /// The target-scoped freshness policy introduced in contract epoch 8.
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

/// Effective target policy exposed by catalog inspection. The policy explains
/// which authority a later evidence evaluation would use; it never claims that
/// any particular receipt is currently fresh.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetFreshnessPolicyInspectionV1 {
    pub contract_epoch: u32,
    pub mode: TargetFreshnessPolicyModeV1,
    pub inputs_policy: InspectedInputsPolicyV1,
    pub source_state: InspectedSourceStateV1,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_link_repository_receipts_retain_supported_freshness_authority() {
        for epoch in 8..=11 {
            assert!(supported_freshness_epoch(epoch));
        }
        assert!(!supported_freshness_epoch(7));
        assert!(!supported_freshness_epoch(12));
        assert!(freshness_identity_includes_source_state(11));
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessReasonCode {
    DirectInputChanged,
    GitIdentityChanged,
    SourceChanged,
    DependencyChanged,
    RunnerChanged,
    ConfigurationChanged,
    InvocationChanged,
    TimeExpired,
    TimeBoundaryMissing,
    LegacyMetadata,
    CollectionFailed,
    CollectionLimit,
    SourceRaced,
    ExecutionMutated,
    UnobservableInput,
    AuthorityVersionChanged,
    DependencyProofMissing,
    DependencyProofInvalid,
    UnsupportedAuthority,
    UnsupportedReference,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FreshnessReason {
    pub code: FreshnessReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetFreshnessStatus {
    Fresh,
    Unknown,
    Stale,
    Missing,
    Unsupported,
}

impl TargetFreshnessStatus {
    pub const fn combine(self, other: Self) -> Self {
        if self.precedence() >= other.precedence() {
            self
        } else {
            other
        }
    }

    const fn precedence(self) -> u8 {
        match self {
            Self::Fresh => 0,
            Self::Unknown => 1,
            Self::Stale => 2,
            Self::Missing => 3,
            Self::Unsupported => 4,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct IdentityComponents {
    pub contract_epoch: u32,
    pub schema_version: u32,
    pub digest_domain: String,
    pub inputs_policy: ActionInputsPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_state: Option<ActionSourceState>,
    pub source_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_content_digest: Option<String>,
    pub authority_digest: String,
    pub dependency_digest: String,
    pub identity_digest: String,
    pub configuration_digest: String,
    pub runner_digest: String,
    pub invocation_digest: String,
}

impl From<&TargetIdentityV1> for IdentityComponents {
    fn from(identity: &TargetIdentityV1) -> Self {
        Self {
            contract_epoch: identity.contract_epoch,
            schema_version: identity.schema_version,
            digest_domain: identity.digest_domain.clone(),
            inputs_policy: identity.inputs_policy,
            source_state: identity.source_state,
            source_digest: identity.source_digest.clone(),
            source_content_digest: identity.source_content_digest.clone(),
            authority_digest: identity.authority_digest.clone(),
            dependency_digest: identity.dependency_digest.clone(),
            identity_digest: identity.identity_digest.clone(),
            configuration_digest: identity.configuration_digest.clone(),
            runner_digest: identity.runner_digest.clone(),
            invocation_digest: identity.invocation_digest.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct TargetFreshness {
    pub status: TargetFreshnessStatus,
    #[serde(flatten)]
    pub reasons: FreshnessReasons,
    pub recorded_identity: Option<IdentityComponents>,
    pub current_identity: Option<IdentityComponents>,
    pub effective_valid_until_ms: Option<u64>,
    pub effective_requires_time_validity: bool,
}

/// A preview of comparable direct-source authority. No file contents, dotenv
/// values, or process environments belong in this metadata.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SourceIdentityPreview {
    pub path: String,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct DependencyIdentity {
    pub target: TargetId,
    pub identity_digest: String,
}

/// Complete, independently versioned target identity. Comparison must check
/// epoch, schema, and domain before treating any digest as comparable.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct TargetIdentityV1 {
    pub contract_epoch: u32,
    pub schema_version: u32,
    pub digest_domain: String,
    pub target: TargetId,
    pub inputs_policy: ActionInputsPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_state: Option<ActionSourceState>,
    pub source_digest: String,
    /// Diagnostic authority without HEAD/branch; never replaces source_digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_content_digest: Option<String>,
    pub authority_digest: String,
    pub dependency_digest: String,
    pub identity_digest: String,
    pub configuration_digest: String,
    pub runner_digest: String,
    pub invocation_digest: String,
    pub source_preview: Vec<SourceIdentityPreview>,
    pub source_entry_count: u64,
    pub source_preview_truncated: bool,
    pub dependencies: Vec<DependencyIdentity>,
}

/// The actual collection failure, independent of elapsed-time heuristics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessCollectionLimit {
    Deadline,
    Resource,
}

/// Counters are observations, never identity authority. In particular elapsed
/// time and absolute checkout paths must not affect a plan's equality token.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FreshnessCollectionStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<FreshnessCollectionLimit>,
    pub elapsed_us: u64,
    pub git_us: u64,
    pub committed_us: u64,
    pub index_us: u64,
    pub worktree_us: u64,
    #[serde(default)]
    pub matching_us: u64,
    #[serde(default)]
    pub identity_us: u64,
    #[serde(default)]
    pub proof_us: u64,
    #[serde(default)]
    pub path_revalidation_us: u64,
    pub discovered_entries: u64,
    pub content_bytes_read: u64,
    pub targets: u64,
    pub dependency_edges: u64,
    pub timeout_ms: u64,
}
