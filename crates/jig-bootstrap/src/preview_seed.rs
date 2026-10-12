use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result};

use super::file_copy::{
    copy_file_or_symlink_with_permissions, prepare_copy_destination_and_read_metadata,
};

/// Seeds the staging render with the repository's own guides so the generated
/// agent map lists them. Only guides that belong to the repository are seeded:
/// ignored scratch trees and nested repositories never reach the map.
pub(super) fn seed_preview_workspace(source_root: &Path, destination_root: &Path) -> Result<()> {
    fs::create_dir_all(destination_root)
        .with_context(|| format!("Failed to create {}", destination_root.display()))?;
    for guide in jig_policy::list_agent_guides(source_root)? {
        let source_path = source_root.join(&guide);
        // Git still lists a tracked guide whose deletion is not staged.
        match fs::symlink_metadata(&source_path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to inspect {}", source_path.display()));
            }
        }
        let destination_path = destination_root.join(&guide);
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        copy_preview_guide(&source_path, &destination_path)?;
    }
    Ok(())
}

fn copy_preview_guide(source_path: &Path, destination_path: &Path) -> Result<()> {
    let metadata = prepare_copy_destination_and_read_metadata(source_path, destination_path)?;
    copy_file_or_symlink_with_permissions(source_path, destination_path, &metadata)
}
