//! Pure, bounded Cargo metadata normalization and conservative impact policy.
//!
//! This module intentionally stops at portable planning facts.  It does not
//! execute Cargo or inspect the filesystem. Opaque package identifiers remain
//! private, in-memory selector proofs and never enter portable plan evidence.

mod impact;
mod normalize;
mod raw;
mod selector_proof;

pub use impact::select_cargo_impact_v1;
pub use normalize::{
    CARGO_METADATA_FORMAT_VERSION_V1, CargoMetadataErrorV1, CargoMetadataGraphV1,
    CargoMetadataLimitsV1, CargoMetadataResourceV1, CargoPackageFactsV1, CargoTargetFactsV1,
    MAX_METADATA_BYTES_V1, MAX_METADATA_CHANGED_PATHS_V1, MAX_METADATA_EDGES_V1,
    MAX_METADATA_FEATURES_V1, MAX_METADATA_NODES_V1, MAX_METADATA_PACKAGES_V1,
    MAX_METADATA_PATH_BYTES_V1, MAX_METADATA_PATHS_V1, MAX_METADATA_ROOTS_V1,
    MAX_METADATA_STRING_BYTES_V1, MAX_METADATA_TARGETS_V1, normalize_cargo_metadata,
    normalize_cargo_metadata_v1,
};
