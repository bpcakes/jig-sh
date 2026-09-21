use std::{collections::BTreeSet, path::Path};

use jig_contract::{
    CargoImpactContextV1, CargoImpactDispositionV1, CargoImpactReasonV1, CargoImpactV1,
    CargoPackageImpactV1, CargoRuntimeTestFilterV1, CargoTargetImpactV1, ComponentId,
};

use super::normalize::{
    CargoMetadataGraphV1, MAX_METADATA_CHANGED_PATHS_V1, MAX_METADATA_PATH_BYTES_V1,
};

/// Select portable package and test-target impact from already-normalized facts.
pub fn select_cargo_impact_v1<P: AsRef<str>>(
    graph: &CargoMetadataGraphV1,
    component: ComponentId,
    workspace_manifest: impl Into<String>,
    changed_paths: &[P],
    context: CargoImpactContextV1,
) -> CargoImpactV1 {
    let workspace_manifest = safe_manifest(&workspace_manifest.into());
    let mut base = CargoImpactV1 {
        component,
        workspace_manifest,
        context,
        disposition: CargoImpactDispositionV1::Narrowed,
        reasons: Vec::new(),
        build_packages: Vec::new(),
        test_targets: Vec::new(),
        runtime_test_filter: CargoRuntimeTestFilterV1::Absent,
    };
    if base.context.metadata_format_version != 1 || !valid_context(&base.context) {
        return CargoImpactV1::unavailable(
            base.component,
            base.workspace_manifest,
            base.context,
            CargoImpactReasonV1::UnsupportedContext,
        );
    }
    if graph.workspace_members.is_empty() {
        return CargoImpactV1::unavailable(
            base.component,
            base.workspace_manifest,
            base.context,
            CargoImpactReasonV1::NoWorkspacePackages,
        );
    }
    if changed_paths.len() > MAX_METADATA_CHANGED_PATHS_V1 {
        return CargoImpactV1::unavailable(
            base.component,
            base.workspace_manifest,
            base.context,
            CargoImpactReasonV1::MetadataMalformed,
        );
    }
    // A target-filtered resolve graph can omit platform-specific dependencies
    // of host-built units (for example procedural macros). Without separate
    // host authority it cannot prove the complete reverse-consumer closure.
    if base.context.target.is_some() {
        base.disposition = CargoImpactDispositionV1::BroadFallback;
        base.reasons.push(CargoImpactReasonV1::UnsupportedContext);
        fill_all_workspace_impact(&mut base, graph);
        return finish(base);
    }
    if changed_paths.is_empty() {
        base.disposition = CargoImpactDispositionV1::BroadFallback;
        base.reasons.push(CargoImpactReasonV1::EmptyChangedPathSet);
        fill_all_workspace_impact(&mut base, graph);
        return finish(base);
    }

    let mut owners = BTreeSet::new();
    let mut broad = false;
    for changed_path in changed_paths {
        let changed_path = changed_path.as_ref();
        let Some(path) = valid_changed_path(changed_path) else {
            return CargoImpactV1::unavailable(
                base.component,
                base.workspace_manifest,
                base.context,
                CargoImpactReasonV1::InvalidChangedPath,
            );
        };
        if topology_path(path) {
            base.reasons.push(CargoImpactReasonV1::TopologyChange {
                path: path.to_owned(),
            });
            broad = true;
        } else if Path::new(path).file_name().and_then(|name| name.to_str()) == Some("build.rs") {
            base.reasons.push(CargoImpactReasonV1::BuildScriptInput {
                path: path.to_owned(),
            });
            broad = true;
        } else if !path.ends_with(".rs") {
            base.reasons.push(CargoImpactReasonV1::UnownedPath {
                path: path.to_owned(),
            });
            broad = true;
        } else {
            base.reasons.push(CargoImpactReasonV1::ChangedSource {
                path: path.to_owned(),
            });
            match unique_owner(graph, path) {
                Owner::One(index) => {
                    owners.insert(index);
                }
                Owner::None => {
                    base.reasons.push(CargoImpactReasonV1::UnownedPath {
                        path: path.to_owned(),
                    });
                    broad = true;
                }
                Owner::Ambiguous => {
                    base.reasons.push(CargoImpactReasonV1::AmbiguousOwnership {
                        path: path.to_owned(),
                    });
                    broad = true;
                }
            }
        }
    }

    if broad {
        base.disposition = CargoImpactDispositionV1::BroadFallback;
        fill_all_workspace_impact(&mut base, graph);
        return finish(base);
    }

    let mut selected = BTreeSet::new();
    for owner in owners {
        let mut queue = vec![owner];
        let mut visited = BTreeSet::new();
        while let Some(index) = queue.pop() {
            if !visited.insert(index) {
                continue;
            }
            if graph
                .packages
                .get(index)
                .is_some_and(|package| package.workspace_member)
            {
                selected.insert(index);
                if index != owner
                    && let Some(package) = graph.packages.get(index)
                {
                    base.reasons.push(CargoImpactReasonV1::ConsumerClosure {
                        package_selector: package.selector.clone(),
                    });
                }
            }
            if let Some(consumers) = graph.reverse_consumers.get(index) {
                queue.extend(consumers.iter().copied());
            }
        }
    }
    if selected.is_empty() {
        base.disposition = CargoImpactDispositionV1::BroadFallback;
        base.reasons
            .push(CargoImpactReasonV1::MissingLocalDependency);
        fill_all_workspace_impact(&mut base, graph);
        return finish(base);
    }
    for index in selected {
        add_package_impact(&mut base, graph, index);
        if let Some(package) = graph.packages.get(index)
            && !base
                .reasons
                .iter()
                .any(|reason| matches!(reason, CargoImpactReasonV1::ChangedSource { .. }))
        {
            base.reasons.push(CargoImpactReasonV1::ChangedSource {
                path: package.package_root.clone(),
            });
        }
    }
    finish(base)
}

