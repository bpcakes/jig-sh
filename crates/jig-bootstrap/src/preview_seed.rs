use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use jig_policy::agent_guides::is_ignored_guide_component;

use super::file_copy::{
    copy_file_or_symlink_with_permissions, prepare_copy_destination_and_read_metadata,
};

pub(super) fn seed_preview_workspace(source_root: &Path, destination_root: &Path) -> Result<()> {
    fs::create_dir_all(destination_root)
        .with_context(|| format!("Failed to create {}", destination_root.display()))?;
    copy_agent_guides_recursive(source_root, destination_root, source_root)
}

fn copy_agent_guides_recursive(
    source_root: &Path,
    destination_root: &Path,
    current_source: &Path,
) -> Result<()> {
    for entry in fs::read_dir(current_source)? {
        let entry = entry?;
        let source_path = entry.path();
        let relative = source_path.strip_prefix(source_root).with_context(|| {
            format!(
                "{} is not under {}",
                source_path.display(),
                source_root.display()
            )
        })?;
        if relative.components().any(is_ignored_guide_component) {
            continue;
        }
        let destination_path = destination_root.join(relative);
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("Failed to create {}", destination_path.display()))?;
            copy_agent_guides_recursive(source_root, destination_root, &source_path)?;
            continue;
        }

        let file_name = source_path.file_name().and_then(|name| name.to_str());
        if file_name != Some("AGENTS.md") {
            continue;
        }

        copy_preview_guide(&source_path, &destination_path)?;
    }
    Ok(())
}

fn copy_preview_guide(source_path: &Path, destination_path: &Path) -> Result<()> {
    let metadata = prepare_copy_destination_and_read_metadata(source_path, destination_path)?;
    copy_file_or_symlink_with_permissions(source_path, destination_path, &metadata)
}
