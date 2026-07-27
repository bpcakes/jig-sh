//! Public, language-neutral status-provider protocol.
//!
//! A provider observes project-specific rewrite state and emits one versioned
//! report. Process execution and cross-provider aggregation intentionally live
//! above this crate.

/// Types and schema generation for `jig.status-provider/v1`.
pub mod v1;

/// Exact wire token for the first major status-provider protocol.
pub const V1_PROTOCOL: &str = "jig.status-provider/v1";

/// Stable identifier embedded in the committed JSON Schema for protocol v1.
pub const V1_SCHEMA_ID: &str = "https://raw.githubusercontent.com/bpcakes/jig-sh/master/crates/jig-contract/contracts/status-provider/v1.schema.json";
