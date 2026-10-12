use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::{Component, Path, PathBuf},
};

use serde_json::from_slice;

use super::raw::{RawMetadata, RawTarget};
use super::selector_proof::SelectorProof;

/// Cargo metadata format version implemented by this module.
pub const CARGO_METADATA_FORMAT_VERSION_V1: u32 = 1;

/// Upper bound on captured Cargo metadata before JSON parsing.
pub const MAX_METADATA_BYTES_V1: usize = 16 * 1024 * 1024;
/// Upper bound on package records in one graph.
pub const MAX_METADATA_PACKAGES_V1: usize = 512;
/// Upper bound on resolved node records in one graph.
pub const MAX_METADATA_NODES_V1: usize = 512;
/// Upper bound on resolved dependency records in one graph.
pub const MAX_METADATA_EDGES_V1: usize = 4_096;
/// Upper bound on Cargo target records in one graph.
pub const MAX_METADATA_TARGETS_V1: usize = 4_096;
/// Upper bound on feature entries and resolved feature selections.
pub const MAX_METADATA_FEATURES_V1: usize = 8_192;
/// Upper bound on workspace and local package roots.
pub const MAX_METADATA_ROOTS_V1: usize = 512;
/// Upper bound on path-bearing metadata fields.
pub const MAX_METADATA_PATHS_V1: usize = 8_192;
/// Upper bound on one portable path in bytes.
pub const MAX_METADATA_PATH_BYTES_V1: usize = 4_096;
/// Upper bound on one non-path Cargo string in bytes.
pub const MAX_METADATA_STRING_BYTES_V1: usize = 4_096;
/// Upper bound on changed paths supplied to impact selection.
pub const MAX_METADATA_CHANGED_PATHS_V1: usize = 4_096;

/// A caller-adjustable copy of the V03 graph limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CargoMetadataLimitsV1 {
    pub max_metadata_bytes: usize,
    pub max_packages: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_targets: usize,
    pub max_features: usize,
    pub max_roots: usize,
    pub max_paths: usize,
    pub max_path_bytes: usize,
    pub max_string_bytes: usize,
    pub max_changed_paths: usize,
}

impl Default for CargoMetadataLimitsV1 {
    fn default() -> Self {
        Self {
            max_metadata_bytes: MAX_METADATA_BYTES_V1,
            max_packages: MAX_METADATA_PACKAGES_V1,
            max_nodes: MAX_METADATA_NODES_V1,
            max_edges: MAX_METADATA_EDGES_V1,
            max_targets: MAX_METADATA_TARGETS_V1,
            max_features: MAX_METADATA_FEATURES_V1,
            max_roots: MAX_METADATA_ROOTS_V1,
            max_paths: MAX_METADATA_PATHS_V1,
            max_path_bytes: MAX_METADATA_PATH_BYTES_V1,
            max_string_bytes: MAX_METADATA_STRING_BYTES_V1,
            max_changed_paths: MAX_METADATA_CHANGED_PATHS_V1,
        }
    }
}

/// Safe categories used when reporting a bound failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoMetadataResourceV1 {
    MetadataBytes,
    Packages,
    Nodes,
    Edges,
    Targets,
    Features,
    Roots,
    Paths,
    PathBytes,
    StringBytes,
    ChangedPaths,
}

impl fmt::Display for CargoMetadataResourceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::MetadataBytes => "metadata_bytes",
            Self::Packages => "packages",
            Self::Nodes => "nodes",
            Self::Edges => "edges",
            Self::Targets => "targets",
            Self::Features => "features",
            Self::Roots => "roots",
            Self::Paths => "paths",
            Self::PathBytes => "path_bytes",
            Self::StringBytes => "string_bytes",
            Self::ChangedPaths => "changed_paths",
        };
        formatter.write_str(name)
    }
}

