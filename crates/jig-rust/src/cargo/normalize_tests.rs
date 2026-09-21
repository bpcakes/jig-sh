use std::path::Path;

use serde_json::json;

use super::*;

fn metadata(extra: serde_json::Value) -> Vec<u8> {
    let package = |id: &str, name: &str, manifest: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "id": id,
            "manifest_path": manifest,
            "targets": [{
                "kind": ["lib"],
                "crate_types": ["lib"],
                "name": name,
                "src_path": manifest.replace("Cargo.toml", "src/lib.rs"),
                "edition": "2024",
                "doc": true,
                "doctest": true,
                "test": true
            }],
            "features": {}
        })
    };
    let root_id = "path+file:///workspace/crates/ExampleRoot#example-root@1.0.0";
    let leaf_id = "path+file:///workspace/crates/ExampleLeaf#example-leaf@1.0.0";
    let mut value = json!({
        "version": 1,
        "workspace_root": "/workspace",
        "packages": [
            package(root_id, "example-root", "/workspace/crates/ExampleRoot/Cargo.toml"),
            package(leaf_id, "example-leaf", "/workspace/crates/ExampleLeaf/Cargo.toml")
        ],
        "workspace_members": [root_id, leaf_id],
        "resolve": {"nodes": [
            {"id": root_id, "dependencies": [leaf_id], "deps": [{"name": "renamed-leaf", "pkg": leaf_id, "dep_kinds": [{"kind": null, "target": null}]}], "features": []},
            {"id": leaf_id, "dependencies": [], "deps": [], "features": []}
        ]}
    });
    if let (Some(object), Some(extra_object)) = (value.as_object_mut(), extra.as_object()) {
        for (key, item) in extra_object {
            object.insert(key.clone(), item.clone());
        }
    }
    serde_json::to_vec(&value).expect("fixture serializes")
}

#[test]
fn normalizes_unknown_fields_and_renamed_edges_without_paths() {
    let graph = normalize_cargo_metadata_v1(
        &metadata(json!({"future_field": {"safe": true}})),
        Path::new("/workspace"),
        CargoMetadataLimitsV1::default(),
    )
    .expect("fixture is valid");
    assert_eq!(graph.workspace_root(), ".");
    assert_eq!(graph.workspace_members().count(), 2);
    assert!(graph.verifies_workspace_selector("example-root@1.0.0"));
    assert!(graph.verifies_workspace_selector("example-leaf@1.0.0"));
    assert!(!graph.verifies_workspace_selector("example-root"));
    assert!(!graph.verifies_workspace_selector("missing@1.0.0"));
    assert!(!graph.verifies_workspace_selector(
        "path+file:///workspace/crates/ExampleRoot#example-root@1.0.0"
    ));
    assert!(!format!("{graph:?}").contains("path+file:"));
    let leaf = graph
        .packages()
        .iter()
        .find(|package| package.selector == "example-leaf@1.0.0")
        .expect("leaf package");
    let consumers = graph
        .reverse_consumers(&leaf.selector)
        .expect("leaf has a graph entry");
    assert_eq!(consumers[0].selector, "example-root@1.0.0");
    assert!(
        graph
            .packages()
            .iter()
            .all(|package| !package.manifest_path.starts_with('/'))
    );
}

#[test]
fn rejects_incomplete_or_ambiguous_graphs() {
    let mut value: serde_json::Value =
        serde_json::from_slice(&metadata(json!({}))).expect("fixture parses");
    value["resolve"]["nodes"][0]["dependencies"] = json!([]);
    assert_eq!(
        normalize_cargo_metadata_v1(
            &serde_json::to_vec(&value).unwrap(),
            Path::new("/workspace"),
            CargoMetadataLimitsV1::default(),
        ),
        Err(CargoMetadataErrorV1::IncompleteResolve)
    );
    value["packages"][1]["name"] = json!("example-root");
    value["packages"][1]["id"] = json!("different-id");
    assert_eq!(
        normalize_cargo_metadata_v1(
            &serde_json::to_vec(&value).unwrap(),
            Path::new("/workspace"),
            CargoMetadataLimitsV1::default(),
        ),
        Err(CargoMetadataErrorV1::DuplicateSelector)
    );
}

#[test]
fn discards_external_dependency_paths_after_validation() {
    let mut value: serde_json::Value =
        serde_json::from_slice(&metadata(json!({}))).expect("fixture parses");
    let external_id = "registry+https://example.invalid/index#external-dependency@1.0.0";
    value["packages"].as_array_mut().unwrap().push(json!({
        "name": "external-dependency",
        "version": "1.0.0",
        "id": external_id,
        "manifest_path": "/home/agent/.cargo/registry/src/external-dependency/Cargo.toml",
        "targets": [{
            "name": "external-dependency",
            "kind": ["lib"],
            "src_path": "/home/agent/.cargo/registry/src/external-dependency/src/lib.rs",
            "test": true,
            "doctest": true
        }],
        "features": {}
    }));
    value["resolve"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": external_id,
            "dependencies": [],
            "deps": [],
            "features": []
        }));
    let graph = normalize_cargo_metadata_v1(
        &serde_json::to_vec(&value).unwrap(),
        Path::new("/workspace"),
        CargoMetadataLimitsV1::default(),
    )
    .expect("external packages are valid graph members");
    let external = graph
        .packages()
        .iter()
        .find(|package| package.selector == "external-dependency@1.0.0")
        .expect("external package");
    assert!(external.manifest_path.is_empty());
    assert!(!graph.verifies_workspace_selector("external-dependency@1.0.0"));
    assert!(!format!("{graph:?}").contains(external_id));
    assert!(external.package_root.is_empty());
    assert!(
        external
            .targets
            .iter()
            .all(|target| target.src_path.is_empty())
    );
}

#[test]
fn enforces_named_byte_and_node_limits_before_returning_facts() {
    let bytes = metadata(json!({}));
    let limits = CargoMetadataLimitsV1 {
        max_metadata_bytes: bytes.len() - 1,
        ..CargoMetadataLimitsV1::default()
    };
    assert!(matches!(
        normalize_cargo_metadata_v1(&bytes, Path::new("/workspace"), limits),
        Err(CargoMetadataErrorV1::MetadataTooLarge { .. })
    ));

    let limits = CargoMetadataLimitsV1 {
        max_nodes: 1,
        ..CargoMetadataLimitsV1::default()
    };
    assert!(matches!(
        normalize_cargo_metadata_v1(&bytes, Path::new("/workspace"), limits),
        Err(CargoMetadataErrorV1::LimitExceeded {
            resource: CargoMetadataResourceV1::Nodes,
            ..
        })
    ));
}

#[test]
fn rejects_control_characters_before_normalized_facts_are_created() {
    let mut value: serde_json::Value =
        serde_json::from_slice(&metadata(json!({}))).expect("fixture parses");
    value["packages"][0]["name"] = json!("example\nroot");
    assert_eq!(
        normalize_cargo_metadata_v1(
            &serde_json::to_vec(&value).unwrap(),
            Path::new("/workspace"),
            CargoMetadataLimitsV1::default(),
        ),
        Err(CargoMetadataErrorV1::InvalidString("package name"))
    );
}
