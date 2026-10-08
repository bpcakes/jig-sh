use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

pub(super) fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

pub(super) fn prepare_copy_destination_and_read_metadata(
    source_path: &Path,
    destination_path: &Path,
) -> Result<fs::Metadata> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    fs::symlink_metadata(source_path)
        .with_context(|| format!("Failed to stat {}", source_path.display()))
}

pub(super) fn copy_file_or_symlink_with_permissions(
    source_path: &Path,
    destination_path: &Path,
    metadata: &fs::Metadata,
) -> Result<()> {
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        let target = fs::read_link(source_path)
            .with_context(|| format!("Failed to read symlink {}", source_path.display()))?;
        create_symlink(&target, destination_path)?;
        return Ok(());
    }

    fs::copy(source_path, destination_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source_path.display(),
            destination_path.display()
        )
    })?;
    fs::set_permissions(destination_path, metadata.permissions()).with_context(|| {
        format!(
            "Failed to set permissions on {}",
            destination_path.display()
        )
    })?;
    Ok(())
}

#[cfg(unix)]
pub(super) fn create_symlink(target: &Path, link_path: &Path) -> Result<()> {
    use std::os::unix::fs as unix_fs;

    if path_exists(link_path) {
        fs::remove_file(link_path)
            .with_context(|| format!("Failed to remove {}", link_path.display()))?;
    }
    unix_fs::symlink(target, link_path).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            link_path.display(),
            target.display()
        )
    })?;
    Ok(())
}