/// Validation failures exposed by pure Cargo normalization.
///
/// No variant carries an opaque Cargo ID or a path supplied by Cargo.  This
/// keeps process diagnostics and absolute checkout paths out of public plan
/// evidence even when malformed metadata is encountered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoMetadataErrorV1 {
    MetadataTooLarge {
        limit: usize,
        observed: usize,
    },
    InvalidJson,
    UnsupportedFormat {
        expected: u32,
        observed: u32,
    },
    MissingField(&'static str),
    LimitExceeded {
        resource: CargoMetadataResourceV1,
        limit: usize,
        observed: usize,
    },
    InvalidString(&'static str),
    InvalidPath(&'static str),
    InvalidRepositoryRoot,
    DuplicatePackageId,
    DuplicateResolveNode,
    DuplicateWorkspaceMember,
    DuplicateSelector,
    UnknownPackage,
    UnknownWorkspaceMember,
    MissingResolveNode,
    IncompleteResolve,
    MissingDependencyKinds,
}

impl fmt::Display for CargoMetadataErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MetadataTooLarge { limit, observed } => {
                write!(
                    formatter,
                    "Cargo metadata exceeds {limit} bytes ({observed})"
                )
            }
            Self::InvalidJson => formatter.write_str("Cargo metadata is not valid JSON"),
            Self::UnsupportedFormat { expected, observed } => write!(
                formatter,
                "Cargo metadata format {observed} is unsupported; expected {expected}"
            ),
            Self::MissingField(field) => write!(formatter, "Cargo metadata is missing {field}"),
            Self::LimitExceeded {
                resource,
                limit,
                observed,
            } => write!(
                formatter,
                "Cargo metadata {resource} limit {limit} exceeded by {observed}"
            ),
            Self::InvalidString(field) => {
                write!(formatter, "Cargo metadata has an invalid {field}")
            }
            Self::InvalidPath(field) => {
                write!(formatter, "Cargo metadata has an invalid {field} path")
            }
            Self::InvalidRepositoryRoot => formatter.write_str("repository root must be absolute"),
            Self::DuplicatePackageId => formatter.write_str("Cargo metadata repeats a package id"),
            Self::DuplicateResolveNode => formatter.write_str("Cargo resolve repeats a node"),
            Self::DuplicateWorkspaceMember => {
                formatter.write_str("Cargo metadata repeats a workspace member")
            }
            Self::DuplicateSelector => formatter.write_str("Cargo package selectors are ambiguous"),
            Self::UnknownPackage => {
                formatter.write_str("Cargo resolve references an unknown package")
            }
            Self::UnknownWorkspaceMember => {
                formatter.write_str("Cargo workspace member is not a local package")
            }
            Self::MissingResolveNode => formatter.write_str("Cargo resolve omits a package node"),
            Self::IncompleteResolve => formatter.write_str("Cargo resolve edges are incomplete"),
            Self::MissingDependencyKinds => {
                formatter.write_str("Cargo resolve omits dependency kinds")
            }
        }
    }
}

impl Error for CargoMetadataErrorV1 {}

/// One normalized Cargo target.  Paths are repository-relative.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoTargetFactsV1 {
    pub name: String,
    pub kinds: Vec<String>,
    pub src_path: String,
    pub test: bool,
    pub doctest: bool,
}

/// One normalized package.  Opaque package IDs are intentionally absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoPackageFactsV1 {
    pub selector: String,
    pub manifest_path: String,
    pub package_root: String,
    pub workspace_member: bool,
    pub activated_features: Vec<String>,
    pub targets: Vec<CargoTargetFactsV1>,
}

/// A bounded, portable Cargo resolve graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoMetadataGraphV1 {
    pub(crate) workspace_root: String,
    pub(crate) packages: Vec<CargoPackageFactsV1>,
    pub(crate) workspace_members: Vec<usize>,
    pub(crate) reverse_consumers: Vec<Vec<usize>>,
    selector_proof: SelectorProof,
}

