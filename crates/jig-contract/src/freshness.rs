//! Additive target authority for the conservative freshness epoch. These are
//! equality tokens, not artifact cache keys or external-state attestations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ActionInputsPolicy, ActionSourceState, TargetId};

mod evaluation;
mod receipt;
pub use evaluation::{EffectiveTimeValidityV1, FreshnessDetailsV1, FreshnessSummaryV1};
pub use receipt::{
    DependencyExecutionProofV1, FreshnessReasons, GlobalExecutionProofV1, TargetFreshnessMetadata,
    TargetFreshnessStateV1, TargetFreshnessV1,
};

pub const TARGET_FRESHNESS_CONTRACT_VERSION: u32 = 9;
pub const WORKTREE_FRESHNESS_CONTRACT_VERSION: u32 = 10;

pub const fn supported_freshness_epoch(epoch: u32) -> bool {
    epoch >= TARGET_FRESHNESS_CONTRACT_VERSION && epoch <= WORKTREE_FRESHNESS_CONTRACT_VERSION
}
pub const TARGET_IDENTITY_SCHEMA_VERSION: u32 = 1;
pub const TARGET_IDENTITY_DOMAIN: &str = "jig-target-identity-v1";
pub const MAX_FRESHNESS_REASON_PREVIEWS: usize = 100;
pub const MAX_FRESHNESS_DIAGNOSTIC_BYTES: usize = 4_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessReasonCode {
    DirectInputChanged,
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
