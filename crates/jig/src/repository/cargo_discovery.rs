//! Bounded, read-only Cargo metadata acquisition.
//!
//! Cargo is invoked directly through the repository's owned-process boundary.
//! The command specification is kept private so the absolute manifest path
//! and Cargo's raw process output cannot become plan or error data.  The
//! private runner seam makes the acquisition policy testable without making
//! process behavior part of the repository API.  The planner calls this module
//! only for affected Rust plans and keeps its normalized result private.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use jig_contract::{CargoImpactContextV1, CargoImpactReasonV1};
use jig_rust::{
    CargoMetadataErrorV1, CargoMetadataGraphV1, CargoMetadataLimitsV1, normalize_cargo_metadata_v1,
};

use crate::{
    context::CommandOutputLimit,
    execution::{ExecutionControl, SupervisedExecutionError, run_supervised_execution_command},
};

const CARGO_METADATA_PROGRAM: &str = "cargo";
const CARGO_METADATA_LABEL: &str = "cargo metadata";
const CARGO_METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const CARGO_METADATA_OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Normalized Cargo facts and the acquisition authority used to obtain them.
#[derive(Debug)]
pub(crate) struct CargoMetadataAcquisition {
    pub(crate) graph: CargoMetadataGraphV1,
    pub(crate) context: CargoImpactContextV1,
}

/// Stable internal acquisition failures.  No variant stores process text,
/// absolute paths, package IDs, or other Cargo-controlled opaque data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CargoMetadataAcquisitionError {
    MissingManifest,
    UnsupportedWorkspaceRoot,
    ProgramUnavailable,
    ProcessFailed,
    NonZeroExit,
    TimedOut,
    OutputLimitExceeded,
    MetadataValidation(CargoMetadataErrorV1),
    CancelledBeforeStart,
    Cancelled,
    UnsupportedContext,
}

impl CargoMetadataAcquisitionError {
    /// Map a discovery failure to the portable reason used by impact records.
    /// Cancellation deliberately has no fallback reason: callers
    /// must propagate it as a cancelled request.
    pub(crate) const fn public_reason(self) -> Option<CargoImpactReasonV1> {
        match self {
            Self::UnsupportedContext => Some(CargoImpactReasonV1::UnsupportedContext),
            Self::CancelledBeforeStart | Self::Cancelled => None,
            Self::MetadataValidation(error) => Some(match error {
                CargoMetadataErrorV1::DuplicateSelector => CargoImpactReasonV1::DuplicateSelector,
                CargoMetadataErrorV1::IncompleteResolve
                | CargoMetadataErrorV1::UnknownPackage
                | CargoMetadataErrorV1::UnknownWorkspaceMember
                | CargoMetadataErrorV1::MissingResolveNode
                | CargoMetadataErrorV1::MissingDependencyKinds => {
                    CargoImpactReasonV1::IncompleteResolve
                }
                CargoMetadataErrorV1::InvalidRepositoryRoot => {
                    CargoImpactReasonV1::UnsupportedWorkspaceRoot
                }
                CargoMetadataErrorV1::InvalidJson
                | CargoMetadataErrorV1::UnsupportedFormat { .. }
                | CargoMetadataErrorV1::MissingField(_)
                | CargoMetadataErrorV1::InvalidString(_)
                | CargoMetadataErrorV1::InvalidPath(_)
                | CargoMetadataErrorV1::DuplicatePackageId
                | CargoMetadataErrorV1::DuplicateResolveNode
                | CargoMetadataErrorV1::DuplicateWorkspaceMember => {
                    CargoImpactReasonV1::MetadataMalformed
                }
                CargoMetadataErrorV1::MetadataTooLarge { .. }
                | CargoMetadataErrorV1::LimitExceeded { .. } => {
                    CargoImpactReasonV1::MetadataResourceLimitExceeded
                }
            }),
            Self::MissingManifest => Some(CargoImpactReasonV1::WorkspaceManifestMissing),
            Self::UnsupportedWorkspaceRoot => Some(CargoImpactReasonV1::UnsupportedWorkspaceRoot),
            Self::ProgramUnavailable => Some(CargoImpactReasonV1::CargoProgramUnavailable),
            Self::NonZeroExit => Some(CargoImpactReasonV1::MetadataCommandNonZero),
            Self::ProcessFailed => Some(CargoImpactReasonV1::MetadataProcessFailure),
            Self::TimedOut => Some(CargoImpactReasonV1::MetadataTimeout),
            Self::OutputLimitExceeded => Some(CargoImpactReasonV1::MetadataOutputLimitExceeded),
        }
    }

    pub(crate) const fn is_cancellation(self) -> bool {
        matches!(self, Self::CancelledBeforeStart | Self::Cancelled)
    }
}