impl CargoMetadataGraphV1 {
    /// Prove a portable selector names exactly one normalized workspace member
    /// backed by one opaque Cargo package ID. Raw IDs never leave the graph.
    #[must_use]
    pub fn verifies_workspace_selector(&self, selector: &str) -> bool {
        let mut matches = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, package)| package.selector == selector);
        let Some((index, package)) = matches.next() else {
            return false;
        };
        matches.next().is_none()
            && package.workspace_member
            && self.workspace_members.contains(&index)
            && self.selector_proof.verifies_index(index)
    }

    #[must_use]
    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    #[must_use]
    pub fn packages(&self) -> &[CargoPackageFactsV1] {
        &self.packages
    }

    pub fn workspace_members(&self) -> impl Iterator<Item = &CargoPackageFactsV1> {
        self.workspace_members
            .iter()
            .filter_map(|index| self.packages.get(*index))
    }

    #[must_use]
    pub fn reverse_consumers(&self, package_selector: &str) -> Option<Vec<&CargoPackageFactsV1>> {
        let index = self
            .packages
            .iter()
            .position(|package| package.selector == package_selector)?;
        Some(
            self.reverse_consumers[index]
                .iter()
                .filter_map(|consumer| self.packages.get(*consumer))
                .collect(),
        )
    }
}

#[derive(Default)]
struct Counters {
    paths: usize,
    targets: usize,
    features: usize,
    roots: usize,
    edges: usize,
}

