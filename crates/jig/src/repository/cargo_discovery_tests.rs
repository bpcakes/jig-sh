use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::execution::NoopExecutionObserver;

pub(super) fn fixture_metadata(root: &Path) -> Vec<u8> {
    let package_id = "path+file:///workspace#ExampleWorkspace@0.1.0";
    let manifest = root.join("Cargo.toml");
    let source = root.join("src/lib.rs");
    serde_json::to_vec(&json!({
        "version": 1,
        "workspace_root": root,
        "packages": [{
            "name": "ExampleWorkspace",
            "version": "0.1.0",
            "id": package_id,
            "manifest_path": manifest,
            "targets": [{
                "name": "ExampleWorkspace",
                "kind": ["lib"],
                "crate_types": ["lib"],
                "src_path": source,
                "edition": "2021",
                "doc": true,
                "doctest": true,
                "test": true
            }],
            "features": {}
        }],
        "workspace_members": [package_id],
        "resolve": {
            "nodes": [{
                "id": package_id,
                "dependencies": [],
                "deps": [],
                "features": []
            }]
        },
        "metadata": {"future": "ignored"}
    }))
    .expect("fixture JSON serializes")
}

#[derive(Default)]
pub(super) struct RecordingRunner {
    pub(super) command: Option<CargoMetadataCommand>,
    result: Option<Result<CargoMetadataProcessOutput, CargoMetadataProcessFailure>>,
}

impl CargoMetadataRunner for RecordingRunner {
    fn run(
        &mut self,
        command: &CargoMetadataCommand,
        _observer: &mut dyn ExecutionControl,
    ) -> Result<CargoMetadataProcessOutput, CargoMetadataProcessFailure> {
        self.command = Some(command.clone());
        self.result.take().expect("test runner result configured")
    }
}

pub(super) fn runner_with_stdout(stdout: Vec<u8>) -> RecordingRunner {
    RecordingRunner {
        command: None,
        result: Some(Ok(CargoMetadataProcessOutput {
            succeeded: true,
            stdout,
        })),
    }
}

fn run_with_runner(
    root: &Path,
    runner: &mut RecordingRunner,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    let manifest = root.join("Cargo.toml");
    let mut observer = NoopExecutionObserver;
    acquire_cargo_metadata_with_runner(root, &manifest, &mut observer, runner)
}

fn run_validated_with_runner(
    root: &Path,
    workspace_manifest: &Path,
    runner: &mut RecordingRunner,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    let manifest = validate_manifest(root, workspace_manifest)?;
    let mut observer = NoopExecutionObserver;
    acquire_cargo_metadata_with_runner(root, &manifest, &mut observer, runner)
}

#[test]
fn command_uses_exact_locked_offline_argv_root_and_environment() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let metadata = fixture_metadata(temp.path());
    let mut runner = runner_with_stdout(metadata);
    run_with_runner(temp.path(), &mut runner).unwrap();

    let command = runner.command.expect("runner recorded command");
    assert_eq!(command.program, OsString::from("cargo"));
    assert_eq!(command.current_dir, temp.path());
    assert_eq!(
        command.args,
        vec![
            OsString::from("metadata"),
            OsString::from("--format-version"),
            OsString::from("1"),
            OsString::from("--locked"),
            OsString::from("--offline"),
            OsString::from("--manifest-path"),
            temp.path().join("Cargo.toml").into_os_string(),
        ]
    );
    assert_eq!(
        command.env,
        vec![(OsString::from("CARGO_NET_OFFLINE"), OsString::from("true"))]
    );
}

#[test]
fn acquisition_normalizes_stdout_and_uses_explicit_authority() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let mut runner = runner_with_stdout(fixture_metadata(temp.path()));
    let acquisition = run_with_runner(temp.path(), &mut runner).unwrap();
    assert_eq!(acquisition.context, CargoImpactContextV1::default());
    assert_eq!(acquisition.graph.workspace_root(), ".");
    assert_eq!(
        acquisition.graph.packages()[0].selector,
        "ExampleWorkspace@0.1.0"
    );
}

#[test]
fn process_failures_have_stable_reasons_and_cancellation_stays_distinct() {
    let cases = [
        (
            CargoMetadataProcessFailure::ProgramUnavailable,
            CargoMetadataAcquisitionError::ProgramUnavailable,
            Some(CargoImpactReasonV1::CargoProgramUnavailable),
        ),
        (
            CargoMetadataProcessFailure::ProcessFailed,
            CargoMetadataAcquisitionError::ProcessFailed,
            Some(CargoImpactReasonV1::MetadataProcessFailure),
        ),
        (
            CargoMetadataProcessFailure::TimedOut,
            CargoMetadataAcquisitionError::TimedOut,
            Some(CargoImpactReasonV1::MetadataTimeout),
        ),
        (
            CargoMetadataProcessFailure::OutputLimitExceeded,
            CargoMetadataAcquisitionError::OutputLimitExceeded,
            Some(CargoImpactReasonV1::MetadataOutputLimitExceeded),
        ),
        (
            CargoMetadataProcessFailure::CancelledBeforeStart,
            CargoMetadataAcquisitionError::CancelledBeforeStart,
            None,
        ),
        (
            CargoMetadataProcessFailure::Cancelled,
            CargoMetadataAcquisitionError::Cancelled,
            None,
        ),
    ];
    for (failure, expected, reason) in cases {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
        let mut runner = RecordingRunner {
            command: None,
            result: Some(Err(failure)),
        };
        let error = run_with_runner(temp.path(), &mut runner).unwrap_err();
        assert_eq!(error, expected);
        assert_eq!(error.public_reason(), reason);
        assert_eq!(error.is_cancellation(), reason.is_none());
        if let Some(reason) = error.public_reason() {
            let portable = serde_json::to_string(&reason).unwrap();
            assert!(!portable.contains(temp.path().to_string_lossy().as_ref()));
            assert!(!portable.contains("cargo metadata"));
        }
    }
}

