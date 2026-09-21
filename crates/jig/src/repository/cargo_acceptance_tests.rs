use std::{collections::BTreeMap, fs, path::Path, process::Command};

use jig_contract::{
    CargoImpactContextV1, CargoImpactDispositionV1, CargoImpactReasonV1, CargoImpactV1,
    CargoRuntimeTestFilterV1, ComponentId,
};
use jig_rust::{CargoMetadataGraphV1, select_cargo_impact_v1};
use tempfile::tempdir;

use super::*;
use crate::execution::NoopExecutionObserver;

fn write_fixture_file(root: &Path, relative: &str, contents: &[u8]) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn create_real_workspace(root: &Path) -> Vec<String> {
    write_fixture_file(
        root,
        "Cargo.toml",
        br#"[workspace]
members = ["leaf", "consumer", "transitive", "unrelated"]
exclude = ["nested"]
resolver = "2"
"#,
    );
    write_fixture_file(
        root,
        "leaf/Cargo.toml",
        br#"[package]
name = "example-leaf"
version = "0.1.0"
edition = "2021"
build = "build.rs"
"#,
    );
    write_fixture_file(
        root,
        "leaf/src/lib.rs",
        b"pub fn leaf_value() -> &'static str { \"leaf\" }\n",
    );
    write_fixture_file(
        root,
        "leaf/build.rs",
        br#"fn main() {
    let path = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("build-ran-sentinel");
    std::fs::write(path, b"build script ran").unwrap();
}
"#,
    );
    write_fixture_file(root, "leaf/build-input.txt", b"ordinary build input\n");
    write_fixture_file(
        root,
        "consumer/Cargo.toml",
        br#"[package]
name = "example-consumer"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "example-consumer-bin"
path = "src/main.rs"

[dependencies]
leaf_alias = { package = "example-leaf", path = "../leaf" }
"#,
    );
    write_fixture_file(
        root,
        "consumer/src/lib.rs",
        b"pub fn consumer_value() -> &'static str { leaf_alias::leaf_value() }\n",
    );
    write_fixture_file(
        root,
        "consumer/src/main.rs",
        b"fn main() { println!(\"{}\", leaf_alias::leaf_value()); }\n",
    );
    write_fixture_file(
        root,
        "transitive/Cargo.toml",
        br#"[package]
name = "example-transitive"
version = "0.1.0"
edition = "2021"

[dependencies]
example-consumer = { path = "../consumer" }
"#,
    );
    write_fixture_file(
        root,
        "transitive/src/lib.rs",
        b"pub fn transitive_value() -> &'static str { example_consumer::consumer_value() }\n",
    );
    write_fixture_file(
        root,
        "unrelated/Cargo.toml",
        br#"[package]
name = "example-unrelated"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_fixture_file(
        root,
        "unrelated/src/lib.rs",
        b"pub fn unrelated_value() -> &'static str { \"unrelated\" }\n",
    );
    write_fixture_file(
        root,
        "nested/Cargo.toml",
        br#"[workspace]
members = ["package"]
"#,
    );
    write_fixture_file(
        root,
        "nested/package/Cargo.toml",
        br#"[package]
name = "example-nested"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_fixture_file(
        root,
        "nested/package/src/lib.rs",
        b"pub fn nested_value() -> &'static str { \"nested\" }\n",
    );

    let status = Command::new("cargo")
        .args(["generate-lockfile", "--offline", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .current_dir(root)
        .env("CARGO_NET_OFFLINE", "true")
        .status()
        .unwrap();
    assert!(status.success(), "fixture lockfile generation failed");

    [
        "Cargo.toml",
        "Cargo.lock",
        "leaf/Cargo.toml",
        "leaf/src/lib.rs",
        "leaf/build.rs",
        "leaf/build-input.txt",
        "consumer/Cargo.toml",
        "consumer/src/lib.rs",
        "consumer/src/main.rs",
        "transitive/Cargo.toml",
        "transitive/src/lib.rs",
        "unrelated/Cargo.toml",
        "unrelated/src/lib.rs",
        "nested/Cargo.toml",
        "nested/package/Cargo.toml",
        "nested/package/src/lib.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn snapshot_files(root: &Path, files: &[String]) -> BTreeMap<String, Vec<u8>> {
    files
        .iter()
        .map(|file| (file.clone(), fs::read(root.join(file)).unwrap()))
        .collect()
}

fn package_selectors(impact: &jig_contract::CargoImpactV1) -> Vec<String> {
    impact
        .build_packages
        .iter()
        .map(|package| package.selector.clone())
        .collect()
}

fn impact_for(
    graph: &CargoMetadataGraphV1,
    context: &CargoImpactContextV1,
    paths: &[&str],
) -> CargoImpactV1 {
    let paths = paths
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    select_cargo_impact_v1(
        graph,
        ComponentId::parse("rust").unwrap(),
        "Cargo.toml",
        &paths,
        context.clone(),
    )
}

fn assert_leaf_impact(impact: &CargoImpactV1) {
    assert_eq!(impact.disposition, CargoImpactDispositionV1::Narrowed);
    assert_eq!(
        package_selectors(impact),
        vec![
            "example-consumer@0.1.0",
            "example-leaf@0.1.0",
            "example-transitive@0.1.0",
        ]
    );
    assert!(!package_selectors(impact).contains(&"example-unrelated@0.1.0".to_owned()));
    assert!(!impact.test_targets.is_empty());
    assert!(impact.test_targets.iter().all(|target| {
        impact
            .build_packages
            .iter()
            .any(|package| package.selector == target.package_selector)
    }));
    assert_eq!(impact.runtime_test_filter, CargoRuntimeTestFilterV1::Absent);
}

fn assert_build_script_fallback(graph: &CargoMetadataGraphV1, context: &CargoImpactContextV1) {
    let impact = impact_for(graph, context, &["leaf/build.rs"]);
    assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
    assert!(impact.reasons.iter().any(|reason| matches!(
        reason,
        CargoImpactReasonV1::BuildScriptInput { path } if path == "leaf/build.rs"
    )));
}

fn assert_ordinary_input_fallback(graph: &CargoMetadataGraphV1, context: &CargoImpactContextV1) {
    let impact = impact_for(graph, context, &["leaf/build-input.txt"]);
    assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
    assert!(impact.reasons.iter().any(|reason| matches!(
        reason,
        CargoImpactReasonV1::UnownedPath { path } if path == "leaf/build-input.txt"
    )));
}

fn assert_deleted_source_and_manifest(
    graph: &CargoMetadataGraphV1,
    context: &CargoImpactContextV1,
    expected_packages: &[String],
) {
    let deleted_source = impact_for(graph, context, &["leaf/src/deleted.rs"]);
    assert_eq!(
        deleted_source.disposition,
        CargoImpactDispositionV1::Narrowed
    );
    assert_eq!(package_selectors(&deleted_source), expected_packages);

    let manifest_change = impact_for(graph, context, &["leaf/Cargo.toml"]);
    assert_eq!(
        manifest_change.disposition,
        CargoImpactDispositionV1::BroadFallback
    );
    assert!(!manifest_change.build_packages.is_empty());
}

fn assert_relocated_source_impact(graph: &CargoMetadataGraphV1, context: &CargoImpactContextV1) {
    let relocated = impact_for(
        graph,
        context,
        &["leaf/src/moved.rs", "unrelated/src/moved.rs"],
    );
    assert_eq!(relocated.disposition, CargoImpactDispositionV1::Narrowed);
    let selectors = package_selectors(&relocated);
    assert!(!selectors.is_empty());
    assert_eq!(
        selectors,
        vec![
            "example-consumer@0.1.0",
            "example-leaf@0.1.0",
            "example-transitive@0.1.0",
            "example-unrelated@0.1.0",
        ]
    );
}

fn assert_excluded_source_is_unowned(graph: &CargoMetadataGraphV1, context: &CargoImpactContextV1) {
    assert!(
        graph
            .packages()
            .iter()
            .all(|package| !package.manifest_path.starts_with("nested/"))
    );
    let impact = impact_for(graph, context, &["nested/package/src/lib.rs"]);
    assert_eq!(impact.disposition, CargoImpactDispositionV1::BroadFallback);
    assert!(impact.reasons.iter().any(|reason| matches!(
        reason,
        CargoImpactReasonV1::UnownedPath { path } if path == "nested/package/src/lib.rs"
    )));
}

#[test]
fn real_offline_workspace_acquisition_and_impact_are_conservative_and_portable() {
    let temp = tempdir().unwrap();
    let files = create_real_workspace(temp.path());
    let before = snapshot_files(temp.path(), &files);
    let mut observer = NoopExecutionObserver;
    let acquisition =
        acquire_cargo_metadata(temp.path(), Path::new("Cargo.toml"), &mut observer).unwrap();
    let context = acquisition.context.clone();
    let impact = impact_for(&acquisition.graph, &context, &["leaf/src/lib.rs"]);

    assert_leaf_impact(&impact);
    assert_build_script_fallback(&acquisition.graph, &context);
    assert_ordinary_input_fallback(&acquisition.graph, &context);
    assert_deleted_source_and_manifest(&acquisition.graph, &context, &package_selectors(&impact));
    assert_relocated_source_impact(&acquisition.graph, &context);
    assert_excluded_source_is_unowned(&acquisition.graph, &context);

    let encoded = serde_json::to_string(&impact).unwrap();
    assert!(!encoded.contains(temp.path().to_string_lossy().as_ref()));
    assert!(!encoded.contains("path+file://"));
    assert!(!temp.path().join("leaf/build-ran-sentinel").exists());
    assert_eq!(before, snapshot_files(temp.path(), &files));
}

#[test]
fn missing_manifest_and_path_dependency_fail_without_empty_success() {
    let temp = tempdir().unwrap();
    let files = create_real_workspace(temp.path());
    fs::remove_file(temp.path().join("leaf/Cargo.toml")).unwrap();
    let mut observer = NoopExecutionObserver;
    let missing_manifest =
        acquire_cargo_metadata(temp.path(), Path::new("leaf/Cargo.toml"), &mut observer)
            .unwrap_err();
    assert_eq!(
        missing_manifest,
        CargoMetadataAcquisitionError::MissingManifest
    );
    assert_eq!(
        missing_manifest.public_reason(),
        Some(CargoImpactReasonV1::WorkspaceManifestMissing)
    );

    let broken = tempdir().unwrap();
    write_fixture_file(
        broken.path(),
        "Cargo.toml",
        br#"[package]
name = "example-broken"
version = "0.1.0"
edition = "2021"

[dependencies]
missing = { path = "../missing" }
"#,
    );
    write_fixture_file(broken.path(), "src/lib.rs", b"pub fn broken() {}\n");
    fs::write(
        broken.path().join("Cargo.lock"),
        b"# This file is intentionally present before discovery.\nversion = 4\n",
    )
    .unwrap();
    let mut observer = NoopExecutionObserver;
    let broken_error =
        acquire_cargo_metadata(broken.path(), Path::new("Cargo.toml"), &mut observer).unwrap_err();
    assert_eq!(broken_error, CargoMetadataAcquisitionError::NonZeroExit);
    assert_eq!(
        broken_error.public_reason(),
        Some(CargoImpactReasonV1::MetadataCommandNonZero)
    );
    assert!(!broken_error.is_cancellation());
    assert!(!temp.path().join("leaf/build-ran-sentinel").exists());
    assert!(
        files
            .iter()
            .all(|file| { file != "leaf/Cargo.toml" || !temp.path().join(file).exists() })
    );
}
