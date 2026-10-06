//! Nonmutating discovery of pending witnessed transactions for one target.
//!
//! Discovery only lets a possible retry reach credential capture. It never
//! creates, locks, or authenticates anything, and the authenticated
//! operation still checks every path, binding, and credential.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result as AnyResult;

use crate::crypto::KdfParams;

use super::VaultStore;
use super::witness::{self, WitnessLocation};

/// The physical final-target path of `root`: canonical parent plus leaf.
fn final_target(root: &Path) -> Option<PathBuf> {
    let parent = fs::canonicalize(root.parent()?).ok()?;
    Some(parent.join(root.file_name()?))
}

/// Keeps an absent home absent when a transaction journal is recorded for
/// it: a pending restore installs it without replacement, so resolving the
/// home must not create it.
pub(super) fn pending_absent_target(
    root: &Path,
    initialization_kdf: &KdfParams,
) -> AnyResult<Option<VaultStore>> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Ok(None),
    }
    let Some(target) = final_target(root) else {
        return Ok(None);
    };
    let witness = WitnessLocation::for_home(&target)?;
    if !journal_recorded(&witness, &target)? {
        return Ok(None);
    }
    witness.ensure_disjoint(&target)?;
    Ok(Some(VaultStore::at(
        target,
        initialization_kdf.clone(),
        witness,
    )))
}

fn journal_recorded(witness: &WitnessLocation, target: &Path) -> AnyResult<bool> {
    let Some(store) = witness.open_existing()? else {
        return Ok(false);
    };
    Ok(store.journal_exists(&witness::target_key(target)))
}

/// Best-effort, read-only probe for status reporting: whether a transaction
/// journal is recorded for this home.
pub(crate) fn pending_transaction_recorded(home: &Path) -> bool {
    let Some(target) = final_target(home) else {
        return false;
    };
    WitnessLocation::for_home(&target)
        .ok()
        .and_then(|witness| journal_recorded(&witness, &target).ok())
        .unwrap_or(false)
}

impl VaultStore {
    /// Whether a transaction journal is recorded for this home.
    pub(crate) fn has_pending_journal(&self) -> AnyResult<bool> {
        journal_recorded(&self.witness, &self.root)
    }
}