fn fill_all_workspace_impact(base: &mut CargoImpactV1, graph: &CargoMetadataGraphV1) {
    for index in &graph.workspace_members {
        add_package_impact(base, graph, *index);
    }
}

fn add_package_impact(base: &mut CargoImpactV1, graph: &CargoMetadataGraphV1, index: usize) {
    let Some(package) = graph.packages.get(index) else {
        return;
    };
    if package.manifest_path.is_empty() || package.selector.is_empty() {
        return;
    }
    base.build_packages.push(CargoPackageImpactV1 {
        selector: package.selector.clone(),
        manifest_path: package.manifest_path.clone(),
        workspace_member: package.workspace_member,
        activated_features: package.activated_features.clone(),
    });
    for target in &package.targets {
        if target.test || (target.doctest && target.kinds.iter().any(|kind| kind == "lib")) {
            base.test_targets.push(CargoTargetImpactV1 {
                package_selector: package.selector.clone(),
                name: target.name.clone(),
                kinds: target.kinds.clone(),
            });
        }
    }
}

fn finish(mut impact: CargoImpactV1) -> CargoImpactV1 {
    if impact.disposition == CargoImpactDispositionV1::Narrowed && impact.build_packages.is_empty()
    {
        impact.disposition = CargoImpactDispositionV1::Unavailable;
        impact
            .reasons
            .push(CargoImpactReasonV1::MissingLocalDependency);
    }
    impact.sort_canonical();
    impact
}

fn valid_context(context: &CargoImpactContextV1) -> bool {
    context.locked
        && context.offline
        && context.features.len() <= super::normalize::MAX_METADATA_FEATURES_V1
        && context.target.as_deref().is_none_or(valid_string)
        && context.features.iter().all(|feature| valid_string(feature))
}

fn valid_string(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= super::normalize::MAX_METADATA_STRING_BYTES_V1
        && !value.contains('\0')
        && !value.chars().any(char::is_control)
}

fn valid_changed_path(path: &str) -> Option<&str> {
    if path.is_empty()
        || path.len() > MAX_METADATA_PATH_BYTES_V1
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.chars().any(char::is_control)
    {
        return None;
    }
    let candidate = Path::new(path);
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(path)
}

fn safe_manifest(manifest: &str) -> String {
    valid_changed_path(manifest)
        .map(str::to_owned)
        .unwrap_or_else(|| ".".to_owned())
}

fn topology_path(path: &str) -> bool {
    let file_name = Path::new(path).file_name().and_then(|name| name.to_str());
    matches!(
        file_name,
        Some(
            "Cargo.toml"
                | "Cargo.lock"
                | "rust-toolchain"
                | "rust-toolchain.toml"
                | "rustfmt.toml"
                | ".rustfmt.toml"
                | "clippy.toml"
                | ".clippy.toml"
        )
    ) || path == ".cargo"
        || path.ends_with("/.cargo")
        || path.starts_with(".cargo/")
        || path.contains("/.cargo/")
}

enum Owner {
    One(usize),
    None,
    Ambiguous,
}

