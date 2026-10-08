//! Ordered locks for one vault home.

use std::path::PathBuf;

use anyhow::{Context, Result as AnyResult};
use fs4::fs_std::FileExt;

use super::witness::{HeldLock, TargetJournal, TargetLock, WitnessStore};
use super::{LOCK_FILE, VaultStore, lock_file, private_open_options};

impl VaultStore {
    fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    /// Runs `f` under the ordered locks: this home's target lock, the home
    /// lock, then the lock of the vault ID its public header names. Same-ID
    /// copies therefore serialize on one witness lock. Without a readable
    /// header, a referenced pending transaction supplies the ID instead.
    /// An absent restore target needs no home lock: the target lock excludes
    /// operations on that path even after recovery installs the home, and
    /// the ID lock excludes other copies until `f` finishes.
    pub(crate) fn with_lock<T>(&self, f: impl FnOnce() -> AnyResult<T>) -> AnyResult<T> {
        let witness = self.witness.open_or_create()?;
        let target = witness.lock_target(&self.target_key())?;
        if !self.root.exists() {
            let _id = self.lock_operation_id(&witness, &target)?;
            return f();
        }
        let file = private_open_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())
            .context("failed to open vault lock")?;
        crate::acl::clear_file(&file, &self.lock_path())?;
        lock_file(&file)?;
        let result = (|| {
            let _id = self.lock_operation_id(&witness, &target)?;
            f()
        })();
        let unlock = FileExt::unlock(&file);
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(error)) => Err(error).context("failed to unlock vault lock"),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(unlock_error)) => Err(error.context(format!(
                "vault operation failed; additionally failed to unlock vault lock: {unlock_error}"
            ))),
        }
    }

    fn lock_operation_id(
        &self,
        witness: &WitnessStore,
        target: &TargetLock,
    ) -> AnyResult<Option<HeldLock>> {
        let vault_id = match self.header_vault_id_for_lock() {
            Some(id) => Some(id),
            None => match witness
                .classify_target_journal(target)
                .map_err(crate::vault::fail_closed)?
            {
                TargetJournal::Referenced { vault_id, .. } => Some(vault_id),
                TargetJournal::Absent | TargetJournal::Orphan(_) => None,
            },
        };
        // The held target lock keeps this marker stable. Recovery may take
        // reentrant ID guards, but this outer guard survives their release.
        vault_id.map(|id| witness.lock_id(&id)).transpose()
    }
}
