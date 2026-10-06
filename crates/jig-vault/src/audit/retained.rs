//! The audit root a retained reveal, exec, broker, or backup handle
//! authenticated, bound to the vault identity it unlocked. Audit-only
//! appends check the witness for that identity instead of letting the
//! current, unauthenticated public header choose which checks apply.

use zeroize::Zeroizing;

use crate::crypto::KEY_LEN;

pub(crate) struct RetainedAuditKey {
    key: Zeroizing<[u8; KEY_LEN]>,
    vault_id: String,
}

impl RetainedAuditKey {
    pub(crate) fn new(key: Zeroizing<[u8; KEY_LEN]>, vault_id: String) -> Self {
        Self { key, vault_id }
    }

    pub(crate) fn key(&self) -> &[u8] {
        self.key.as_ref()
    }

    /// The vault ID authenticated when the handle was opened.
    pub(crate) fn vault_id(&self) -> &str {
        &self.vault_id
    }
}
