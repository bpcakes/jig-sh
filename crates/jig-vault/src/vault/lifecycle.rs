use std::path::{Path, PathBuf};

use anyhow::Result as AnyResult;
use secrecy::SecretString;

use crate::audit::AuditAction;
use crate::backup::{
    BACKUP_START_EVENT_RESERVE_BYTES, BackupCreateRequest, BackupCreateResult,
    BackupRestoreRequest, BackupRestoreResult, BackupSnapshot, inspect_embedded_vault,
    max_backup_audit_bytes,
};
use crate::crypto::KdfParams;
use crate::error::{
    ClassifiedVaultError, classified, classified_kind, classify_source, vault_error_from_anyhow,
};
#[cfg(target_os = "linux")]
use crate::format::V1_FORMAT_VERSION;
use crate::format::{FORMAT_VERSION, VaultFile, validate_header};
use crate::store::VaultStore;
use crate::{Result, VaultError, VaultErrorKind};

use super::envelope::RekeyedVaultEnvelope;
use super::{OpenVault, Vault, validate_new_vault_passphrase_inner};

impl Vault {
    /// Re-encrypts a version-two vault under a new passphrase without changing
    /// its identity, data-encryption key, state, or audit key.
    pub fn change_passphrase(&self, current: &SecretString, new: &SecretString) -> Result<()> {
        self.store.change_passphrase(current, new)
    }

    /// Validates an existing version-two vault before passphrase capture,
    /// without creating a vault home, lock, file, or audit event.
    pub fn preflight_passphrase_change(home: PathBuf) -> Result<()> {
        VaultStore::preflight_passphrase_change(home)
    }

    /// Validates an existing source vault and backup destination without
    /// creating a vault home, lock, archive, or audit event.
    pub fn preflight_backup_create(
        source_home: PathBuf,
        output: &Path,
        overwrite: bool,
    ) -> Result<BackupCreateRequest> {
        crate::backup::preflight_create(source_home, output, overwrite)
    }

    /// Creates and atomically installs an independently encrypted backup.
    pub fn create_backup(
        passphrase: &SecretString,
        request: BackupCreateRequest,
    ) -> Result<BackupCreateResult> {
        crate::backup::create(passphrase, request)
    }

    /// Reads and validates a bounded encrypted archive and absent target
    /// without capturing a passphrase or creating filesystem state.
    pub fn preflight_backup_restore(
        input: &Path,
        target_home: PathBuf,
    ) -> Result<BackupRestoreRequest> {
        crate::backup::preflight_restore(input, target_home)
    }

    /// Restores an encrypted archive through an owned sibling staging
    /// directory and an atomic absent-target installation.
    pub fn restore_backup(
        passphrase: &SecretString,
        request: BackupRestoreRequest,
    ) -> Result<BackupRestoreResult> {
        crate::backup::restore(passphrase, request)
    }
}

