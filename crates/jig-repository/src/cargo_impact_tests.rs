use std::{cell::Cell, fs, path::Path, rc::Rc};

use jig_contract::{
    ActionEffect, ActionIntent, ActionRunner, ActionSpec, CargoImpactContextV1,
    CargoImpactDispositionV1, CargoImpactReasonV1, ComponentId, ComponentSpec, ProfileId,
    ProfileSpec, RunPlan, SelectionReason, SourceIdentity, TargetId,
};
use jig_rust::{CargoMetadataLimitsV1, normalize_cargo_metadata_v1};
use serde_json::json;
use tempfile::tempdir;

use super::*;

struct StubAcquirer {
    results: Vec<
        std::result::Result<
            cargo_discovery::CargoMetadataAcquisition,
            cargo_discovery::CargoMetadataAcquisitionError,
        >,
    >,
    manifests: Vec<String>,
    mutate_source: Option<std::path::PathBuf>,
    cancel_after_acquire: Option<Rc<Cell<bool>>>,
}

impl CargoImpactAcquirer for StubAcquirer {
    fn acquire(
        &mut self,
        _repository_root: &Path,
        workspace_manifest: &Path,
        _observer: &mut dyn ExecutionControl,
    ) -> std::result::Result<
        cargo_discovery::CargoMetadataAcquisition,
        cargo_discovery::CargoMetadataAcquisitionError,
    > {
        self.manifests
            .push(workspace_manifest.display().to_string());
        if let Some(path) = self.mutate_source.take() {
            fs::write(path, b"mutated").unwrap();
        }
        if self.results.is_empty() {
            panic!("stub result configured for every acquisition");
        }
        let result = self.results.remove(0);
        if let Some(cancelled) = &self.cancel_after_acquire {
            cancelled.set(true);
        }
        result
    }
}

fn component(id: &str, root: &str) -> ComponentSpec {
    let mut component = ComponentSpec::new(ComponentId::parse(id).unwrap(), root);
    component.adapters = vec!["rust".into()];
    component
}

fn catalog(components: &[ComponentSpec]) -> RepositoryCatalog {
    catalog_with_dotenv_input(components, false)
}

fn catalog_with_dotenv_input(
    components: &[ComponentSpec],
    include_dotenv_input: bool,
) -> RepositoryCatalog {
    let mut actions = Vec::new();
    let mut targets = Vec::new();
    for component in components {
        let target: TargetId = format!("{}:test", component.id).parse().unwrap();
        let mut action = ActionSpec::new(
            target.clone(),
            ActionIntent::Check,
            ActionRunner::command("cargo_test_command"),
        );
        action.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
        action.inputs = vec!["**/*.rs".into(), "**/Cargo.toml".into()];
        if include_dotenv_input {
            action.inputs.push(".env".into());
        }
        actions.push(action);
        targets.push(target);
    }
    let profile = ProfileSpec::new(ProfileId::parse("verify").unwrap(), targets);
    RepositoryCatalog::from_native(
        6,
        "sha256:config",
        components,
        &actions,
        &[profile],
        Some(&ProfileId::parse("verify").unwrap()),
    )
    .unwrap()
}

fn plan(catalog: &RepositoryCatalog, components: &[ComponentSpec]) -> RunPlan {
    let targets = components
        .iter()
        .map(|component| {
            let target: TargetId = format!("{}:test", component.id).parse().unwrap();
            let mut planned = jig_contract::PlannedTarget::new(
                target,
                ActionIntent::Check,
                ActionRunner::command("cargo_test_command"),
                "digest",
            );
            planned.effects = vec![ActionEffect::ReadOnly, ActionEffect::Process];
            planned.reasons = vec![SelectionReason::Profile {
                profile: ProfileId::parse("verify").unwrap(),
            }];
            planned
        })
        .collect();
    let mut plan = RunPlan::new(
        "",
        catalog.config_digest(),
        SourceIdentity::new(None, "source"),
        targets,
        Vec::new(),
    );
    plan.affected_base = Some("HEAD".into());
    plan
}

