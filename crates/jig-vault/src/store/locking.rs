//! Ordered locks for one vault home.

use std::path::PathBuf;

use anyhow::{Context, Result as AnyResult};
use fs4::fs_std::FileExt;

use super::{LOCK_FILE, VaultStore, lock_file, private_open_options};

impl VaultStore {
    fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    /// Runs `f` under the ordered locks: this home's target lock, the home
    /// lock, then the lock of the vault ID its public header names. Same-ID
    /// copies therefore serialize on one witness lock. An absent home kept
    /// for a pending restore has no home lock yet; its target lock alone
    /// serializes every operation on the path until recovery installs it.
    pub(crate) fn with_lock<T>(&self, f: impl FnOnce() -> AnyResult<T>) -> AnyResult<T> {
        let witness = self.witness.open_or_create()?;
        let _target = witness.lock_target(&self.target_key())?;
        if !self.root.exists() {
            let _id = match self.header_vault_id_for_lock() {
                Some(vault_id) => Some(witness.lock_id(&vault_id)?),
                None => None,
            };
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
            let _id = match self.header_vault_id_for_lock() {
                Some(vault_id) => Some(witness.lock_id(&vault_id)?),
                None => None,
            };
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
}