/// Normalize Cargo's format-version-1 JSON into portable facts.
pub fn normalize_cargo_metadata_v1(
    bytes: &[u8],
    repository_root: &Path,
    limits: CargoMetadataLimitsV1,
) -> Result<CargoMetadataGraphV1, CargoMetadataErrorV1> {
    if !repository_root.is_absolute() {
        return Err(CargoMetadataErrorV1::InvalidRepositoryRoot);
    }
    if bytes.len() > limits.max_metadata_bytes {
        return Err(CargoMetadataErrorV1::MetadataTooLarge {
            limit: limits.max_metadata_bytes,
            observed: bytes.len(),
        });
    }
    let metadata: RawMetadata = from_slice(bytes).map_err(|_| CargoMetadataErrorV1::InvalidJson)?;
    if metadata.version != CARGO_METADATA_FORMAT_VERSION_V1 {
        return Err(CargoMetadataErrorV1::UnsupportedFormat {
            expected: CARGO_METADATA_FORMAT_VERSION_V1,
            observed: metadata.version,
        });
    }
    if metadata.packages.is_empty() {
        return Err(CargoMetadataErrorV1::MissingField("packages"));
    }
    ensure_count(
        CargoMetadataResourceV1::Packages,
        limits.max_packages,
        metadata.packages.len(),
    )?;
    let Some(resolve) = metadata.resolve.as_ref() else {
        return Err(CargoMetadataErrorV1::MissingField("resolve"));
    };
    ensure_count(
        CargoMetadataResourceV1::Nodes,
        limits.max_nodes,
        resolve.nodes.len(),
    )?;

    let mut counters = Counters::default();
    let workspace_root = normalize_path(
        &metadata.workspace_root,
        repository_root,
        &limits,
        &mut counters,
        "workspace_root",
    )?;
    counters.roots = 1;

    let mut id_to_index = BTreeMap::new();
    let mut selector_set = BTreeSet::new();
    let mut packages = Vec::with_capacity(metadata.packages.len());
    for package in &metadata.packages {
        check_string(&package.id, &limits, "package id")?;
        if package.id.is_empty() || id_to_index.contains_key(&package.id) {
            return Err(if package.id.is_empty() {
                CargoMetadataErrorV1::InvalidString("package id")
            } else {
                CargoMetadataErrorV1::DuplicatePackageId
            });
        }
        check_string(&package.name, &limits, "package name")?;
        check_string(&package.version, &limits, "package version")?;
        check_string(&package.manifest_path, &limits, "manifest")?;
        let selector = package_selector(&package.name, &package.version)?;
        if !selector_set.insert(selector.clone()) {
            return Err(CargoMetadataErrorV1::DuplicateSelector);
        }
        let manifest_path = normalize_package_manifest(
            &package.manifest_path,
            repository_root,
            &limits,
            &mut counters,
        )?;
        let local = !manifest_path.is_empty();
        let package_root = if local {
            package_root(&manifest_path)
        } else {
            String::new()
        };
        counters.roots = counters.roots.saturating_add(1);
        ensure_count(
            CargoMetadataResourceV1::Roots,
            limits.max_roots,
            counters.roots,
        )?;
        let targets = normalize_targets(
            &package.targets,
            repository_root,
            local,
            &limits,
            &mut counters,
        )?;
        normalize_features(&package.features, &limits, &mut counters)?;
        let index = packages.len();
        id_to_index.insert(package.id.clone(), index);
        packages.push(CargoPackageFactsV1 {
            selector,
            manifest_path,
            package_root,
            workspace_member: false,
            activated_features: Vec::new(),
            targets,
        });
    }

    let mut workspace_members = Vec::with_capacity(metadata.workspace_members.len());
    let mut workspace_member_set = BTreeSet::new();
    ensure_count(
        CargoMetadataResourceV1::Roots,
        limits.max_roots,
        metadata.workspace_members.len().saturating_add(1),
    )?;
    for member_id in &metadata.workspace_members {
        check_string(member_id, &limits, "workspace member")?;
        let Some(index) = id_to_index.get(member_id).copied() else {
            return Err(CargoMetadataErrorV1::UnknownWorkspaceMember);
        };
        if !workspace_member_set.insert(index) {
            return Err(CargoMetadataErrorV1::DuplicateWorkspaceMember);
        }
        if packages[index].manifest_path.is_empty() {
            return Err(CargoMetadataErrorV1::UnknownWorkspaceMember);
        }
        packages[index].workspace_member = true;
        workspace_members.push(index);
    }
    if workspace_members.is_empty() {
        return Err(CargoMetadataErrorV1::MissingField("workspace_members"));
    }

    let mut node_set = BTreeSet::new();
    let mut node_dependencies: Vec<Vec<usize>> = vec![Vec::new(); packages.len()];
    for node in &resolve.nodes {
        check_string(&node.id, &limits, "resolve node")?;
        let Some(&source) = id_to_index.get(&node.id) else {
            return Err(CargoMetadataErrorV1::UnknownPackage);
        };
        if !node_set.insert(source) {
            return Err(CargoMetadataErrorV1::DuplicateResolveNode);
        }
        let mut listed_dependencies = BTreeSet::new();
        for dependency_id in &node.dependencies {
            check_string(dependency_id, &limits, "dependency")?;
            let Some(&target) = id_to_index.get(dependency_id) else {
                return Err(CargoMetadataErrorV1::UnknownPackage);
            };
            if !listed_dependencies.insert(target) {
                return Err(CargoMetadataErrorV1::IncompleteResolve);
            }
        }
        let mut resolved_dependencies = BTreeSet::new();
        for dependency in &node.deps {
            check_string(&dependency.name, &limits, "dependency name")?;
            check_string(&dependency.pkg, &limits, "dependency")?;
            let Some(&target) = id_to_index.get(&dependency.pkg) else {
                return Err(CargoMetadataErrorV1::UnknownPackage);
            };
            if dependency.dep_kinds.is_empty() {
                return Err(CargoMetadataErrorV1::MissingDependencyKinds);
            }
            for dependency_kind in &dependency.dep_kinds {
                if let Some(kind) = dependency_kind.kind.as_deref() {
                    check_string(kind, &limits, "dependency kind")?;
                }
                if let Some(target_triple) = dependency_kind.target.as_deref() {
                    check_string(target_triple, &limits, "dependency target")?;
                }
            }
            if !resolved_dependencies.insert(target) {
                return Err(CargoMetadataErrorV1::IncompleteResolve);
            }
            counters.edges = counters.edges.saturating_add(1);
            ensure_count(
                CargoMetadataResourceV1::Edges,
                limits.max_edges,
                counters.edges,
            )?;
            node_dependencies[source].push(target);
        }
        if listed_dependencies != resolved_dependencies {
            return Err(CargoMetadataErrorV1::IncompleteResolve);
        }
        if !node.features.is_empty() {
            counters.features = counters.features.saturating_add(node.features.len());
            ensure_count(
                CargoMetadataResourceV1::Features,
                limits.max_features,
                counters.features,
            )?;
        }
        for feature in &node.features {
            check_string(feature, &limits, "resolved feature")?;
        }
        node_dependencies[source].sort_unstable();
        node_dependencies[source].dedup();
        packages[source].activated_features = node.features.clone();
        packages[source].activated_features.sort();
        packages[source].activated_features.dedup();
    }
    if node_set.len() != packages.len() {
        return Err(CargoMetadataErrorV1::MissingResolveNode);
    }

    let mut reverse_consumers = vec![Vec::new(); packages.len()];
    for (source, dependencies) in node_dependencies.iter().enumerate() {
        for &target in dependencies {
            reverse_consumers[target].push(source);
        }
    }
    for consumers in &mut reverse_consumers {
        consumers.sort_unstable();
        consumers.dedup();
    }
    workspace_members.sort_unstable();
    Ok(CargoMetadataGraphV1 {
        workspace_root,
        packages,
        workspace_members,
        reverse_consumers,
        selector_proof: SelectorProof::new(id_to_index),
    })
}