fn metadata(root: &Path, workspace_root: &str) -> cargo_discovery::CargoMetadataAcquisition {
    let id = "path+file:///workspace#ExamplePackage@0.1.0";
    let package_root = if workspace_root == "." {
        root.to_path_buf()
    } else {
        root.join(workspace_root)
    };
    let bytes = serde_json::to_vec(&json!({
        "version": 1,
        "workspace_root": package_root,
        "packages": [{
            "name": "ExamplePackage",
            "version": "0.1.0",
            "id": id,
            "manifest_path": package_root.join("Cargo.toml"),
            "targets": [{
                "name": "example",
                "kind": ["lib"],
                "src_path": package_root.join("src/lib.rs"),
                "test": true,
                "doctest": true
            }],
            "features": {}
        }],
        "workspace_members": [id],
        "resolve": {
            "nodes": [{
                "id": id,
                "dependencies": [],
                "deps": [],
                "features": []
            }]
        }
    }))
    .unwrap();
    let graph =
        normalize_cargo_metadata_v1(&bytes, root, CargoMetadataLimitsV1::default()).unwrap();
    cargo_discovery::CargoMetadataAcquisition {
        graph,
        context: CargoImpactContextV1::default(),
    }
}

#[test]
fn rust_components_narrow_and_deduplicate_shared_roots() {
    let temp = tempdir().unwrap();
    let first = component("api", ".");
    let second = component("worker", ".");
    let catalog = catalog(&[first.clone(), second.clone()]);
    let mut plan = plan(&catalog, &[first, second]);
    let mut acquirer = StubAcquirer {
        results: vec![Ok(metadata(temp.path(), "."))],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };
    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src/lib.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();
    assert_eq!(acquirer.manifests, ["Cargo.toml"]);
    assert_eq!(plan.cargo_impacts.len(), 2);
    assert!(plan.cargo_impacts.iter().all(|impact| {
        impact.disposition == CargoImpactDispositionV1::Unavailable
            && impact
                .reasons
                .iter()
                .any(|reason| matches!(reason, CargoImpactReasonV1::AmbiguousOwnership { .. }))
    }));
}

#[test]
fn affected_rust_component_attaches_narrowed_package_scope() {
    let temp = tempdir().unwrap();
    let rust = component("api", ".");
    let catalog = catalog(std::slice::from_ref(&rust));
    let mut plan = plan(&catalog, std::slice::from_ref(&rust));
    let mut acquirer = StubAcquirer {
        results: vec![Ok(metadata(temp.path(), "."))],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };

    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src/lib.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();

    assert_eq!(acquirer.manifests, ["Cargo.toml"]);
    assert_eq!(plan.cargo_impacts.len(), 1);
    let impact = &plan.cargo_impacts[0];
    assert_eq!(impact.disposition, CargoImpactDispositionV1::Narrowed);
    assert_eq!(impact.build_packages[0].selector, "ExamplePackage@0.1.0");
}

fn nested_workspace_impact(root: &Path, workspace: &str, paths: &[String]) -> CargoImpactV1 {
    let members = ["a", "b"].map(|name| {
        let id = format!("path+file:///ExampleWorkspace/{name}#example-{name}@0.1.0");
        let package_root = root.join(workspace).join(name);
        json!({"name":format!("example-{name}"), "version":"0.1.0", "id":id,
            "manifest_path":package_root.join("Cargo.toml"),
            "targets":[{"name":format!("example_{name}"), "kind":["lib"],
                "src_path":package_root.join("src/lib.rs"), "test":true}], "features":{}})
    });
    let ids = members
        .iter()
        .map(|package| package["id"].clone())
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&json!({
        "version":1, "workspace_root":root.join(workspace), "packages":members,
        "workspace_members":ids,
        "resolve":{"nodes":ids.iter().map(|id| json!({"id":id, "dependencies":[], "deps":[], "features":[]})).collect::<Vec<_>>()}
    })).unwrap();
    let graph =
        normalize_cargo_metadata_v1(&bytes, root, CargoMetadataLimitsV1::default()).unwrap();
    let rust = component("backend", workspace);
    let catalog = catalog(std::slice::from_ref(&rust));
    let mut plan = plan(&catalog, std::slice::from_ref(&rust));
    let mut acquirer = StubAcquirer {
        results: vec![Ok(cargo_discovery::CargoMetadataAcquisition {
            graph,
            context: CargoImpactContextV1::default(),
        })],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };
    attach_cargo_impacts_with_acquirer(root, &catalog, &mut plan, paths, &[], None, &mut acquirer)
        .unwrap();
    plan.cargo_impacts.remove(0)
}