#[test]
fn missing_manifest_nonzero_exit_and_malformed_metadata_are_typed() {
    let temp = tempdir().unwrap();
    let mut observer = NoopExecutionObserver;
    let missing =
        acquire_cargo_metadata(temp.path(), Path::new("Cargo.toml"), &mut observer).unwrap_err();
    assert_eq!(missing, CargoMetadataAcquisitionError::MissingManifest);
    assert_eq!(
        missing.public_reason(),
        Some(CargoImpactReasonV1::WorkspaceManifestMissing)
    );

    fs::write(temp.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let mut nonzero = RecordingRunner {
        command: None,
        result: Some(Ok(CargoMetadataProcessOutput {
            succeeded: false,
            stdout: Vec::new(),
        })),
    };
    let nonzero_error = run_with_runner(temp.path(), &mut nonzero).unwrap_err();
    assert_eq!(nonzero_error, CargoMetadataAcquisitionError::NonZeroExit);
    assert_eq!(
        nonzero_error.public_reason(),
        Some(CargoImpactReasonV1::MetadataCommandNonZero)
    );

    let mut malformed = runner_with_stdout(b"{not-json".to_vec());
    let malformed_error = run_with_runner(temp.path(), &mut malformed).unwrap_err();
    assert_eq!(
        malformed_error,
        CargoMetadataAcquisitionError::MetadataValidation(CargoMetadataErrorV1::InvalidJson)
    );
    assert_eq!(
        malformed_error.public_reason(),
        Some(CargoImpactReasonV1::MetadataMalformed)
    );
}

#[test]
fn metadata_validation_reasons_distinguish_malformed_resource_and_graph_failures() {
    let malformed = [
        CargoMetadataErrorV1::InvalidJson,
        CargoMetadataErrorV1::UnsupportedFormat {
            expected: 1,
            observed: 2,
        },
        CargoMetadataErrorV1::MissingField("resolve"),
        CargoMetadataErrorV1::InvalidString("package name"),
        CargoMetadataErrorV1::InvalidPath("manifest"),
        CargoMetadataErrorV1::DuplicatePackageId,
        CargoMetadataErrorV1::DuplicateResolveNode,
        CargoMetadataErrorV1::DuplicateWorkspaceMember,
    ];
    for error in malformed {
        assert_eq!(
            CargoMetadataAcquisitionError::MetadataValidation(error).public_reason(),
            Some(CargoImpactReasonV1::MetadataMalformed)
        );
    }

    let resource_limited = [
        CargoMetadataErrorV1::MetadataTooLarge {
            limit: 16,
            observed: 17,
        },
        CargoMetadataErrorV1::LimitExceeded {
            resource: jig_rust::CargoMetadataResourceV1::Nodes,
            limit: 1,
            observed: 2,
        },
    ];
    for error in resource_limited {
        assert_eq!(
            CargoMetadataAcquisitionError::MetadataValidation(error).public_reason(),
            Some(CargoImpactReasonV1::MetadataResourceLimitExceeded)
        );
    }

    let graph_incomplete = [
        CargoMetadataErrorV1::IncompleteResolve,
        CargoMetadataErrorV1::UnknownPackage,
        CargoMetadataErrorV1::UnknownWorkspaceMember,
        CargoMetadataErrorV1::MissingResolveNode,
        CargoMetadataErrorV1::MissingDependencyKinds,
    ];
    for error in graph_incomplete {
        assert_eq!(
            CargoMetadataAcquisitionError::MetadataValidation(error).public_reason(),
            Some(CargoImpactReasonV1::IncompleteResolve)
        );
    }
    assert_eq!(
        CargoMetadataAcquisitionError::MetadataValidation(CargoMetadataErrorV1::DuplicateSelector,)
            .public_reason(),
        Some(CargoImpactReasonV1::DuplicateSelector)
    );
    assert_eq!(
        CargoMetadataAcquisitionError::MetadataValidation(
            CargoMetadataErrorV1::InvalidRepositoryRoot,
        )
        .public_reason(),
        Some(CargoImpactReasonV1::UnsupportedWorkspaceRoot)
    );
}

#[test]
fn unsupported_root_and_limits_are_unavailable_without_raw_details() {
    let relative_root = Path::new("relative-root");
    let mut observer = NoopExecutionObserver;
    let error =
        acquire_cargo_metadata(relative_root, Path::new("Cargo.toml"), &mut observer).unwrap_err();
    assert_eq!(
        error,
        CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot
    );
    assert_eq!(
        error.public_reason(),
        Some(CargoImpactReasonV1::UnsupportedWorkspaceRoot)
    );

    let temp = tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let mut runner = runner_with_stdout(vec![b'x'; 17 * 1024 * 1024]);
    let error = run_with_runner(temp.path(), &mut runner).unwrap_err();
    assert_eq!(
        error,
        CargoMetadataAcquisitionError::MetadataValidation(CargoMetadataErrorV1::MetadataTooLarge {
            limit: jig_rust::MAX_METADATA_BYTES_V1,
            observed: 17 * 1024 * 1024,
        })
    );
    assert_eq!(
        error.public_reason(),
        Some(CargoImpactReasonV1::MetadataResourceLimitExceeded)
    );
}

#[test]
fn real_offline_metadata_does_not_run_build_script_or_change_sources() {
    let temp = tempdir().unwrap();
    let cargo_toml = br#"[package]
name = "ExampleWorkspace"
version = "0.1.0"
edition = "2021"
build = "build.rs"
"#;
    let cargo_lock = br#"# This file is automatically @generated by Cargo.
version = 4

[[package]]
name = "ExampleWorkspace"
version = "0.1.0"
"#;
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("Cargo.toml"), cargo_toml).unwrap();
    fs::write(temp.path().join("Cargo.lock"), cargo_lock).unwrap();
    fs::write(temp.path().join("src/lib.rs"), b"pub fn example() {}\n").unwrap();
    fs::write(
            temp.path().join("build.rs"),
            br#"fn main() {
    let path = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("build-ran");
    std::fs::write(path, b"ran").unwrap();
}
"#,
        )
        .unwrap();
    let before = [
        fs::read(temp.path().join("Cargo.toml")).unwrap(),
        fs::read(temp.path().join("Cargo.lock")).unwrap(),
        fs::read(temp.path().join("build.rs")).unwrap(),
        fs::read(temp.path().join("src/lib.rs")).unwrap(),
    ];

    let mut observer = NoopExecutionObserver;
    let acquisition =
        acquire_cargo_metadata(temp.path(), Path::new("Cargo.toml"), &mut observer).unwrap();
    assert_eq!(
        acquisition.graph.packages()[0].selector,
        "ExampleWorkspace@0.1.0"
    );
    assert!(acquisition.context.locked);
    assert!(acquisition.context.offline);
    assert!(!temp.path().join("build-ran").exists());
    assert_eq!(before[0], fs::read(temp.path().join("Cargo.toml")).unwrap());
    assert_eq!(before[1], fs::read(temp.path().join("Cargo.lock")).unwrap());
    assert_eq!(before[2], fs::read(temp.path().join("build.rs")).unwrap());
    assert_eq!(before[3], fs::read(temp.path().join("src/lib.rs")).unwrap());
}

