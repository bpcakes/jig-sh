//! Attach conservative Cargo package-impact facts after generic affected
//! target selection has settled.
//!
//! This module deliberately does not alter authored target runners, arguments,
//! effects, or generic selection reasons. It adds planning evidence for each
//! selected component whose adapter is exactly rust and leaves all other plans
//! untouched.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::Result;
use jig_contract::{
    CargoImpactContextV1, CargoImpactReasonV1, CargoImpactV1, ComponentId, RunPlan,
};
use jig_rust::select_cargo_impact_v1;

use crate::{
    context::RepoContext,
    execution::{ExecutionCancellation, ExecutionControl, ExecutionObserver},
    repository::{RepositoryCatalog, cargo_discovery},
};

/// Keep one Cargo acquisition per distinct authored component root.
pub(super) const MAX_CARGO_DISCOVERY_ROOTS_V1: usize = 32;

#[derive(Clone, Debug)]
struct RustComponent {
    id: ComponentId,
    root: String,
    workspace_manifest: String,
}

/// Private seam used by production and focused planner tests. The seam keeps
/// tests independent of Cargo while the product path remains a direct,
/// repository-owned acquisition.
pub(super) trait CargoImpactAcquirer {
    fn acquire(
        &mut self,
        repository_root: &Path,
        workspace_manifest: &Path,
        observer: &mut dyn ExecutionControl,
    ) -> std::result::Result<
        cargo_discovery::CargoMetadataAcquisition,
        cargo_discovery::CargoMetadataAcquisitionError,
    >;
}

struct ProductionCargoImpactAcquirer;

impl CargoImpactAcquirer for ProductionCargoImpactAcquirer {
    fn acquire(
        &mut self,
        repository_root: &Path,
        workspace_manifest: &Path,
        observer: &mut dyn ExecutionControl,
    ) -> std::result::Result<
        cargo_discovery::CargoMetadataAcquisition,
        cargo_discovery::CargoMetadataAcquisitionError,
    > {
        cargo_discovery::acquire_cargo_metadata(repository_root, workspace_manifest, observer)
    }
}

struct PlanningExecutionControl<'a> {
    cancelled: Option<&'a dyn Fn() -> bool>,
}

impl ExecutionObserver for PlanningExecutionControl<'_> {}

impl ExecutionCancellation for PlanningExecutionControl<'_> {
    fn cancelled(&self) -> bool {
        self.cancelled.is_some_and(|cancelled| cancelled())
    }
}

/// Acquire and attach Cargo facts for an affected plan. Callers must invoke
/// this only after generic affected target selection and before final plan
/// digest/freshness work.
pub(super) fn attach_cargo_impacts(
    ctx: &RepoContext,
    catalog: &RepositoryCatalog,
    plan: &mut RunPlan,
    changed_paths: &[String],
    observed_input_paths: &[String],
    cancelled: Option<&dyn Fn() -> bool>,
) -> Result<()> {
    let mut acquirer = ProductionCargoImpactAcquirer;
    attach_cargo_impacts_with_acquirer(
        ctx.root(),
        catalog,
        plan,
        changed_paths,
        observed_input_paths,
        cancelled,
        &mut acquirer,
    )
}