#[test]
fn ancestor_configuration_broadens_nested_cargo_workspace() {
    let temp = tempdir().unwrap();
    for (workspace, config_path) in [
        ("backend", ".cargo/config.toml"),
        ("backend", ".cargo/config"),
        ("backend", ".cargo"),
        ("backend", "rust-toolchain.toml"),
        ("backend", "Cargo.toml"),
        ("product/backend", "product/.cargo/config.toml"),
        ("product/backend", "product/.cargo"),
        ("product/backend", ".cargo/config.toml"),
        ("product/backend", "product/Cargo.lock"),
    ] {
        let source = format!("{workspace}/a/src/lib.rs");
        let source_only =
            nested_workspace_impact(temp.path(), workspace, std::slice::from_ref(&source));
        assert_eq!(source_only.disposition, CargoImpactDispositionV1::Narrowed);
        assert_eq!(
            source_only
                .build_packages
                .iter()
                .map(|package| package.selector.as_str())
                .collect::<Vec<_>>(),
            ["example-a@0.1.0"]
        );
        let changed =
            nested_workspace_impact(temp.path(), workspace, &[source, config_path.into()]);
        assert_eq!(
            changed.disposition,
            CargoImpactDispositionV1::BroadFallback,
            "{workspace}: {config_path}"
        );
        assert_eq!(
            changed
                .build_packages
                .iter()
                .map(|package| package.selector.as_str())
                .collect::<Vec<_>>(),
            ["example-a@0.1.0", "example-b@0.1.0"]
        );
        let expected_reason = CargoImpactReasonV1::TopologyChange {
            path: config_path.into(),
        };
        assert!(changed.reasons.contains(&expected_reason));
    }
}

#[test]
fn sibling_configuration_is_not_inherited_by_unrelated_workspace() {
    let temp = tempdir().unwrap();
    for config_path in [
        "frontend/.cargo/config.toml",
        "backend-old/.cargo/config",
        ".cargo-other/config.toml",
        "frontend/rust-toolchain.toml",
    ] {
        let impact = nested_workspace_impact(
            temp.path(),
            "backend",
            &["backend/a/src/lib.rs".into(), config_path.into()],
        );
        assert_eq!(
            impact.disposition,
            CargoImpactDispositionV1::Narrowed,
            "{config_path}"
        );
        assert_eq!(
            impact
                .build_packages
                .iter()
                .map(|package| package.selector.as_str())
                .collect::<Vec<_>>(),
            ["example-a@0.1.0"]
        );
    }
}

#[test]
fn ancestor_configuration_is_shared_even_when_an_ancestor_component_owns_the_file() {
    let components = [
        component("root", "."),
        component("backend", "backend"),
        component("worker", "worker"),
    ];
    let catalog = catalog(&components);
    let plan = plan(&catalog, &components);
    let rust = selected_rust_components(&catalog, &plan);
    let assignments = assign_changed_paths(
        &rust,
        &BTreeSet::from([".cargo/config.toml".into(), "backend/a/src/lib.rs".into()]),
    );
    for component in components {
        assert!(
            assignments.owned[&component.id].contains(&".cargo/config.toml".into()),
            "{}",
            component.id
        );
    }
    assert!(assignments.ambiguous.is_empty());
}

