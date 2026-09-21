use std::collections::BTreeMap;

use serde::Deserialize;

/// Cargo metadata format-version 1 fields used by the bounded normalizer.
///
/// These types intentionally do not deny unknown fields: Cargo can add
/// metadata without making an otherwise usable graph unavailable.
#[derive(Debug, Deserialize)]
pub(crate) struct RawMetadata {
    pub version: u32,
    pub workspace_root: String,
    pub packages: Vec<RawPackage>,
    pub workspace_members: Vec<String>,
    pub resolve: Option<RawResolve>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawPackage {
    pub name: String,
    pub version: String,
    pub id: String,
    pub manifest_path: String,
    pub targets: Vec<RawTarget>,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawTarget {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: String,
    #[serde(default)]
    pub test: bool,
    #[serde(default)]
    pub doctest: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawResolve {
    pub nodes: Vec<RawNode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawNode {
    pub id: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub deps: Vec<RawDependency>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawDependency {
    pub name: String,
    pub pkg: String,
    #[serde(default)]
    pub dep_kinds: Vec<RawDependencyKind>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawDependencyKind {
    pub kind: Option<String>,
    pub target: Option<String>,
}