pub(super) fn attach_cargo_impacts_with_acquirer(
    repository_root: &Path,
    catalog: &RepositoryCatalog,
    plan: &mut RunPlan,
    changed_paths: &[String],
    observed_input_paths: &[String],
    cancelled: Option<&dyn Fn() -> bool>,
    acquirer: &mut dyn CargoImpactAcquirer,
) -> Result<()> {
    let components = selected_rust_components(catalog, plan);
    if components.is_empty() {
        plan.cargo_impacts.clear();
        return Ok(());
    }

    let mut paths = changed_paths.iter().cloned().collect::<BTreeSet<_>>();
    paths.extend(selected_observed_input_paths(
        catalog,
        plan,
        &components,
        observed_input_paths,
    )?);
    let assignments = assign_changed_paths(&components, &paths);
    let mut roots = BTreeMap::<String, Vec<RustComponent>>::new();
    for component in components.values() {
        roots
            .entry(component.root.clone())
            .or_default()
            .push(component.clone());
    }

    let mut control = PlanningExecutionControl { cancelled };
    let mut impacts = Vec::with_capacity(components.len());
    if roots.len() > MAX_CARGO_DISCOVERY_ROOTS_V1 {
        if control.cancelled() {
            return Err(anyhow::anyhow!("Cargo metadata discovery was cancelled"));
        }
        for component in components.values() {
            impacts.push(unavailable(
                component,
                CargoImpactReasonV1::MetadataResourceLimitExceeded,
                CargoImpactContextV1::default(),
            ));
        }
        plan.cargo_impacts = canonical_impacts(impacts);
        return Ok(());
    }

    for (root, root_components) in roots {
        if control.cancelled() {
            return Err(anyhow::anyhow!("Cargo metadata discovery was cancelled"));
        }
        let manifest = root_components
            .first()
            .expect("Cargo root groups must not be empty")
            .workspace_manifest
            .clone();
        let acquisition = acquirer.acquire(repository_root, Path::new(&manifest), &mut control);
        if control.cancelled() {
            return Err(anyhow::anyhow!("Cargo metadata discovery was cancelled"));
        }
        let root_ambiguity = root_components.iter().find_map(|component| {
            assignments
                .ambiguous
                .get(&component.id)
                .and_then(|paths| paths.first())
                .cloned()
        });
        match acquisition {
            Ok(acquisition) => {
                let context = acquisition.context.clone();
                for component in root_components {
                    let impact = if acquisition.graph.workspace_root() != root {
                        unavailable(
                            &component,
                            CargoImpactReasonV1::UnsupportedWorkspaceRoot,
                            CargoImpactContextV1::default(),
                        )
                    } else if let Some(path) = root_ambiguity.as_deref() {
                        unavailable(
                            &component,
                            CargoImpactReasonV1::AmbiguousOwnership {
                                path: path.to_owned(),
                            },
                            context.clone(),
                        )
                    } else if let Some(path) = assignments
                        .shadowed
                        .get(&component.id)
                        .and_then(|paths| paths.first())
                    {
                        unavailable(
                            &component,
                            CargoImpactReasonV1::UnownedPath {
                                path: path.to_owned(),
                            },
                            context.clone(),
                        )
                    } else {
                        let component_paths = assignments
                            .owned
                            .get(&component.id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]);
                        select_cargo_impact_v1(
                            &acquisition.graph,
                            component.id.clone(),
                            component.workspace_manifest.clone(),
                            component_paths,
                            context.clone(),
                        )
                    };
                    impacts.push(impact);
                }
            }
            Err(error) if error.is_cancellation() => {
                return Err(anyhow::anyhow!("Cargo metadata discovery was cancelled"));
            }
            Err(error) => {
                let reason = error
                    .public_reason()
                    .expect("non-cancellation acquisition errors have a public reason");
                for component in root_components {
                    impacts.push(unavailable(
                        &component,
                        reason.clone(),
                        CargoImpactContextV1::default(),
                    ));
                }
            }
        }
    }
    plan.cargo_impacts = canonical_impacts(impacts);
    Ok(())
}

