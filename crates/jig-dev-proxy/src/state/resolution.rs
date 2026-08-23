use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{
    LockOutcome, StateStore, ensure_default_state_parent_permissions,
    ensure_state_create_ancestor_is_not_shared_writable, ensure_state_dir_permissions,
    existing_dir_is_empty, path_is_symlink,
};

const STATE_TREE_SCAN_ATTEMPTS: usize = 8;

pub(super) fn ensure_state_dir_has_no_symlinks(path: &Path) -> Result<()> {
    for attempt in 0..STATE_TREE_SCAN_ATTEMPTS {
        match ensure_state_tree_has_no_symlinks(path, path) {
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == ErrorKind::NotFound)
                    && attempt + 1 < STATE_TREE_SCAN_ATTEMPTS =>
            {
                // Atomic state-file replacement can remove a directory entry
                // between read_dir and symlink_metadata. Require a complete,
                // stable scan rather than treating that vanished entry as safe.
                std::thread::yield_now();
            }
            result => return result,
        }
    }
    unreachable!("the final state-tree scan attempt always returns")
}

fn ensure_state_tree_has_no_symlinks(root: &Path, path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!(
                "Proxy state dir {} contains symlink {}. Use a dedicated state directory without symlinks.",
                root.display(),
                entry_path.display()
            );
        }
        if metadata.is_dir() {
            ensure_state_tree_has_no_symlinks(root, &entry_path)?;
        }
    }
    Ok(())
}

impl StateStore {
    /// Resolves an existing proxy state directory without creating one.
    ///
    /// Existing paths go through the same symlink, permission, and replacement
    /// recovery checks as `resolve`. A missing configured path is reported as
    /// `None`, allowing read/stop commands to remain non-mutating.
    pub(crate) fn resolve_existing(explicit: Option<PathBuf>) -> Result<Option<Self>> {
        let candidate = if let Some(path) = explicit.as_ref() {
            path.clone()
        } else if let Ok(path) = std::env::var("JIG_PROXY_STATE_DIR") {
            PathBuf::from(path)
        } else {
            dirs::home_dir()
                .context("Could not resolve home directory for Jig proxy state")?
                .join(".jig/proxy")
        };
        if path_is_symlink(&candidate)? {
            // Delegate to the normal resolver so existing-path diagnostics stay
            // identical and no alternate symlink path is accepted here.
            return Self::resolve(explicit).map(Some);
        }
        if !candidate.try_exists().with_context(|| {
            format!(
                "Failed to inspect Jig proxy state dir {}",
                candidate.display()
            )
        })? {
            return Ok(None);
        }
        Self::resolve(explicit).map(Some)
    }

    pub(crate) fn resolve(explicit: Option<PathBuf>) -> Result<Self> {
        match Self::resolve_interruptible(explicit, &|| false)? {
            LockOutcome::Acquired(store) => Ok(store),
            LockOutcome::Cancelled => {
                anyhow::bail!("uncancelled proxy state resolution was cancelled")
            }
        }
    }

    pub(crate) fn resolve_interruptible(
        explicit: Option<PathBuf>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<Self>> {
        let (root, can_chmod_existing) = if let Some(path) = explicit {
            (path, false)
        } else if let Ok(path) = std::env::var("JIG_PROXY_STATE_DIR") {
            (PathBuf::from(path), false)
        } else {
            (
                dirs::home_dir()
                    .context("Could not resolve home directory for Jig proxy state")?
                    .join(".jig/proxy"),
                true,
            )
        };
        if path_is_symlink(&root)? {
            anyhow::bail!(
                "Proxy state dir {} must not be a symlink. Use a dedicated real directory.",
                root.display()
            );
        }
        ensure_state_create_ancestor_is_not_shared_writable(&root)?;
        let default_parent_existed = can_chmod_existing
            && root
                .parent()
                .is_some_and(|parent| parent.try_exists().unwrap_or(false));
        let existed = root.exists();
        fs::create_dir_all(&root)
            .with_context(|| format!("Failed to create proxy state dir {}", root.display()))?;
        if path_is_symlink(&root)? {
            anyhow::bail!(
                "Proxy state dir {} became a symlink while it was being prepared. Use a dedicated real directory.",
                root.display()
            );
        }
        let root = fs::canonicalize(&root)
            .with_context(|| format!("Failed to resolve proxy state dir {}", root.display()))?;
        if can_chmod_existing {
            ensure_default_state_parent_permissions(&root, default_parent_existed)?;
        }
        let can_chmod_root = can_chmod_existing || !existed || existing_dir_is_empty(&root);
        ensure_state_dir_has_no_symlinks(&root)?;
        ensure_state_dir_permissions(&root, can_chmod_root)?;
        ensure_state_dir_has_no_symlinks(&root)?;
        if cancelled() {
            return Ok(LockOutcome::Cancelled);
        }
        Ok(LockOutcome::Acquired(Self {
            root,
            can_chmod_root,
        }))
    }
}