fn unique_owner(graph: &CargoMetadataGraphV1, path: &str) -> Owner {
    let mut best_depth = None;
    let mut best = Vec::new();
    for (index, package) in graph.packages.iter().enumerate() {
        if package.manifest_path.is_empty() || !under_root(path, &package.package_root) {
            continue;
        }
        let depth = if package.package_root == "." {
            0
        } else {
            package.package_root.split('/').count()
        };
        if best_depth.is_none_or(|best_depth| depth > best_depth) {
            best_depth = Some(depth);
            best.clear();
            best.push(index);
        } else if best_depth == Some(depth) {
            best.push(index);
        }
    }
    // Cargo targets may live outside their package directory, including inside
    // another workspace member. Their adjacent module sources share that
    // authority: a directory-only winner cannot safely exclude this package.
    for (index, package) in graph.packages.iter().enumerate() {
        if package.manifest_path.is_empty() {
            continue;
        }
        if package.targets.iter().any(|target| {
            !target.src_path.is_empty()
                && Path::new(&target.src_path)
                    .parent()
                    .and_then(Path::to_str)
                    .is_some_and(|parent| {
                        under_root(path, if parent.is_empty() { "." } else { parent })
                    })
        }) && !best.contains(&index)
        {
            best.push(index);
        }
    }
    match best.as_slice() {
        [] => Owner::None,
        [index] => Owner::One(*index),
        _ => Owner::Ambiguous,
    }
}

