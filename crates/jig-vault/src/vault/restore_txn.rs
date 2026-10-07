//! Witnessed restore candidates and restore recovery.
//!
//! An explicit restore authorizes recovering older content: the archived
//! state is authenticated directly rather than compared with the witness,
//! then resealed under a fresh vault key at a generation above both the
//! archive and the committed witness. Ordinary opens never get this
//! exception.

use anyhow::{Context, Result as AnyResult, bail};
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::VaultErrorKind;
use crate::audit::AuditAction;
use crate::crypto::KEY_LEN;
use crate::error::{classified, classify_source};
use crate::format::{AuditRoot, V1_FORMAT_VERSION, V3_FORMAT_VERSION, V3StateFields};
use crate::store::VaultStore;
use crate::store::witness::{
    Checkpoint, Journal, JournalPayload, TransactionKind, WitnessRecord, WitnessStore, sha256_hex,
};

use super::OpenVault;
use super::commit::prepare_v3_mutation_event;
use super::envelope::{ParsedVaultEnvelope, RotatedVaultEnvelope};
use super::transaction::{check_candidate_bindings, pending_credential_error, pending_error};

/// Authenticated metadata of the archive being restored.
pub(crate) struct RestoreSource<'a> {
    pub(crate) vault_id: &'a str,
    pub(crate) format_version: u32,
    pub(crate) backup_created_at_ms: i128,
}

/// The witnessed successor a restore will install, with the ID's current
/// record read under the ID lock.
pub(crate) struct RestorePlan {
    pub(crate) next: Checkpoint,
    pub(crate) record: Option<WitnessRecord>,
}

impl VaultStore {
    /// Called on the restore staging store. Authenticates the archived
    /// state, rewrites staging as its witnessed successor, and runs `commit`
    /// with the plan while the staging and ID locks are still held, so the
    /// journal and pending marker are written against the record it read.
    pub(crate) fn prepare_restore_candidate<T>(
        &self,
        passphrase: &SecretString,
        source: &RestoreSource<'_>,
        witness: &WitnessStore,
        commit: impl FnOnce(RestorePlan) -> AnyResult<T>,
    ) -> AnyResult<T> {
        self.with_lock(|| {
            let bytes = self
                .read_vault_bytes()?
                .context("restored vault state disappeared from staging")?;
            let text = std::str::from_utf8(&bytes).context("restored vault is not valid UTF-8")?;
            let unlocked = ParsedVaultEnvelope::parse(text)?
                .validate()?
                .unlock(passphrase)?;
            let mut vault = OpenVault::from_unlocked(unlocked, sha256_hex(&bytes));
            // Check the archived checkpoint before replacing it with the
            // restore event. Freshness against the live witness is deliberately
            // separate: an intact older archive is valid recovery input.
            let audit = if vault.state.v3.is_some() {
                self.verify_mutation_anchor_unlocked(&vault)
            } else {
                vault.verify_audit_unlocked(self).map(|_| ())
            };
            audit.map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "restored vault audit chain verification failed",
                    error,
                )
            })?;
            if vault.format_version() == V1_FORMAT_VERSION {
                return Err(classified(
                    VaultErrorKind::InvalidInput,
                    "backup contains vault format 1; migrate the source first and create a new backup",
                ));
            }
            if vault.format_version() != source.format_version
                || vault.file.header.vault_id != source.vault_id
            {
                return Err(classified(
                    VaultErrorKind::Serialization,
                    "restored vault identity does not match authenticated backup metadata",
                ));
            }
            let _id = witness.lock_id(source.vault_id)?;
            let record = witness.read_record(source.vault_id)?;
            if record.as_ref().is_some_and(|record| record.pending.is_some()) {
                return Err(classified(
                    VaultErrorKind::AlreadyExists,
                    "an unrelated vault transaction is pending for this vault; finish it before restoring",
                ));
            }
            let witnessed = record
                .as_ref()
                .and_then(|record| record.committed.as_ref())
                .map(|checkpoint| checkpoint.generation);
            let source_generation = vault.state.v3.as_ref().map(|fields| fields.generation);
            let base = match (source_generation, witnessed) {
                (Some(archived), witnessed) => archived.max(witnessed.unwrap_or(0)),
                (None, Some(witnessed)) => witnessed,
                (None, None) => bail!("an unwitnessed format 2 archive uses the legacy restore"),
            };
            let generation = base.checked_add(1).ok_or_else(|| {
                classified(
                    VaultErrorKind::Internal,
                    "vault state generation cannot advance further",
                )
            })?;
            if vault.state.v3.is_none() {
                // A witnessed format 2 archive converts to format 3. Its root
                // is the archived DEK's derived audit key, so its history
                // verifies unchanged.
                vault.state.v3 = Some(V3StateFields {
                    audit_root: AuditRoot::from_legacy_audit_key(&vault.audit_key),
                    generation,
                    mutation_audit_mac: String::new(),
                });
            }
            let details = serde_json::json!({
                "source_vault_id": source.vault_id,
                "source_format_version": source.format_version,
                "target_format_version": V3_FORMAT_VERSION,
                "backup_version": crate::BACKUP_FORMAT_VERSION,
                "backup_created_at_ms": source.backup_created_at_ms,
                "source_generation": source_generation,
            });
            let prepared = prepare_v3_mutation_event(
                self,
                vault.audit_key.as_ref(),
                AuditAction::BackupRestore,
                details,
                generation,
            )?;
            let fields = vault.state.v3.as_mut().expect("format 3 fields were set");
            fields.generation = generation;
            fields.mutation_audit_mac = prepared.mac().to_owned();
            let envelope = RotatedVaultEnvelope::seal(
                &vault.file.header,
                passphrase,
                &vault.state,
                vault.file.header.kdf.clone(),
            )?;
            let candidate = envelope.serialize_pretty()?;
            self.validate_vault_text_len(&candidate).map_err(|error| {
                classify_source(
                    VaultErrorKind::InvalidInput,
                    "restored vault state is too large to save safely",
                    error,
                )
            })?;
            let next = Checkpoint {
                generation,
                envelope_sha256: sha256_hex(candidate.as_bytes()),
                mutation_audit_mac: prepared.mac().to_owned(),
            };
            // Staging is not live yet, so plain writes are safe here.
            prepared.commit_unlocked(self)?;
            self.write_vault_text_unlocked(&candidate)?;
            commit(RestorePlan { next, record })
        })
    }
}