#[test]
fn unrelated_observed_dotenv_does_not_broaden_cargo_impact() {
    let temp = tempdir().unwrap();
    let rust = component("api", ".");
    let catalog = catalog(std::slice::from_ref(&rust));
    let mut plan = plan(&catalog, std::slice::from_ref(&rust));
    let mut acquirer = StubAcquirer {
        results: vec![Ok(metadata(temp.path(), "."))],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };

    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src/lib.rs".into()],
        &[".env".into()],
        None,
        &mut acquirer,
    )
    .unwrap();

    assert_eq!(
        plan.cargo_impacts[0].disposition,
        CargoImpactDispositionV1::Narrowed
    );
}

#[test]
fn declared_observed_dotenv_broadens_cargo_impact() {
    let temp = tempdir().unwrap();
    let rust = component("api", ".");
    let catalog = catalog_with_dotenv_input(std::slice::from_ref(&rust), true);
    let mut plan = plan(&catalog, std::slice::from_ref(&rust));
    let mut acquirer = StubAcquirer {
        results: vec![Ok(metadata(temp.path(), "."))],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };

    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src/lib.rs".into()],
        &[".env".into()],
        None,
        &mut acquirer,
    )
    .unwrap();

    let impact = &plan.cargo_impacts[0];
    assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
    assert!(impact.reasons.iter().any(|reason| {
        matches!(reason, CargoImpactReasonV1::UnownedPath { path } if path == ".env")
    }));
}

#[test]
fn non_rust_selection_does_not_acquire_or_publish_impacts() {
    let temp = tempdir().unwrap();
    let mut non_rust = component("api", ".");
    non_rust.adapters.clear();
    let catalog = catalog(std::slice::from_ref(&non_rust));
    let mut plan = plan(&catalog, std::slice::from_ref(&non_rust));
    let mut acquirer = StubAcquirer {
        results: Vec::new(),
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };

    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src/lib.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();

    assert!(acquirer.manifests.is_empty());
    assert!(plan.cargo_impacts.is_empty());
}

