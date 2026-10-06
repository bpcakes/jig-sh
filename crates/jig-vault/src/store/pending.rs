//! Nonmutating discovery of pending witnessed transactions for one target:
//! its journal, or an authoritative marker naming it whose journal is
//! missing.
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

/// Keeps an absent home absent when a transaction is recorded for it: a
/// pending restore installs it without replacement, so resolving the home
/// must not create it, even when the transaction can only fail closed
/// because its journal is missing.
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
    if !transaction_recorded(&witness, &target)? {
        return Ok(None);
    }
    witness.ensure_disjoint(&target)?;
    Ok(Some(VaultStore::at(
        target,
        initialization_kdf.clone(),
        witness,
    )))
}

fn transaction_recorded(witness: &WitnessLocation, target: &Path) -> AnyResult<bool> {
    let Some(store) = witness.open_existing()? else {
        return Ok(false);
    };
    store.target_pending(&witness::target_key(target))
}

/// Best-effort, read-only probe for status reporting: whether a transaction
/// is recorded for this home.
pub(crate) fn pending_transaction_recorded(home: &Path) -> bool {
    let Some(target) = final_target(home) else {
        return false;
    };
    WitnessLocation::for_home(&target)
        .ok()
        .and_then(|witness| transaction_recorded(&witness, &target).ok())
        .unwrap_or(false)
}

impl VaultStore {
    /// Whether a transaction is recorded for this home.
    pub(crate) fn has_pending_transaction(&self) -> AnyResult<bool> {
        transaction_recorded(&self.witness, &self.root)
    }
}