/// Alias with the short name used by callers that already know the format.
pub fn normalize_cargo_metadata(
    bytes: &[u8],
    repository_root: &Path,
    limits: CargoMetadataLimitsV1,
) -> Result<CargoMetadataGraphV1, CargoMetadataErrorV1> {
    normalize_cargo_metadata_v1(bytes, repository_root, limits)
}

fn normalize_targets(
    raw_targets: &[RawTarget],
    repository_root: &Path,
    local: bool,
    limits: &CargoMetadataLimitsV1,
    counters: &mut Counters,
) -> Result<Vec<CargoTargetFactsV1>, CargoMetadataErrorV1> {
    let mut targets = Vec::with_capacity(raw_targets.len());
    for target in raw_targets {
        ensure_count(
            CargoMetadataResourceV1::Targets,
            limits.max_targets,
            counters.targets.saturating_add(1),
        )?;
        counters.targets = counters.targets.saturating_add(1);
        check_string(&target.name, limits, "target name")?;
        if target.name.is_empty() {
            return Err(CargoMetadataErrorV1::InvalidString("target name"));
        }
        if target.kind.is_empty() {
            return Err(CargoMetadataErrorV1::InvalidString("target kind"));
        }
        let mut kinds = target.kind.clone();
        for kind in &kinds {
            check_string(kind, limits, "target kind")?;
        }
        kinds.sort();
        kinds.dedup();
        let src_path = if local {
            normalize_path(
                &target.src_path,
                repository_root,
                limits,
                counters,
                "target source",
            )?
        } else {
            discard_external_path(&target.src_path, limits, counters, "target source")?;
            String::new()
        };
        targets.push(CargoTargetFactsV1 {
            name: target.name.clone(),
            kinds,
            src_path,
            test: target.test,
            doctest: target.doctest,
        });
    }
    targets.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.kinds.cmp(&right.kinds))
            .then_with(|| left.src_path.cmp(&right.src_path))
    });
    Ok(targets)
}

fn normalize_package_manifest(
    raw: &str,
    repository_root: &Path,
    limits: &CargoMetadataLimitsV1,
    counters: &mut Counters,
) -> Result<String, CargoMetadataErrorV1> {
    check_string(raw, limits, "manifest")?;
    check_path_bytes(raw, limits)?;
    if raw.starts_with("file:") || raw.contains('\\') {
        return Err(CargoMetadataErrorV1::InvalidPath("manifest"));
    }
    let candidate = Path::new(raw);
    if candidate.is_absolute() && candidate.strip_prefix(repository_root).is_err() {
        if candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(CargoMetadataErrorV1::InvalidPath("manifest"));
        }
        counters.paths = counters.paths.saturating_add(1);
        ensure_count(
            CargoMetadataResourceV1::Paths,
            limits.max_paths,
            counters.paths,
        )?;
        return Ok(String::new());
    }
    normalize_path(raw, repository_root, limits, counters, "manifest")
}

fn discard_external_path(
    raw: &str,
    limits: &CargoMetadataLimitsV1,
    counters: &mut Counters,
    field: &'static str,
) -> Result<(), CargoMetadataErrorV1> {
    check_string(raw, limits, field)?;
    check_path_bytes(raw, limits)?;
    if raw.starts_with("file:") || raw.contains('\\') {
        return Err(CargoMetadataErrorV1::InvalidPath(field));
    }
    if Path::new(raw)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CargoMetadataErrorV1::InvalidPath(field));
    }
    counters.paths = counters.paths.saturating_add(1);
    ensure_count(
        CargoMetadataResourceV1::Paths,
        limits.max_paths,
        counters.paths,
    )
}

