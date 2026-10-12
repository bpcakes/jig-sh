//! Nonmutating discovery of pending witnessed transactions for one target:
//! its journal, or an authoritative marker naming it whose journal is
//! missing.
//!
//! Discovery only lets a possible retry reach credential capture. It never
//! creates, locks, or authenticates anything, and the authenticated
//! operation still checks every path, binding, and credential.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result as AnyResult};

use crate::crypto::KdfParams;
use crate::error::classified_recovery;
use crate::{VaultErrorKind, VaultRecovery};

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
        Ok(metadata) if !metadata.is_dir() => {
            reject_recorded_target_collision(root)?;
            return Ok(None);
        }
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

/// Annotate an already-refused non-directory leaf without following it or
/// mutating the target, journal, or witness. Ordinary invalid homes retain
/// their usual diagnostics; only a recorded target needs recovery routing.
pub(super) fn reject_recorded_target_collision(root: &Path) -> AnyResult<()> {
    let Some(target) = final_target(root) else {
        return Ok(());
    };
    let witness = WitnessLocation::for_home(&target)?;
    if transaction_recorded(&witness, &target)? {
        return Err(classified_recovery(
            VaultErrorKind::Io,
            VaultRecovery::StorageConflict,
            "Pending vault target is occupied by a non-directory entry; refusing to replace or follow it.",
        ));
    }
    Ok(())
}

fn transaction_recorded(witness: &WitnessLocation, target: &Path) -> AnyResult<bool> {
    let Some(store) = witness.open_existing()? else {
        return Ok(false);
    };
    store.target_pending(&witness::target_key(target))
}

/// Best-effort, conservative discovery for preflight, including orphan journals.
pub(crate) fn pending_transaction_recorded(home: &Path) -> bool {
    let Some(target) = final_target(home) else {
        return false;
    };
    WitnessLocation::for_home(&target)
        .ok()
        .and_then(|witness| transaction_recorded(&witness, &target).ok())
        .unwrap_or(false)
}

/// Read-only status presentation: only a marker commits to finishing a
/// transaction. A journal left before that point is discarded on retry.
pub(crate) fn pending_transaction_marked(home: &Path) -> AnyResult<bool> {
    let parent = home
        .parent()
        .context("vault home has no parent directory")?;
    let parent = match fs::canonicalize(parent) {
        Ok(parent) => parent,
        // A restore creates its parents before recording a transaction.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("failed to inspect pending vault target"),
    };
    let target = parent.join(
        home.file_name()
            .context("vault home has no directory name")?,
    );
    let witness = WitnessLocation::for_home(&target)?;
    let Some(store) = witness.open_existing()? else {
        return Ok(false);
    };
    store
        .target_has_pending_marker(&witness::target_key(&target))
        .map_err(|error| {
            let message = format!("failed to inspect pending vault transactions: {error}");
            error.context(message)
        })
}

impl VaultStore {
    /// Whether a transaction is recorded for this home.
    pub(crate) fn has_pending_transaction(&self) -> AnyResult<bool> {
        transaction_recorded(&self.witness, &self.root)
    }
}
