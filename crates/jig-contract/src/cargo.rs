//! Portable Cargo impact facts carried by an immutable repository run plan.
//!
//! These types deliberately describe candidate build scope only. They do not
//! encode a Cargo command, a shell fragment, or a runtime test-name filter.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ComponentId;

/// The conservative outcome of Cargo package impact analysis.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CargoImpactDispositionV1 {
    Narrowed,
    BroadFallback,
    Unavailable,
}

/// Feature and target context under which Cargo metadata was resolved.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoImpactContextV1 {
    /// Cargo metadata format requested by the acquisition boundary.
    pub metadata_format_version: u32,
    /// Explicit target triple, when the discovery request supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Explicit feature selections carried by the discovery request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Whether default features were disabled for the discovery request.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_default_features: bool,
    /// Whether the request selected all features.
    #[serde(default, skip_serializing_if = "is_false")]
    pub all_features: bool,
    /// Whether Cargo metadata was acquired with `--locked`.
    pub locked: bool,
    /// Whether Cargo metadata was acquired with `--offline`.
    pub offline: bool,
}

impl Default for CargoImpactContextV1 {
    fn default() -> Self {
        Self {
            metadata_format_version: 1,
            target: None,
            features: Vec::new(),
            no_default_features: false,
            all_features: false,
            locked: true,
            offline: true,
        }
    }
}

/// A portable Cargo package identity and its normalized repository location.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoPackageImpactV1 {
    /// A validated, portable Cargo selector in `name@version` form.
    pub selector: String,
    /// Repository-relative package manifest path.
    pub manifest_path: String,
    /// Whether this package is a member of the acquired workspace.
    pub workspace_member: bool,
    /// Features reported active for this package by Cargo's resolve graph.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activated_features: Vec<String>,
}

/// A Cargo target that can participate in package test/build selection.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoTargetImpactV1 {
    /// The validated `name@version` selector of the owning package.
    pub package_selector: String,
    /// Cargo's target name.
    pub name: String,
    /// Cargo target kinds, such as `lib`, `bin`, or `test`.
    pub kinds: Vec<String>,
}

/// Why a Cargo impact result was broadened or could not be established.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CargoImpactReasonV1 {
    ChangedSource { path: String },
    ConsumerClosure { package_selector: String },
    TopologyChange { path: String },
    BuildScriptInput { path: String },
    UnownedPath { path: String },
    AmbiguousOwnership { path: String },
    InvalidChangedPath,
    EmptyChangedPathSet,
    NoWorkspacePackages,
    WorkspaceManifestMissing,
    UnsupportedWorkspaceRoot,
    CargoProgramUnavailable,
    MetadataCommandNonZero,
    MetadataProcessFailure,
    MetadataTimeout,
    MetadataOutputLimitExceeded,
    MetadataMalformed,
    MetadataResourceLimitExceeded,
    IncompleteResolve,
    MissingLocalDependency,
    DuplicateSelector,
    UnsupportedContext,
}

/// V03 deliberately makes runtime test-name filtering explicit and absent.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CargoRuntimeTestFilterV1 {
    Absent,
}

/// Portable package-impact evidence attached to a run plan.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CargoImpactV1 {
    /// Authored repository component for which the Cargo graph was acquired.
    pub component: ComponentId,
    /// Repository-relative workspace manifest used for discovery.
    pub workspace_manifest: String,
    pub context: CargoImpactContextV1,
    pub disposition: CargoImpactDispositionV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<CargoImpactReasonV1>,
    /// Packages that may need to be built for this impact.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_packages: Vec<CargoPackageImpactV1>,
    /// Test-enabled Cargo targets belonging to the selected build packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_targets: Vec<CargoTargetImpactV1>,
    /// V03 never infers a runtime test-name filter from package impact.
    pub runtime_test_filter: CargoRuntimeTestFilterV1,
}

impl CargoImpactV1 {
    /// Sort all repeated fields into their canonical order before persistence.
    pub fn sort_canonical(&mut self) {
        self.context.features.sort();
        self.context.features.dedup();
        self.reasons.sort();
        self.reasons.dedup();
        for package in &mut self.build_packages {
            package.activated_features.sort();
            package.activated_features.dedup();
        }
        self.build_packages.sort();
        self.build_packages.dedup();
        for target in &mut self.test_targets {
            target.kinds.sort();
            target.kinds.dedup();
        }
        self.test_targets.sort();
        self.test_targets.dedup();
    }

    /// Construct an unavailable result without carrying process output or raw
    /// Cargo identity into the portable plan.
    #[must_use]
    pub fn unavailable(
        component: ComponentId,
        workspace_manifest: impl Into<String>,
        context: CargoImpactContextV1,
        reason: CargoImpactReasonV1,
    ) -> Self {
        Self {
            component,
            workspace_manifest: workspace_manifest.into(),
            context,
            disposition: CargoImpactDispositionV1::Unavailable,
            reasons: vec![reason],
            build_packages: Vec::new(),
            test_targets: Vec::new(),
            runtime_test_filter: CargoRuntimeTestFilterV1::Absent,
        }
    }
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_impact_canonical_sort_is_stable_and_deduplicates_portable_facts() {
        let mut impact = CargoImpactV1 {
            component: ComponentId::parse("rust").unwrap(),
            workspace_manifest: "Cargo.toml".to_owned(),
            context: CargoImpactContextV1 {
                features: vec!["z".to_owned(), "a".to_owned(), "a".to_owned()],
                ..CargoImpactContextV1::default()
            },
            disposition: CargoImpactDispositionV1::Narrowed,
            reasons: vec![
                CargoImpactReasonV1::EmptyChangedPathSet,
                CargoImpactReasonV1::EmptyChangedPathSet,
            ],
            build_packages: vec![CargoPackageImpactV1 {
                selector: "example@1.0.0".to_owned(),
                manifest_path: "crates/example/Cargo.toml".to_owned(),
                workspace_member: true,
                activated_features: vec!["z".to_owned(), "a".to_owned()],
            }],
            test_targets: vec![CargoTargetImpactV1 {
                package_selector: "example@1.0.0".to_owned(),
                name: "example".to_owned(),
                kinds: vec!["lib".to_owned()],
            }],
            runtime_test_filter: CargoRuntimeTestFilterV1::Absent,
        };
        let mut expected = impact.clone();
        impact
            .build_packages
            .push(expected.build_packages[0].clone());
        impact.test_targets.push(expected.test_targets[0].clone());
        impact.sort_canonical();
        expected.sort_canonical();
        assert_eq!(impact.context.features, ["a", "z"]);
        assert_eq!(impact.reasons.len(), 1);
        assert_eq!(impact.build_packages.len(), 1);
        assert_eq!(impact.test_targets.len(), 1);
        assert_eq!(impact, expected);
    }

    #[test]
    fn cargo_impact_context_serializes_acquisition_authority() {
        let value = serde_json::to_value(CargoImpactContextV1::default()).unwrap();
        assert_eq!(value["locked"], true);
        assert_eq!(value["offline"], true);

        let context = CargoImpactContextV1 {
            locked: false,
            offline: false,
            ..CargoImpactContextV1::default()
        };
        let value = serde_json::to_value(context).unwrap();
        assert_eq!(value["locked"], false);
        assert_eq!(value["offline"], false);
    }
}
