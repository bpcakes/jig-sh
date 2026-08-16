use jig_vault::{FieldKind, VaultWriteMode};
use jig_vault_tui::{VaultAction, VaultActionResult, VaultBackend};
use secrecy::SecretString;

use super::*;
use crate::command::{VaultRuntimeOptions, VaultTuiRequest};

mod core;
mod lifecycle;

fn request(home: std::path::PathBuf) -> VaultTuiRequest {
    VaultTuiRequest {
        vault: VaultRuntimeOptions {
            home: Some(home),
            ..Default::default()
        },
    }
}

#[cfg(unix)]
fn lifecycle_backup_path(temp: &tempfile::TempDir) -> std::path::PathBuf {
    // Recompute the owned action path so the Linux-only restore use can be
    // compiled out on macOS without leaving a redundant final clone.
    temp.path().join("vault.backup")
}

fn mutate(
    backend: &VaultTuiBackend,
    snapshot: &VaultSnapshot,
    mutation: VaultMutation,
) -> std::result::Result<VaultSnapshot, VaultUiError> {
    let result = backend.execute(VaultAction::Mutate {
        revision: snapshot.revision.clone(),
        mutation,
    })?;
    let VaultActionResult::Snapshot(snapshot) = result else {
        panic!("vault mutation did not return a snapshot");
    };
    Ok(snapshot)
}