impl VaultStore {
    pub(crate) fn preflight_passphrase_change(home: PathBuf) -> Result<()> {
        let store = VaultStore::open_existing(home)?;
        let text = store
            .read_vault_text()
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Io, error))?
            .ok_or_else(|| {
                VaultError::new(
                    VaultErrorKind::NotFound,
                    format!("vault does not exist at {}", store.vault_path().display()),
                )
            })?;
        let file: VaultFile = serde_json::from_str(&text).map_err(|error| {
            VaultError::from_anyhow(
                VaultErrorKind::Serialization,
                anyhow::Error::new(error).context("failed to parse vault public header"),
            )
        })?;
        validate_header(&file.header)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Serialization, error))?;
        if file.header.version != FORMAT_VERSION {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault format {} does not support passphrase change; run `jig vault migrate --to {FORMAT_VERSION}` first",
                    file.header.version
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn change_passphrase(
        &self,
        current: &SecretString,
        new: &SecretString,
    ) -> Result<()> {
        self.change_passphrase_with_kdf(current, new, KdfParams::production())
    }

    #[cfg(test)]
    pub(crate) fn change_passphrase_for_test(
        &self,
        current: &SecretString,
        new: &SecretString,
    ) -> Result<()> {
        self.change_passphrase_with_kdf(current, new, self.initialization_kdf().clone())
    }

    fn change_passphrase_with_kdf(
        &self,
        current: &SecretString,
        new: &SecretString,
        kdf: KdfParams,
    ) -> Result<()> {
        validate_new_vault_passphrase_inner(new)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        self.with_lock(|| self.change_passphrase_unlocked(current, new, kdf))
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn change_passphrase_unlocked(
        &self,
        current: &SecretString,
        new: &SecretString,
        kdf: KdfParams,
    ) -> AnyResult<()> {
        let vault = self.open_unlocked(current)?;
        vault.verify_audit_unlocked(self).map_err(|error| {
            classify_source(
                VaultErrorKind::AuditTampered,
                "vault audit chain verification failed",
                error,
            )
        })?;
        if vault.format_version() != FORMAT_VERSION {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault format {} does not support passphrase change; run `jig vault migrate --to {FORMAT_VERSION}` first",
                    vault.format_version()
                ),
            ));
        }

        // Complete every fallible cryptographic and serialization step before
        // appending intent so invalid input and RNG/serialization failures do
        // not advance the audit chain.
        let envelope = RekeyedVaultEnvelope::seal(&vault.file, new, &vault.dek, &vault.state, kdf)?;
        let file_text = envelope.serialize_pretty()?;
        self.validate_vault_text_len(&file_text).map_err(|error| {
            classify_source(
                VaultErrorKind::InvalidInput,
                "rekeyed vault state is too large to save safely",
                error,
            )
        })?;
        vault
            .append_audit_unlocked(
                self,
                AuditAction::PassphraseChange,
                serde_json::json!({ "format_version": FORMAT_VERSION }),
            )
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit append failed before passphrase change save",
                    error,
                )
            })?;
        self.write_vault_text_unlocked(&file_text)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Io,
                    "passphrase change audit was appended, but vault save failed",
                    error,
                )
            })?;
        Ok(())
    }

    pub(crate) fn prepare_backup_snapshot(
        &self,
        passphrase: &SecretString,
        operation_id: String,
    ) -> Result<BackupSnapshot> {
        self.with_lock(|| {
            // Bound the audit before opening it. Audit verification otherwise
            // accepts the broader persistent-log cap, while backup is an
            // explicitly smaller one-shot operation.
            let pre_start_vault_bytes = self.read_vault_bytes()?.ok_or_else(|| {
                classified(
                    VaultErrorKind::NotFound,
                    format!("vault does not exist at {}", self.vault_path().display()),
                )
            })?;
            let (pre_start_id, pre_start_version) =
                inspect_embedded_vault(&pre_start_vault_bytes)?;
            if pre_start_version != FORMAT_VERSION {
                return Err(classified(
                    VaultErrorKind::InvalidInput,
                    format!(
                        "vault format {pre_start_version} cannot be backed up; run `jig vault migrate --to {FORMAT_VERSION}` first"
                    ),
                ));
            }
            let max_before_start = max_backup_audit_bytes(
                pre_start_id.len(),
                pre_start_vault_bytes.len(),
            )?
            .saturating_sub(BACKUP_START_EVENT_RESERVE_BYTES);
            let audit_len = self.audit_len()?.ok_or_else(|| {
                classified(
                    VaultErrorKind::AuditTampered,
                    "vault audit log is missing; restore audit.jsonl before creating a backup",
                )
            })?;
            if audit_len > max_before_start as u64 {
                return Err(classified(
                    VaultErrorKind::InvalidInput,
                    format!(
                        "vault audit log is {audit_len} bytes; this one-shot backup supports at most {max_before_start} bytes before its start event"
                    ),
                ));
            }

            let vault = self.open_unlocked(passphrase)?;
            vault.verify_audit_unlocked(self).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed before backup",
                    error,
                )
            })?;
            if vault.format_version() != FORMAT_VERSION {
                return Err(classified(
                    VaultErrorKind::InvalidInput,
                    format!(
                        "vault format {} cannot be backed up; run `jig vault migrate --to {FORMAT_VERSION}` first",
                        vault.format_version()
                    ),
                ));
            }
            let source_vault_id = vault.file.header.vault_id.clone();
            vault
                .append_audit_unlocked(
                    self,
                    AuditAction::BackupStart,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "source_vault_id": source_vault_id,
                        "source_format_version": FORMAT_VERSION,
                    }),
                )
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::AuditTampered,
                        "failed to append backup start audit event",
                        error,
                    )
                })?;

            let capture = (|| -> AnyResult<_> {
                let vault_bytes = self.read_vault_bytes()?.ok_or_else(|| {
                    classified(
                        VaultErrorKind::NotFound,
                        "vault state disappeared while preparing backup",
                    )
                })?;
                let (captured_id, captured_version) = inspect_embedded_vault(&vault_bytes)?;
                if captured_id != source_vault_id || captured_version != FORMAT_VERSION {
                    return Err(classified(
                        VaultErrorKind::Serialization,
                        "vault identity changed while preparing backup",
                    ));
                }
                let max_audit = max_backup_audit_bytes(captured_id.len(), vault_bytes.len())?;
                let captured_audit_len = self.audit_len()?.ok_or_else(|| {
                    classified(
                        VaultErrorKind::AuditTampered,
                        "vault audit log disappeared while preparing backup",
                    )
                })?;
                if captured_audit_len > max_audit as u64 {
                    return Err(classified(
                        VaultErrorKind::InvalidInput,
                        format!(
                            "vault audit log is {captured_audit_len} bytes; this one-shot backup supports at most {max_audit} audit bytes for the current vault size"
                        ),
                    ));
                }
                let audit_bytes = self
                    .read_audit_bytes_bounded(max_audit)?
                    .ok_or_else(|| {
                        classified(
                            VaultErrorKind::AuditTampered,
                            "vault audit log disappeared while preparing backup",
                        )
                    })?;
                Ok((vault_bytes, audit_bytes))
            })();
            let (vault_bytes, audit_bytes) = match capture {
                Ok(capture) => capture,
                Err(error) => {
                    return Err(backup_prepare_failure_unlocked(
                        self,
                        &vault,
                        &operation_id,
                        "snapshot",
                        error,
                    ));
                }
            };
            let OpenVault { audit_key, .. } = vault;
            Ok(BackupSnapshot {
                store: self.clone(),
                audit_key,
                operation_id,
                source_vault_id,
                source_format_version: FORMAT_VERSION,
                vault_bytes,
                audit_bytes,
            })
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                self.map_open_error(error)
            }
        })
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn finalize_backup_restore(
        &self,
        passphrase: &SecretString,
        expected_vault_id: &str,
        expected_format_version: u32,
        backup_created_at_ms: i128,
    ) -> Result<()> {
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault.verify_audit_unlocked(self).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "restored vault audit chain verification failed",
                    error,
                )
            })?;
            if vault.format_version() == V1_FORMAT_VERSION {
                return Err(classified(
                    VaultErrorKind::InvalidInput,
                    format!(
                        "backup contains vault format {V1_FORMAT_VERSION}; migrate the source first with `jig vault migrate --to {FORMAT_VERSION}` and create a new backup"
                    ),
                ));
            }
            if vault.format_version() != expected_format_version
                || vault.file.header.vault_id != expected_vault_id
            {
                return Err(classified(
                    VaultErrorKind::Serialization,
                    "restored vault identity does not match authenticated backup metadata",
                ));
            }
            vault
                .append_audit_unlocked(
                    self,
                    AuditAction::BackupRestore,
                    serde_json::json!({
                        "source_vault_id": expected_vault_id,
                        "source_format_version": expected_format_version,
                        "backup_version": crate::BACKUP_FORMAT_VERSION,
                        "backup_created_at_ms": backup_created_at_ms,
                    }),
                )
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::AuditTampered,
                        "failed to append backup restore audit event",
                        error,
                    )
                })?;
            vault.verify_audit_unlocked(self).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "restored vault audit chain failed verification after restore event",
                    error,
                )
            })?;
            Ok(())
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                self.map_open_error(error)
            }
        })
    }
}

fn backup_prepare_failure_unlocked(
    store: &VaultStore,
    vault: &OpenVault,
    operation_id: &str,
    stage: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
    match vault.append_audit_unlocked(
        store,
        AuditAction::BackupFailed,
        serde_json::json!({
            "operation_id": operation_id,
            "stage": stage,
            "error": "vault backup failed",
        }),
    ) {
        Ok(_) => error,
        Err(audit_error) => classify_source(
            kind,
            "vault backup preparation failed; additionally failed to append failure audit event",
            error.context(format!(
                "additional audit failure while recording backup failure: {audit_error}"
            )),
        ),
    }
}
