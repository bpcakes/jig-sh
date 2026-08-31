use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use super::{EntryKind, RepositoryFileLeaf, StoredEntry, path};

pub(super) fn prepare_private_metadata_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        let metadata = fs::symlink_metadata(parent)
            .with_context(|| format!("Git metadata parent is unavailable: {}", parent.display()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "Git metadata parent is not a real directory: {}",
                parent.display()
            );
        }
    }
    match fs::create_dir(path) {
        Ok(()) => set_private_directory_permissions(path)?,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            reject_symlink(path, false)?;
        }
        Err(error) => return Err(error.into()),
    }
    sync_parent(path)
}

pub(super) fn create_private_directory(path: &Path) -> Result<()> {
    fs::create_dir(path).with_context(|| format!("Failed to create {}", path.display()))?;
    set_private_directory_permissions(path)?;
    sync_parent(path)
}

pub(super) fn cleanup_journal(metadata_root: &Path, journal: &Path) -> Result<()> {
    reject_symlink(journal, false)?;
    let cleanup = metadata_root.join(format!(".transaction-cleanup-{}", Ulid::new()));
    fs::rename(journal, &cleanup)?;
    sync_directory(metadata_root)?;
    fs::remove_dir_all(&cleanup)?;
    sync_directory(metadata_root)
}

pub(super) fn reject_symlink(path: &Path, allow_missing: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("Unsafe symlink at {}", path.display())
        }
        Ok(metadata) if !allow_missing && !metadata.is_dir() => {
            bail!("Expected a real directory at {}", path.display())
        }
        Ok(_) => Ok(()),
        Err(error) if allow_missing && error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("Unsafe repository update path: {}", path.display());
    }
    path::validate_no_reserved_git_metadata_components(path)
}

pub(super) fn relative_path_text(path: &Path) -> Result<String> {
    validate_relative_path(path)?;
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .context("Repository update paths must be UTF-8")
}

pub(super) fn leaf_for_entry(entry: &StoredEntry) -> RepositoryFileLeaf {
    match entry.kind {
        EntryKind::Missing => RepositoryFileLeaf::Missing,
        EntryKind::Regular => RepositoryFileLeaf::RegularFile,
        EntryKind::Symlink => RepositoryFileLeaf::Symlink,
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(unix)]
pub(super) fn permission_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
pub(super) fn permission_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
const fn private_file_mode() -> u32 {
    0o600
}

#[cfg(not(unix))]
const fn private_file_mode() -> u32 {
    0
}

#[cfg(unix)]
pub(super) fn permissions_from_mode(mode: u32) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(mode)
}

#[cfg(not(unix))]
pub(super) fn permissions_from_mode(_mode: u32) -> fs::Permissions {
    fs::metadata(".")
        .expect("current directory has metadata")
        .permissions()
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_private_file_permissions(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_private_file_permissions(_file: &File) -> Result<()> {
    Ok(())
}

pub(super) fn write_synced(path: PathBuf, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    let mut file = options
        .open(&path)
        .with_context(|| format!("Failed to create {}", path.display()))?;
    set_private_file_permissions(&file)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    sync_parent(&path)
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

pub(super) fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn os_path_bytes(path: &Path) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(path.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
pub(super) fn os_path_bytes(path: &Path) -> Result<Vec<u8>> {
    Ok(path
        .to_str()
        .context("Symlink targets must be UTF-8 on this platform")?
        .as_bytes()
        .to_vec())
}

pub(super) fn create_payload_symlink(
    path: &Path,
    target: &Path,
    target_is_directory: bool,
) -> Result<()> {
    #[cfg(unix)]
    {
        let _ = target_is_directory;
        std::os::unix::fs::symlink(target, path)?;
    }
    #[cfg(windows)]
    {
        if target_is_directory {
            std::os::windows::fs::symlink_dir(target, path)?;
        } else {
            std::os::windows::fs::symlink_file(target, path)?;
        }
    }
    sync_parent(path)
}

pub(super) fn private_payload_matches_mode(metadata: &fs::Metadata) -> bool {
    permission_mode(metadata) == private_file_mode()
}
