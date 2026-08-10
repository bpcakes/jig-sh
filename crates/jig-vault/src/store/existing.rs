use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Context, Result as AnyResult, bail};

use crate::crypto::KdfParams;
use crate::{Result, VaultError, VaultErrorKind};

use super::{AUDIT_FILE, VAULT_FILE, VaultStore, ensure_tree_has_no_symlinks};

impl VaultStore {
    /// Opens an already-existing vault home without creating or chmodding any
    /// directory, state file, audit file, or lock file.
    pub(crate) fn open_existing(root: PathBuf) -> Result<Self> {
        open_existing_private_dir(root)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))
    }

    pub(crate) fn revalidate_existing(&self) -> AnyResult<()> {
        validate_existing_private_dir(&self.root)
    }
}

fn open_existing_private_dir(root: PathBuf) -> AnyResult<VaultStore> {
    validate_existing_private_dir(&root)?;
    let root = fs::canonicalize(&root)
        .with_context(|| format!("failed to canonicalize vault home {}", root.display()))?;
    validate_existing_private_dir(&root)?;
    Ok(VaultStore {
        root,
        initialization_kdf: KdfParams::production(),
        #[cfg(test)]
        fail_next_vault_write: Arc::new(AtomicBool::new(false)),
    })
}

fn validate_existing_private_dir(root: &Path) -> AnyResult<()> {
    reject_symlinked_path_components(root)?;
    let metadata = fs::symlink_metadata(root)
        .with_context(|| format!("failed to inspect existing vault home {}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "existing vault home must be a real directory: {}",
            root.display()
        );
    }
    ensure_owned_private_directory(root, &metadata)?;
    ensure_tree_has_no_symlinks(root, root)?;
    for path in [root.join(VAULT_FILE), root.join(AUDIT_FILE)] {
        let metadata = fs::symlink_metadata(&path).with_context(|| {
            format!("existing vault is missing required file {}", path.display())
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!(
                "existing vault file must be a regular non-symlink file: {}",
                path.display()
            );
        }
        ensure_owned_private_file(&path, &metadata)?;
    }
    Ok(())
}

fn reject_symlinked_path_components(path: &Path) -> AnyResult<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for existing vault")?
            .join(path)
    };
    let mut ancestors = absolute.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for ancestor in ancestors {
        let metadata = fs::symlink_metadata(ancestor)
            .with_context(|| format!("failed to inspect path component {}", ancestor.display()))?;
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing existing vault through symlinked path component {}",
                ancestor.display()
            );
        }
    }
    Ok(())
}

fn ensure_owned_private_directory(path: &Path, metadata: &fs::Metadata) -> AnyResult<()> {
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            bail!(
                "existing vault home permissions are {mode:o}; expected owner-only permissions for {}",
                path.display()
            );
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!(
                "existing vault home is not owned by the current user: {}",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (path, metadata);
    Ok(())
}

fn ensure_owned_private_file(path: &Path, metadata: &fs::Metadata) -> AnyResult<()> {
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            bail!(
                "existing vault file permissions are {mode:o}; expected owner-only permissions for {}",
                path.display()
            );
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!(
                "existing vault file is not owned by the current user: {}",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (path, metadata);
    Ok(())
}
