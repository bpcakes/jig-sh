//! Durable directory creation for vault homes and the witness.

use std::fs;
use std::path::Path;

use anyhow::Result as AnyResult;

use super::sync_parent_dir;

/// Creates `path` with its missing ancestors and syncs every new directory
/// entry, outermost first, so nothing later made durable inside the tree
/// (such as a pending transaction's audit log or witness marker) can
/// outlive the directories holding it.
pub(super) fn create_dir_all_durable(path: &Path) -> AnyResult<()> {
    let created: Vec<&Path> = path
        .ancestors()
        .take_while(|ancestor| {
            fs::symlink_metadata(ancestor)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        })
        .collect();
    fs::create_dir_all(path)?;
    for directory in created.iter().rev() {
        if let Some(parent) = directory.parent() {
            sync_parent_dir(parent)?;
        }
    }
    Ok(())
}