#[test]
fn limits_are_fixed_and_within_the_delivery_contract() {
    fn assert_bounded(timeout: Duration, output_limit: usize) {
        assert!(timeout <= Duration::from_secs(30));
        assert!(output_limit <= 16 * 1024 * 1024);
    }
    assert_bounded(CARGO_METADATA_TIMEOUT, CARGO_METADATA_OUTPUT_LIMIT_BYTES);
}

#[test]
fn command_path_bytes_remain_opaque_to_public_failures() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let mut runner = RecordingRunner {
        command: None,
        result: Some(Err(CargoMetadataProcessFailure::ProcessFailed)),
    };
    let error = run_with_runner(temp.path(), &mut runner).unwrap_err();
    let debug = format!("{error:?}");
    assert!(!debug.contains(temp.path().to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
fn manifest_symlink_escape_is_rejected_before_runner() {
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let repository = tempdir().unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("Cargo.toml"),
        repository.path().join("Cargo.toml"),
    )
    .unwrap();
    let mut runner = runner_with_stdout(fixture_metadata(repository.path()));

    let error = run_validated_with_runner(repository.path(), Path::new("Cargo.toml"), &mut runner)
        .unwrap_err();

    assert_eq!(
        error,
        CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot
    );
    assert!(runner.command.is_none());
    let reason = serde_json::to_string(&error.public_reason()).unwrap();
    assert!(!reason.contains(outside.path().to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
fn manifest_ancestor_symlink_escape_is_rejected_before_runner() {
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("Cargo.toml"), b"[workspace]\n").unwrap();
    let repository = tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), repository.path().join("linked")).unwrap();
    let mut runner = runner_with_stdout(fixture_metadata(repository.path()));

    let error = run_validated_with_runner(
        repository.path(),
        Path::new("linked/Cargo.toml"),
        &mut runner,
    )
    .unwrap_err();

    assert_eq!(
        error,
        CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot
    );
    assert!(runner.command.is_none());
    let reason = serde_json::to_string(&error.public_reason()).unwrap();
    assert!(!reason.contains(outside.path().to_string_lossy().as_ref()));
}
