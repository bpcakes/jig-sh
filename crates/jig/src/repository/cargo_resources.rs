//! Resolve opted-in Cargo artifact claims without compiling or exposing paths.
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::Result;
use jig_contract::{ActionRunner, CargoImpactContextV1, ExecutionResourceV1, PlannedTarget};
use sha2::{Digest, Sha256};

use crate::{
    context::{CommandOutputLimit, RepoContext},
    execution::{
        ExecutionCancellation, ExecutionObserver, SupervisedExecutionError,
        run_supervised_execution_command,
    },
    repository_path::{resolve_repository_working_directory, validate_runner_environment},
    state::{ResourceClaim, ResourceClaimMode},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedCargoResources {
    pub(crate) claims: Vec<ResourceClaim>,
    /// A portable explanation: partial claims coordinate only this repository.
    pub(crate) partial_reason: Option<&'static str>,
    identity: Vec<String>,
}

impl ResolvedCargoResources {
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self == other
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CargoResourceStop {
    Cancelled,
    TimedOut,
}

impl fmt::Display for CargoResourceStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "Cargo resource resolution was cancelled",
            Self::TimedOut => "Cargo resource resolution timed out",
        })
    }
}

impl std::error::Error for CargoResourceStop {}

struct Budget<'a> {
    started: Instant,
    timeout: Duration,
    cancelled: &'a dyn Fn() -> bool,
}

impl Budget<'_> {
    fn remaining(&self) -> Result<Duration> {
        if (self.cancelled)() {
            return Err(CargoResourceStop::Cancelled.into());
        }
        let remaining = self.timeout.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            return Err(CargoResourceStop::TimedOut.into());
        }
        Ok(remaining)
    }
}

impl ExecutionObserver for Budget<'_> {}
impl ExecutionCancellation for Budget<'_> {
    fn cancelled(&self) -> bool {
        (self.cancelled)()
    }
}

/// The caller supplies its remaining target budget, not a new per-probe timeout.
pub(crate) fn resolve(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<ResolvedCargoResources> {
    let mut budget = Budget {
        started: Instant::now(),
        timeout,
        cancelled,
    };
    budget.remaining()?;
    let root = fs::canonicalize(ctx.root());
    let physical_root = root.as_ref().ok().and_then(|path| {
        physical_directory_key_in_domain(path, b"jig-cargo-repository-physical-v1").ok()
    });
    let repository_key = physical_root.clone().unwrap_or_else(|| {
        path_key(
            b"jig-cargo-repository-v1",
            root.as_deref().unwrap_or(ctx.root()),
        )
    });
    let partial = |reason| ResolvedCargoResources {
        claims: vec![ResourceClaim {
            opaque_key: repository_key.clone(),
            mode: ResourceClaimMode::Exclusive,
        }],
        partial_reason: Some(reason),
        identity: vec![repository_key.clone()],
    };
    if planned.resources.is_empty() {
        return Ok(ResolvedCargoResources {
            claims: Vec::new(),
            partial_reason: None,
            identity: Vec::new(),
        });
    }
    let Ok(root) = root else {
        budget.remaining()?;
        return Ok(partial("repository_identity_unavailable"));
    };
    let mut keys = Vec::new();
    let mut identity = Vec::new();
    let mut partial_reason = physical_root
        .is_none()
        .then_some("repository_identity_unavailable");
    for declaration in &planned.resources {
        budget.remaining()?;
        let authority =
            serde_json::to_vec(&(declaration, &planned.runner, &planned.prepared_rust_input))?;
        identity.push(format!("{:x}", Sha256::digest(authority)));
        let ExecutionResourceV1::CargoV1 {
            workspace_manifest,
            working_directory,
            context,
        } = declaration;
        let command = metadata_command(
            &root,
            planned,
            workspace_manifest,
            working_directory.as_deref(),
            context,
        );
        let mut command = match command {
            Ok(command) => command,
            Err(reason) => {
                budget.remaining()?;
                partial_reason.get_or_insert(reason);
                continue;
            }
        };
        let remaining = budget.remaining()?;
        let output = run_supervised_execution_command(
            &mut command,
            remaining,
            CommandOutputLimit::from_bytes(16 * 1024 * 1024).expect("valid metadata limit"),
            "Cargo resource metadata",
            &mut budget,
        );
        budget.remaining()?;
        let output = match output {
            Ok(output) if output.status.success() => output,
            Ok(_) => {
                partial_reason.get_or_insert("cargo_metadata_failed");
                continue;
            }
            Err(error) => {
                partial_reason.get_or_insert(metadata_failure(error)?);
                continue;
            }
        };
        let directories = metadata_directories(&output.stdout);
        budget.remaining()?;
        let directories = match directories {
            Ok(directories) => directories,
            Err(reason) => {
                partial_reason.get_or_insert(reason);
                continue;
            }
        };
        for directory in directories {
            budget.remaining()?;
            let canonical = canonical_future_directory(&directory);
            budget.remaining()?;
            let Some(canonical) = canonical else {
                partial_reason.get_or_insert("artifact_directory_identity_unavailable");
                continue;
            };
            // This stable spelling claim bridges an absent directory becoming
            // real. Alone it does not prove physical aliases, hence partial.
            keys.push(path_key(b"jig-cargo-artifact-v1", &canonical));
            match physical_directory_key(&canonical) {
                Ok(key) => keys.push(key),
                Err(reason) => {
                    partial_reason.get_or_insert(reason);
                }
            }
            budget.remaining()?;
        }
    }
    keys.sort();
    keys.dedup();
    identity.sort();
    let mut claims = vec![ResourceClaim {
        opaque_key: repository_key,
        mode: if partial_reason.is_some() {
            ResourceClaimMode::Exclusive
        } else {
            ResourceClaimMode::Shared
        },
    }];
    claims.extend(keys.into_iter().map(|opaque_key| ResourceClaim {
        opaque_key,
        mode: ResourceClaimMode::Exclusive,
    }));
    budget.remaining()?;
    Ok(ResolvedCargoResources {
        claims,
        partial_reason,
        identity,
    })
}

fn metadata_failure(error: SupervisedExecutionError) -> Result<&'static str> {
    match error {
        SupervisedExecutionError::CancelledBeforeStart | SupervisedExecutionError::Cancelled => {
            Err(CargoResourceStop::Cancelled.into())
        }
        SupervisedExecutionError::TimedOut => Err(CargoResourceStop::TimedOut.into()),
        SupervisedExecutionError::OutputLimitExceeded { .. } => Ok("cargo_metadata_output_limit"),
        SupervisedExecutionError::Failed {
            process_started: false,
            ..
        } => Ok("cargo_metadata_unavailable"),
        SupervisedExecutionError::Failed {
            process_started: true,
            ..
        } => {
            // An await, cleanup or capture failure supplies no proof that the
            // metadata process safely completed. Do not admit another child,
            // and do not expose raw probe diagnostics or paths.
            anyhow::bail!("Cargo metadata supervision failed; resource admission stopped")
        }
    }
}

