//! Additive target authority for the conservative freshness epoch. These are
//! equality tokens, not artifact cache keys or external-state attestations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ActionInputsPolicy, TargetId};

pub const TARGET_FRESHNESS_CONTRACT_VERSION: u32 = 9;
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

/// Counters are observations, never identity authority. In particular elapsed
/// time and absolute checkout paths must not affect a plan's equality token.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct FreshnessCollectionStats {
    pub elapsed_us: u64,
    pub git_us: u64,
    pub committed_us: u64,
    pub index_us: u64,
    pub worktree_us: u64,
    pub discovered_entries: u64,
    pub content_bytes_read: u64,
    pub targets: u64,
    pub dependency_edges: u64,
    pub timeout_ms: u64,
}