#[test]
fn nested_root_gets_most_specific_path_and_root_does_not_claim_excluded_workspace() {
    let temp = tempdir().unwrap();
    let root = component("root", ".");
    let nested = component("nested", "nested");
    let catalog = catalog(&[root.clone(), nested.clone()]);
    let mut plan = plan(&catalog, &[root, nested]);
    let mut acquirer = StubAcquirer {
        results: vec![
            Ok(metadata(temp.path(), ".")),
            Ok(metadata(temp.path(), "nested")),
        ],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };
    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["nested/src/lib.rs".into(), "excluded/src/lib.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();
    assert_eq!(acquirer.manifests, ["Cargo.toml", "nested/Cargo.toml"]);
    assert_eq!(plan.cargo_impacts[0].component.as_str(), "nested");
    assert_eq!(plan.cargo_impacts[1].component.as_str(), "root");
    assert_eq!(
        plan.cargo_impacts[1].reasons,
        [CargoImpactReasonV1::UnownedPath {
            path: "nested/src/lib.rs".into()
        }]
    );
}

#[test]
fn acquisition_failure_and_cancellation_are_conservative_and_distinct() {
    let temp = tempdir().unwrap();
    let rust = component("api", ".");
    let catalog = catalog(std::slice::from_ref(&rust));
    let mut run_plan = plan(&catalog, std::slice::from_ref(&rust));
    let mut acquirer = StubAcquirer {
        results: vec![Err(
            cargo_discovery::CargoMetadataAcquisitionError::ProgramUnavailable,
        )],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };
    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut run_plan,
        &["src/lib.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();
    assert_eq!(
        run_plan.cargo_impacts[0].reasons,
        [CargoImpactReasonV1::CargoProgramUnavailable]
    );

    let mut cancelled_plan = plan(&catalog, std::slice::from_ref(&rust));
    let mut cancelled = StubAcquirer {
        results: vec![Err(
            cargo_discovery::CargoMetadataAcquisitionError::Cancelled,
        )],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };
    let error = attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut cancelled_plan,
        &["src/lib.rs".into()],
        &[],
        None,
        &mut cancelled,
    )
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(cancelled_plan.cargo_impacts.is_empty());
}

#[test]
fn discovery_root_bound_is_unavailable_without_invoking_cargo() {
    let temp = tempdir().unwrap();
    let components = (0..=MAX_CARGO_DISCOVERY_ROOTS_V1)
        .map(|index| component(&format!("crate-{index}"), &format!("crate-{index}")))
        .collect::<Vec<_>>();
    let catalog = catalog(&components);
    let mut plan = plan(&catalog, &components);
    let mut acquirer = StubAcquirer {
        results: Vec::new(),
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };

    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["crate-0/src/lib.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();

    assert!(acquirer.manifests.is_empty());
    assert_eq!(plan.cargo_impacts.len(), MAX_CARGO_DISCOVERY_ROOTS_V1 + 1);
    assert!(
        plan.cargo_impacts.iter().all(|impact| {
            impact.reasons == [CargoImpactReasonV1::MetadataResourceLimitExceeded]
        })
    );
}

#[test]
fn cancellation_precedes_root_bound_fallback() {
    let temp = tempdir().unwrap();
    let components = (0..=MAX_CARGO_DISCOVERY_ROOTS_V1)
        .map(|index| component(&format!("crate-{index}"), &format!("crate-{index}")))
        .collect::<Vec<_>>();
    let catalog = catalog(&components);
    let mut plan = plan(&catalog, &components);
    let mut acquirer = StubAcquirer {
        results: Vec::new(),
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: None,
    };
    let cancellation = || true;

    let error = attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["crate-0/src/lib.rs".into()],
        &[],
        Some(&cancellation),
        &mut acquirer,
    )
    .unwrap_err();

    assert!(error.to_string().contains("cancelled"));
    assert!(acquirer.manifests.is_empty());
    assert!(plan.cargo_impacts.is_empty());
}

#[test]
fn cancellation_after_acquisition_does_not_publish_impact() {
    let temp = tempdir().unwrap();
    let rust = component("api", ".");
    let catalog = catalog(std::slice::from_ref(&rust));
    let mut plan = plan(&catalog, std::slice::from_ref(&rust));
    let cancelled = Rc::new(Cell::new(false));
    let mut acquirer = StubAcquirer {
        results: vec![Ok(metadata(temp.path(), "."))],
        manifests: Vec::new(),
        mutate_source: None,
        cancel_after_acquire: Some(Rc::clone(&cancelled)),
    };
    let cancellation_flag = Rc::clone(&cancelled);
    let cancellation = move || cancellation_flag.get();

    let error = attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src/lib.rs".into()],
        &[],
        Some(&cancellation),
        &mut acquirer,
    )
    .unwrap_err();

    assert!(error.to_string().contains("cancelled"));
    assert_eq!(acquirer.manifests, ["Cargo.toml"]);
    assert!(plan.cargo_impacts.is_empty());
}

#[test]
fn source_mutation_during_acquisition_remains_visible_to_planner_boundary() {
    let temp = tempdir().unwrap();
    let rust = component("api", ".");
    let catalog = catalog(std::slice::from_ref(&rust));
    let mut plan = plan(&catalog, std::slice::from_ref(&rust));
    let source = temp.path().join("src.rs");
    fs::write(&source, b"before").unwrap();
    let mut acquirer = StubAcquirer {
        results: vec![Ok(metadata(temp.path(), "."))],
        manifests: Vec::new(),
        mutate_source: Some(source.clone()),
        cancel_after_acquire: None,
    };
    attach_cargo_impacts_with_acquirer(
        temp.path(),
        &catalog,
        &mut plan,
        &["src.rs".into()],
        &[],
        None,
        &mut acquirer,
    )
    .unwrap();
    assert_eq!(fs::read(source).unwrap(), b"mutated");
    assert_eq!(plan.cargo_impacts.len(), 1);
}
