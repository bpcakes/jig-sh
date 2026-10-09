//! Destination checks for `jig init`, `jig adopt`, and `jig update`.

use std::path::Path;
use std::{fs, io};

use anyhow::{Context, Result, bail};
use jig_context::RepoContext;
use jig_repository::path;
use jig_repository::path::bootstrap_invocation_cwd;

use super::ANSWERS_FILE;
use super::opts::InitOpts;

pub fn preflight_init_destination(opts: &InitOpts) -> Result<()> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = path::resolve_init_destination(&opts.path, &invocation_cwd)?;
    validate_init_destination(&destination, opts.force)?;
    ensure_init_destination_noreplace_supported(&destination)
}

pub(super) fn ensure_init_destination_noreplace_supported(destination: &Path) -> Result<()> {
    let (existing_ancestor, _) = path::split_existing_ancestor(destination)?;
    path::ensure_atomic_noreplace_publication_supported(&existing_ancestor)
}

pub(super) fn validate_init_destination(path: &Path, force: bool) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect init destination {}", path.display()));
        }
    };
    if !metadata.file_type().is_dir() {
        bail!(
            "Init destination is not a real directory: {}",
            path.display()
        );
    }

    let first_entry = fs::read_dir(path)?
        .next()
        .transpose()
        .with_context(|| format!("Failed to enumerate {}", path.display()))?;
    if first_entry.is_none() || force {
        return Ok(());
    }

    bail!(
        "Init destination is not empty: {}. Re-run with --force to overwrite.",
        path.display()
    );
}

pub(super) fn validate_adopt_destination(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("Adopt destination does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("Adopt destination is not a directory: {}", path.display());
    }
    Ok(())
}

pub(super) fn validate_update_destination(path: &Path) -> Result<()> {
    validate_adopt_destination(path)?;
    let answers_path = path.join(ANSWERS_FILE);
    if !answers_path.exists() {
        bail!(
            "Update destination does not contain {}: {}",
            ANSWERS_FILE,
            path.display()
        );
    }
    Ok(())
}

pub(super) fn reject_newer_declared_contract(path: &Path) -> Result<()> {
    let Ok(contract_version) = RepoContext::declared_contract_version_from_root(path) else {
        // Missing or damaged manifests remain repairable through adopt/update.
        return Ok(());
    };
    if contract_version > jig_context::CURRENT_CONTRACT_VERSION {
        bail!(
            "Refusing to rewrite repository contract {contract_version} with this older Jig runtime, which supports contracts through {}. Install a newer compatible Jig runtime and retry; --force does not permit contract downgrades.",
            jig_context::CURRENT_CONTRACT_VERSION
        );
    }
    Ok(())
}