fn normalize_features(
    features: &BTreeMap<String, Vec<String>>,
    limits: &CargoMetadataLimitsV1,
    counters: &mut Counters,
) -> Result<(), CargoMetadataErrorV1> {
    for (feature, expansions) in features {
        check_string(feature, limits, "feature")?;
        counters.features = counters.features.saturating_add(1);
        ensure_count(
            CargoMetadataResourceV1::Features,
            limits.max_features,
            counters.features,
        )?;
        for expansion in expansions {
            check_string(expansion, limits, "feature expansion")?;
            counters.features = counters.features.saturating_add(1);
            ensure_count(
                CargoMetadataResourceV1::Features,
                limits.max_features,
                counters.features,
            )?;
        }
    }
    Ok(())
}

fn package_selector(name: &str, version: &str) -> Result<String, CargoMetadataErrorV1> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        || version.is_empty()
        || version.bytes().any(|byte| {
            byte.is_ascii_whitespace() || byte == 0 || byte == b'/' || byte == b'\\' || byte == b'@'
        })
    {
        return Err(CargoMetadataErrorV1::InvalidString("package selector"));
    }
    Ok(format!("{name}@{version}"))
}

fn check_string(
    value: &str,
    limits: &CargoMetadataLimitsV1,
    field: &'static str,
) -> Result<(), CargoMetadataErrorV1> {
    if value.is_empty() || value.contains('\0') || value.chars().any(char::is_control) {
        return Err(CargoMetadataErrorV1::InvalidString(field));
    }
    if value.len() > limits.max_string_bytes {
        return Err(CargoMetadataErrorV1::LimitExceeded {
            resource: CargoMetadataResourceV1::StringBytes,
            limit: limits.max_string_bytes,
            observed: value.len(),
        });
    }
    Ok(())
}

fn normalize_path(
    raw: &str,
    repository_root: &Path,
    limits: &CargoMetadataLimitsV1,
    counters: &mut Counters,
    field: &'static str,
) -> Result<String, CargoMetadataErrorV1> {
    check_string(raw, limits, field)?;
    check_path_bytes(raw, limits)?;
    if raw.starts_with("file:") || raw.contains('\\') {
        return Err(CargoMetadataErrorV1::InvalidPath(field));
    }
    counters.paths = counters.paths.saturating_add(1);
    ensure_count(
        CargoMetadataResourceV1::Paths,
        limits.max_paths,
        counters.paths,
    )?;
    let candidate = Path::new(raw);
    let relative = if candidate.is_absolute() {
        candidate
            .strip_prefix(repository_root)
            .map_err(|_| CargoMetadataErrorV1::InvalidPath(field))?
    } else {
        candidate
    };
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CargoMetadataErrorV1::InvalidPath(field));
            }
        }
    }
    let value = if normalized.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        normalized
            .to_str()
            .ok_or(CargoMetadataErrorV1::InvalidPath(field))?
            .to_owned()
    };
    check_path_bytes(&value, limits)?;
    Ok(value)
}

fn check_path_bytes(
    value: &str,
    limits: &CargoMetadataLimitsV1,
) -> Result<(), CargoMetadataErrorV1> {
    if value.len() > limits.max_path_bytes {
        return Err(CargoMetadataErrorV1::LimitExceeded {
            resource: CargoMetadataResourceV1::PathBytes,
            limit: limits.max_path_bytes,
            observed: value.len(),
        });
    }
    Ok(())
}

fn package_root(manifest_path: &str) -> String {
    Path::new(manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| !path.is_empty())
        .unwrap_or(".")
        .to_owned()
}

fn ensure_count(
    resource: CargoMetadataResourceV1,
    limit: usize,
    observed: usize,
) -> Result<(), CargoMetadataErrorV1> {
    if observed > limit {
        Err(CargoMetadataErrorV1::LimitExceeded {
            resource,
            limit,
            observed,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;