fn selected_observed_input_paths(
    catalog: &RepositoryCatalog,
    plan: &RunPlan,
    components: &BTreeMap<ComponentId, RustComponent>,
    observed_input_paths: &[String],
) -> Result<BTreeSet<String>> {
    let mut matchers = Vec::new();
    for planned in &plan.targets {
        if !components.contains_key(&planned.target.component) {
            continue;
        }
        let action = catalog
            .action(&planned.target)
            .expect("selected targets must exist in the repository catalog");
        matchers.extend(
            action
                .inputs
                .iter()
                .map(|input| super::affected::compile_input(&planned.target, input))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok(observed_input_paths
        .iter()
        .filter(|path| matchers.iter().any(|matcher| matcher.is_match(path)))
        .cloned()
        .collect())
}

fn selected_rust_components(
    catalog: &RepositoryCatalog,
    plan: &RunPlan,
) -> BTreeMap<ComponentId, RustComponent> {
    plan.targets
        .iter()
        .filter_map(|target| {
            let component = catalog.component(&target.target.component)?;
            component
                .adapters
                .iter()
                .any(|adapter| adapter == "rust")
                .then(|| {
                    let root = component.root.clone();
                    (
                        component.id.clone(),
                        RustComponent {
                            id: component.id.clone(),
                            workspace_manifest: component_manifest(&root),
                            root,
                        },
                    )
                })
        })
        .collect()
}

fn component_manifest(root: &str) -> String {
    if root == "." {
        "Cargo.toml".to_owned()
    } else {
        format!("{root}/Cargo.toml")
    }
}

struct PathAssignments {
    owned: BTreeMap<ComponentId, Vec<String>>,
    ambiguous: BTreeMap<ComponentId, Vec<String>>,
    shadowed: BTreeMap<ComponentId, Vec<String>>,
}

fn assign_changed_paths(
    components: &BTreeMap<ComponentId, RustComponent>,
    paths: &BTreeSet<String>,
) -> PathAssignments {
    let mut owned = BTreeMap::<ComponentId, Vec<String>>::new();
    let mut ambiguous = BTreeMap::<ComponentId, Vec<String>>::new();
    let mut shadowed = BTreeMap::<ComponentId, Vec<String>>::new();
    for path in paths {
        // Build configuration can be inherited from above a component root.
        // Preserve that shared authority before assigning ordinary files to
        // their most-specific owner; otherwise a local source edit could hide
        // an ancestor configuration change and falsely permit package narrowing.
        if let Some(directory) = rust_configuration_directory(path) {
            for component in components.values().filter(|component| {
                root_contains(directory, &component.root) && !root_contains(&component.root, path)
            }) {
                owned
                    .entry(component.id.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
        let mut matches = components
            .values()
            .filter(|component| root_contains(&component.root, path))
            .collect::<Vec<_>>();
        let Some(max_depth) = matches
            .iter()
            .map(|component| root_depth(&component.root))
            .max()
        else {
            continue;
        };
        matches.retain(|component| root_depth(&component.root) == max_depth);
        if matches.len() == 1 {
            owned
                .entry(matches[0].id.clone())
                .or_default()
                .push(path.clone());
            for component in components.values().filter(|component| {
                root_contains(&component.root, path) && root_depth(&component.root) < max_depth
            }) {
                shadowed
                    .entry(component.id.clone())
                    .or_default()
                    .push(path.clone());
            }
        } else {
            for component in matches {
                ambiguous
                    .entry(component.id.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
    }
    PathAssignments {
        owned,
        ambiguous,
        shadowed,
    }
}

fn rust_configuration_directory(path: &str) -> Option<&str> {
    if path == ".cargo" || path.starts_with(".cargo/") {
        return Some(".");
    }
    if let Some((directory, _)) = path.split_once("/.cargo/") {
        return Some(directory);
    }
    if let Some(directory) = path.strip_suffix("/.cargo") {
        return Some(directory);
    }
    let (directory, file) = path.rsplit_once('/').unwrap_or((".", path));
    matches!(
        file,
        "Cargo.toml"
            | "Cargo.lock"
            | "rust-toolchain"
            | "rust-toolchain.toml"
            | "rustfmt.toml"
            | ".rustfmt.toml"
            | "clippy.toml"
            | ".clippy.toml"
    )
    .then_some(directory)
}

fn unavailable(
    component: &RustComponent,
    reason: CargoImpactReasonV1,
    context: CargoImpactContextV1,
) -> CargoImpactV1 {
    CargoImpactV1::unavailable(
        component.id.clone(),
        component.workspace_manifest.clone(),
        context,
        reason,
    )
}

fn canonical_impacts(mut impacts: Vec<CargoImpactV1>) -> Vec<CargoImpactV1> {
    for impact in &mut impacts {
        impact.sort_canonical();
    }
    impacts.sort_by(|left, right| left.component.cmp(&right.component));
    impacts
}

fn root_contains(root: &str, path: &str) -> bool {
    root == "."
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn root_depth(root: &str) -> usize {
    if root == "." {
        0
    } else {
        root.split('/').count()
    }
}

#[cfg(test)]
#[path = "cargo_impact_tests.rs"]
mod tests;