/// Authenticates a pending restore candidate's exact envelope text and
/// checks its bindings to the journal.
pub(crate) fn authenticate_restore_candidate_text(
    journal: &Journal,
    candidate: &str,
    credentials: &[&SecretString],
) -> AnyResult<(Zeroizing<[u8; KEY_LEN]>, usize)> {
    for (index, credential) in credentials.iter().enumerate() {
        let unlocked = ParsedVaultEnvelope::parse(candidate)?
            .validate()?
            .unlock(credential);
        let unlocked = match unlocked {
            Ok(unlocked) => unlocked,
            Err(error)
                if crate::error::classified_kind(&error)
                    == Some(VaultErrorKind::Authentication) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        check_candidate_bindings(
            journal,
            &unlocked.file.header.vault_id,
            unlocked.state.v3.as_ref(),
            candidate,
        )?;
        return Ok((unlocked.audit_key, index));
    }
    Err(pending_credential_error(TransactionKind::Restore))
}

/// A restore that failed after its pending marker was recorded.
pub(crate) fn restore_pending_error(error: anyhow::Error) -> anyhow::Error {
    pending_error(TransactionKind::Restore, error)
}

impl VaultStore {
    /// Generic credentialed-open recovery of a pending restore for this
    /// home, which needs no access to the original archive.
    pub(super) fn recover_pending_restore(
        &self,
        witness: &WitnessStore,
        journal: &Journal,
        record: WitnessRecord,
        credentials: &[&SecretString],
    ) -> AnyResult<usize> {
        let JournalPayload::Restore(payload) = &journal.payload else {
            bail!("vault transaction journal does not describe a restore");
        };
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let candidate = crate::backup::read_restore_candidate(journal, payload, self.root())?;
            let (_, index) = authenticate_restore_candidate_text(journal, &candidate, credentials)?;
            crate::backup::finish_pending_restore(witness, journal, record, self.root())?;
            Ok(index)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (witness, payload, record, credentials);
            bail!("vault restore recovery is unsupported on this platform")
        }
    }
}