fn metadata_command(
    root: &Path,
    planned: &PlannedTarget,
    manifest: &str,
    working_directory: Option<&str>,
    context: &CargoImpactContextV1,
) -> std::result::Result<Command, &'static str> {
    let manifest_path = Path::new(manifest);
    if manifest_path.is_absolute()
        || manifest_path
            .components()
            .any(|p| matches!(p, Component::ParentDir))
    {
        return Err("workspace_manifest_unavailable");
    }
    let absolute_manifest =
        fs::canonicalize(root.join(manifest_path)).map_err(|_| "workspace_manifest_unavailable")?;
    if !absolute_manifest.starts_with(root) || !absolute_manifest.is_file() {
        return Err("workspace_manifest_unavailable");
    }
    let cwd = resolve_repository_working_directory(root, working_directory)
        .map_err(|_| "working_directory_unavailable")?;
    let empty = BTreeMap::new();
    let (environment, context) = match &planned.runner {
        ActionRunner::Command { environment, .. }
        | ActionRunner::Shell { environment, .. }
        | ActionRunner::Argv { environment, .. } => (environment, context),
        ActionRunner::RustNextestV1 { .. } => (
            &empty,
            planned
                .prepared_rust_input
                .as_ref()
                .map_or(context, |prepared| &prepared.context),
        ),
        ActionRunner::Native { .. } => return Err("unsupported_runner"),
    };
    validate_runner_environment(environment).map_err(|_| "runner_environment_unavailable")?;
    if context.metadata_format_version != 1 {
        return Err("unsupported_cargo_context");
    }
    let mut command = Command::new("cargo");
    command
        .current_dir(cwd)
        .envs(environment)
        .args([
            "metadata",
            "--no-deps",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(absolute_manifest);
    if let Some(target) = &context.target {
        command.arg("--filter-platform").arg(target);
    }
    for feature in &context.features {
        command.arg("--features").arg(feature);
    }
    if context.all_features {
        command.arg("--all-features");
    }
    if context.no_default_features {
        command.arg("--no-default-features");
    }
    Ok(command)
}

fn metadata_directories(bytes: &[u8]) -> std::result::Result<[PathBuf; 2], &'static str> {
    let metadata: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "cargo_metadata_invalid")?;
    let directory = |key| {
        metadata[key]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or("cargo_artifact_directories_unavailable")
    };
    Ok([
        directory("target_directory")?,
        directory("build_directory")?,
    ])
}

/// Resolve aliases without creating output directories. A dangling symlink or
/// inaccessible existing ancestor is unknown authority, not a lexical identity.
fn canonical_future_directory(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() || path.components().any(|p| matches!(p, Component::ParentDir)) {
        return None;
    }
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut canonical = fs::canonicalize(ancestor).ok()?;
                if !canonical.is_dir() {
                    return None;
                }
                for part in suffix.iter().rev() {
                    canonical.push(part);
                }
                return Some(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(ancestor.file_name()?.to_owned());
                ancestor = ancestor.parent()?;
            }
            Err(_) => return None,
        }
    }
}

fn path_key(domain: &[u8], path: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update([0]);
    digest.update(path.as_os_str().as_encoded_bytes());
    format!("{:x}", digest.finalize())
}

/// Existing directory device/inode authority covers physical aliases such as
/// bind mounts. It complements rather than replaces the stable spelling claim.
fn physical_directory_key(path: &Path) -> std::result::Result<String, &'static str> {
    physical_directory_key_in_domain(path, b"jig-cargo-artifact-physical-v1")
}

#[cfg(unix)]
fn physical_directory_key_in_domain(
    path: &Path,
    domain: &[u8],
) -> std::result::Result<String, &'static str> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "artifact_directory_not_created"
        } else {
            "artifact_directory_identity_unavailable"
        }
    })?;
    if !metadata.is_dir() {
        return Err("artifact_directory_identity_unavailable");
    }
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update([0]);
    digest.update(metadata.dev().to_be_bytes());
    digest.update(metadata.ino().to_be_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(not(unix))]
fn physical_directory_key_in_domain(
    _path: &Path,
    _domain: &[u8],
) -> std::result::Result<String, &'static str> {
    Err("artifact_directory_identity_unavailable")
}

#[cfg(test)]
mod tests;