#[derive(Clone, Debug)]
struct CargoMetadataCommand {
    program: OsString,
    args: Vec<OsString>,
    current_dir: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl CargoMetadataCommand {
    fn new(
        repository_root: &Path,
        absolute_manifest: &Path,
        context: &CargoImpactContextV1,
    ) -> Self {
        let mut command = Self {
            program: OsString::from(CARGO_METADATA_PROGRAM),
            args: vec![
                OsString::from("metadata"),
                OsString::from("--format-version"),
                OsString::from("1"),
                // Discovery never receives execution's permission to update
                // Cargo.lock. Cargo must reject missing/stale lock authority
                // before writing it, including read-only explain requests.
                OsString::from("--locked"),
            ],
            current_dir: repository_root.to_path_buf(),
            env: vec![(
                OsString::from("CARGO_NET_OFFLINE"),
                OsString::from(if context.offline { "true" } else { "false" }),
            )],
        };
        if context.offline {
            command.args.push("--offline".into());
        }
        command.args.extend([
            "--manifest-path".into(),
            absolute_manifest.as_os_str().to_owned(),
        ]);
        if let Some(target) = &context.target {
            command
                .args
                .extend(["--filter-platform".into(), target.into()]);
        }
        for feature in &context.features {
            command.args.extend(["--features".into(), feature.into()]);
        }
        if context.all_features {
            command.args.push("--all-features".into());
        }
        if context.no_default_features {
            command.args.push("--no-default-features".into());
        }
        command
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CargoMetadataProcessOutput {
    succeeded: bool,
    stdout: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CargoMetadataProcessFailure {
    CancelledBeforeStart,
    Cancelled,
    TimedOut,
    OutputLimitExceeded,
    ProgramUnavailable,
    ProcessFailed,
}

trait CargoMetadataRunner {
    fn run(
        &mut self,
        command: &CargoMetadataCommand,
        observer: &mut dyn ExecutionControl,
    ) -> Result<CargoMetadataProcessOutput, CargoMetadataProcessFailure>;
}

struct OwnedCargoMetadataRunner;

impl CargoMetadataRunner for OwnedCargoMetadataRunner {
    fn run(
        &mut self,
        command: &CargoMetadataCommand,
        observer: &mut dyn ExecutionControl,
    ) -> Result<CargoMetadataProcessOutput, CargoMetadataProcessFailure> {
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(&command.current_dir);
        for (key, value) in &command.env {
            process.env(key, value);
        }
        let output = run_supervised_execution_command(
            &mut process,
            CARGO_METADATA_TIMEOUT,
            CommandOutputLimit::from_bytes(CARGO_METADATA_OUTPUT_LIMIT_BYTES as u64)
                .expect("Cargo metadata output limit is valid"),
            CARGO_METADATA_LABEL,
            observer,
        )
        .map_err(map_supervised_failure)?;
        Ok(CargoMetadataProcessOutput {
            succeeded: output.status.success(),
            stdout: output.stdout,
        })
    }
}

fn map_supervised_failure(error: SupervisedExecutionError) -> CargoMetadataProcessFailure {
    match error {
        SupervisedExecutionError::CancelledBeforeStart => {
            CargoMetadataProcessFailure::CancelledBeforeStart
        }
        SupervisedExecutionError::Cancelled => CargoMetadataProcessFailure::Cancelled,
        SupervisedExecutionError::TimedOut => CargoMetadataProcessFailure::TimedOut,
        SupervisedExecutionError::OutputLimitExceeded { .. } => {
            CargoMetadataProcessFailure::OutputLimitExceeded
        }
        SupervisedExecutionError::Failed {
            process_started: false,
            ..
        } => CargoMetadataProcessFailure::ProgramUnavailable,
        SupervisedExecutionError::Failed {
            process_started: true,
            ..
        } => CargoMetadataProcessFailure::ProcessFailed,
    }
}

/// Acquire and normalize Cargo metadata for a manifest below an absolute
/// repository root.  The production path performs no writes and no retries.
pub(crate) fn acquire_cargo_metadata(
    repository_root: &Path,
    workspace_manifest: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    acquire_cargo_metadata_in_context(
        repository_root,
        workspace_manifest,
        &CargoImpactContextV1::default(),
        observer,
    )
}

/// Acquire metadata under the typed feature, platform and network policy.
/// Discovery always locks the existing resolution, independently from execution's
/// permission to update it; the returned context reports that actual authority.
pub(crate) fn acquire_cargo_metadata_in_context(
    repository_root: &Path,
    workspace_manifest: &Path,
    context: &CargoImpactContextV1,
    observer: &mut dyn ExecutionControl,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    if observer.cancelled() {
        return Err(CargoMetadataAcquisitionError::CancelledBeforeStart);
    }
    validate_context(context)?;
    let absolute_manifest = validate_manifest(repository_root, workspace_manifest)?;
    let mut runner = OwnedCargoMetadataRunner;
    acquire_cargo_metadata_in_context_with_runner(
        repository_root,
        &absolute_manifest,
        context,
        observer,
        &mut runner,
    )
}

#[cfg(test)]
fn acquire_cargo_metadata_with_runner(
    repository_root: &Path,
    absolute_manifest: &Path,
    observer: &mut dyn ExecutionControl,
    runner: &mut dyn CargoMetadataRunner,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    acquire_cargo_metadata_in_context_with_runner(
        repository_root,
        absolute_manifest,
        &CargoImpactContextV1::default(),
        observer,
        runner,
    )
}

fn acquire_cargo_metadata_in_context_with_runner(
    repository_root: &Path,
    absolute_manifest: &Path,
    context: &CargoImpactContextV1,
    observer: &mut dyn ExecutionControl,
    runner: &mut dyn CargoMetadataRunner,
) -> Result<CargoMetadataAcquisition, CargoMetadataAcquisitionError> {
    if observer.cancelled() {
        return Err(CargoMetadataAcquisitionError::CancelledBeforeStart);
    }
    validate_context(context)?;
    let command = CargoMetadataCommand::new(repository_root, absolute_manifest, context);
    let output = runner
        .run(&command, observer)
        .map_err(map_process_failure)?;
    if observer.cancelled() {
        return Err(CargoMetadataAcquisitionError::Cancelled);
    }
    if !output.succeeded {
        return Err(CargoMetadataAcquisitionError::NonZeroExit);
    }
    let graph = normalize_cargo_metadata_v1(
        &output.stdout,
        repository_root,
        CargoMetadataLimitsV1::default(),
    )
    .map_err(CargoMetadataAcquisitionError::MetadataValidation)?;
    if observer.cancelled() {
        return Err(CargoMetadataAcquisitionError::Cancelled);
    }
    Ok(CargoMetadataAcquisition {
        graph,
        context: CargoImpactContextV1 {
            locked: true,
            ..context.clone()
        },
    })
}

fn validate_context(context: &CargoImpactContextV1) -> Result<(), CargoMetadataAcquisitionError> {
    let valid_token = |value: &str| {
        !value.is_empty()
            && value.len() <= 4096
            && !value.starts_with('-')
            && !value
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace() || ch == ',')
    };
    if context.metadata_format_version != 1
        || context.features.len() > 256
        || context.features.iter().map(String::len).sum::<usize>() > 65_536
        || context.features.iter().any(|feature| !valid_token(feature))
        || context
            .target
            .as_ref()
            .is_some_and(|target| !valid_token(target))
    {
        return Err(CargoMetadataAcquisitionError::UnsupportedContext);
    }
    Ok(())
}

fn validate_manifest(
    repository_root: &Path,
    workspace_manifest: &Path,
) -> Result<PathBuf, CargoMetadataAcquisitionError> {
    if !repository_root.is_absolute() || !repository_root.is_dir() {
        return Err(CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot);
    }
    let absolute_manifest = if workspace_manifest.is_absolute() {
        workspace_manifest.to_path_buf()
    } else {
        repository_root.join(workspace_manifest)
    };
    if !absolute_manifest.is_absolute()
        || absolute_manifest
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || absolute_manifest.strip_prefix(repository_root).is_err()
    {
        return Err(CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot);
    }
    let canonical_root = fs::canonicalize(repository_root)
        .map_err(|_| CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot)?;
    let canonical_manifest = match fs::canonicalize(&absolute_manifest) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CargoMetadataAcquisitionError::MissingManifest);
        }
        Err(_) => return Err(CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot),
    };
    if canonical_manifest.strip_prefix(canonical_root).is_err() {
        return Err(CargoMetadataAcquisitionError::UnsupportedWorkspaceRoot);
    }
    if !canonical_manifest.is_file() {
        return Err(CargoMetadataAcquisitionError::MissingManifest);
    }
    Ok(absolute_manifest)
}

fn map_process_failure(error: CargoMetadataProcessFailure) -> CargoMetadataAcquisitionError {
    match error {
        CargoMetadataProcessFailure::CancelledBeforeStart => {
            CargoMetadataAcquisitionError::CancelledBeforeStart
        }
        CargoMetadataProcessFailure::Cancelled => CargoMetadataAcquisitionError::Cancelled,
        CargoMetadataProcessFailure::TimedOut => CargoMetadataAcquisitionError::TimedOut,
        CargoMetadataProcessFailure::OutputLimitExceeded => {
            CargoMetadataAcquisitionError::OutputLimitExceeded
        }
        CargoMetadataProcessFailure::ProgramUnavailable => {
            CargoMetadataAcquisitionError::ProgramUnavailable
        }
        CargoMetadataProcessFailure::ProcessFailed => CargoMetadataAcquisitionError::ProcessFailed,
    }
}

#[cfg(test)]
#[path = "cargo_acceptance_tests.rs"]
mod acceptance_tests;
#[cfg(test)]
#[path = "cargo_discovery_context_tests.rs"]
mod context_tests;
#[cfg(test)]
#[path = "cargo_discovery_tests.rs"]
mod tests;