fn under_root(path: &str, root: &str) -> bool {
    let prefix = root.strip_suffix('/').unwrap_or(root);
    root == "." || path == root || path.starts_with(&(prefix.to_owned() + "/"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use jig_contract::{CargoImpactDispositionV1, CargoImpactReasonV1, ComponentId};
    use serde_json::json;

    use super::*;
    use crate::cargo::normalize::{CargoMetadataLimitsV1, normalize_cargo_metadata_v1};

    fn graph() -> CargoMetadataGraphV1 {
        let id = |name: &str| format!("path+file:///workspace/{name}#{name}@1.0.0");
        let leaf = id("ExampleLeaf");
        let middle = id("ExampleMiddle");
        let app = id("ExampleApp");
        let unrelated = id("ExampleUnrelated");
        let package = |name: &str, kind: &str| {
            let lower = name.to_ascii_lowercase();
            json!({
                "name": lower,
                "version": "1.0.0",
                "id": id(name),
                "manifest_path": format!("/workspace/{name}/Cargo.toml"),
                "targets": [
                    {"name": lower, "kind": [kind], "src_path": format!("/workspace/{name}/src/lib.rs"), "test": true, "doctest": kind == "lib"},
                    {"name": format!("{lower}_unit"), "kind": ["test"], "src_path": format!("/workspace/{name}/tests/unit.rs"), "test": true, "doctest": false}
                ],
                "features": {"default": []}
            })
        };
        let mut value = json!({
            "version": 1,
            "workspace_root": "/workspace",
            "packages": [package("ExampleLeaf", "lib"), package("ExampleMiddle", "lib"), package("ExampleApp", "bin"), package("ExampleUnrelated", "lib")],
            "workspace_members": [leaf, middle, app, unrelated],
            "resolve": {"nodes": [
                {"id": leaf, "dependencies": [], "deps": [], "features": ["default"]},
                {"id": middle, "dependencies": [leaf], "deps": [{"name": "renamed-leaf", "pkg": leaf, "dep_kinds": [{"kind": "build", "target": null}, {"kind": "dev", "target": "cfg(unix)"}]}], "features": []},
                {"id": app, "dependencies": [middle], "deps": [{"name": "middle", "pkg": middle, "dep_kinds": [{"kind": null, "target": null}]}], "features": []},
                {"id": unrelated, "dependencies": [], "deps": [], "features": []}
            ]}
        });
        value["unknown_future_field"] = json!(true);
        normalize_cargo_metadata_v1(
            &serde_json::to_vec(&value).unwrap(),
            Path::new("/workspace"),
            CargoMetadataLimitsV1::default(),
        )
        .expect("impact fixture is valid")
    }

    fn context() -> CargoImpactContextV1 {
        CargoImpactContextV1 {
            metadata_format_version: 1,
            ..CargoImpactContextV1::default()
        }
    }

    #[test]
    fn private_leaf_change_reaches_renamed_build_and_transitive_consumers() {
        let impact = select_cargo_impact_v1(
            &graph(),
            ComponentId::parse("rust").unwrap(),
            "Cargo.toml",
            &["ExampleLeaf/src/lib.rs".to_owned()],
            context(),
        );
        assert_eq!(impact.disposition, CargoImpactDispositionV1::Narrowed);
        let selectors: Vec<_> = impact
            .build_packages
            .iter()
            .map(|package| package.selector.as_str())
            .collect();
        assert_eq!(
            selectors,
            vec![
                "exampleapp@1.0.0",
                "exampleleaf@1.0.0",
                "examplemiddle@1.0.0"
            ]
        );
        assert!(!selectors.contains(&"exampleunrelated@1.0.0"));
        assert!(impact.runtime_test_filter == CargoRuntimeTestFilterV1::Absent);
        let serialized = serde_json::to_string(&impact).expect("impact serializes");
        assert!(!serialized.contains("file:///"));
        assert!(!serialized.contains("/workspace"));
    }

    #[test]
    fn platform_filtered_context_cannot_prove_host_consumer_closure() {
        let mut context = context();
        context.target = Some("aarch64-apple-darwin".into());
        let impact = select_cargo_impact_v1(
            &graph(),
            ComponentId::parse("rust").unwrap(),
            "Cargo.toml",
            &["ExampleLeaf/src/lib.rs"],
            context.clone(),
        );
        assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
        assert_eq!(impact.context, context);
        assert_eq!(impact.build_packages.len(), 4);
        assert!(
            impact
                .reasons
                .contains(&CargoImpactReasonV1::UnsupportedContext)
        );
    }

    #[test]
    fn topology_unowned_and_ambiguous_inputs_use_broad_fallback() {
        let impact = select_cargo_impact_v1(
            &graph(),
            ComponentId::parse("rust").unwrap(),
            "Cargo.toml",
            &["Cargo.toml".to_owned(), "README.md".to_owned()],
            context(),
        );
        assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
        assert!(
            impact
                .reasons
                .iter()
                .any(|reason| matches!(reason, CargoImpactReasonV1::TopologyChange { .. }))
        );
        assert!(
            impact
                .reasons
                .iter()
                .any(|reason| matches!(reason, CargoImpactReasonV1::UnownedPath { .. }))
        );
        assert_eq!(impact.build_packages.len(), 4);
    }

    #[test]
    fn overlapping_package_roots_are_typed_as_ambiguous() {
        let mut graph = graph();
        graph.packages[1].package_root = graph.packages[0].package_root.clone();
        let impact = select_cargo_impact_v1(
            &graph,
            ComponentId::parse("rust").unwrap(),
            "Cargo.toml",
            &["ExampleLeaf/src/lib.rs".to_owned()],
            context(),
        );
        assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
        assert!(impact.reasons.iter().any(|reason| matches!(
            reason,
            CargoImpactReasonV1::AmbiguousOwnership { path } if path == "ExampleLeaf/src/lib.rs"
        )));
    }

    #[test]
    fn declared_target_source_directory_conflicts_broaden_ownership() {
        let mut graph = graph();
        let shared = "ExampleUnrelated/shared.rs";
        graph.packages[0].targets[0].src_path = shared.into();
        for path in [shared, "ExampleUnrelated/shared_module.rs"] {
            let impact = select_cargo_impact_v1(
                &graph,
                ComponentId::parse("rust").unwrap(),
                "Cargo.toml",
                &[path],
                context(),
            );
            assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
            assert_eq!(impact.build_packages.len(), 4);
            assert!(impact.reasons.iter().any(|reason| matches!(
                reason,
                CargoImpactReasonV1::AmbiguousOwnership { path: changed } if changed == path
            )));
        }
    }

    #[test]
    fn invalid_path_and_context_never_return_empty_narrowing() {
        let invalid_path = select_cargo_impact_v1(
            &graph(),
            ComponentId::parse("rust").unwrap(),
            "/private/workspace/Cargo.toml",
            &["/private/workspace/src/lib.rs".to_owned()],
            context(),
        );
        assert_eq!(
            invalid_path.disposition,
            CargoImpactDispositionV1::Unavailable
        );
        assert!(invalid_path.build_packages.is_empty());
        assert_eq!(invalid_path.workspace_manifest, ".");
        let mut bad_context = context();
        bad_context.metadata_format_version = 2;
        let impact = select_cargo_impact_v1(
            &graph(),
            ComponentId::parse("rust").unwrap(),
            "Cargo.toml",
            &["ExampleLeaf/src/lib.rs".to_owned()],
            bad_context,
        );
        assert_eq!(impact.disposition, CargoImpactDispositionV1::Unavailable);
        assert!(impact.build_packages.is_empty());

        for (locked, offline) in [(false, true), (true, false)] {
            let mut bad_context = context();
            bad_context.locked = locked;
            bad_context.offline = offline;
            let impact = select_cargo_impact_v1(
                &graph(),
                ComponentId::parse("rust").unwrap(),
                "Cargo.toml",
                &["ExampleLeaf/src/lib.rs"],
                bad_context,
            );
            assert_eq!(impact.disposition, CargoImpactDispositionV1::Unavailable);
            assert!(
                impact
                    .reasons
                    .iter()
                    .any(|reason| matches!(reason, CargoImpactReasonV1::UnsupportedContext))
            );
        }
    }
}
