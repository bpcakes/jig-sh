//! Explicit one-way envelope migration.
//!
//! Supported targets are format 2 (preserving the legacy 1->2 upgrade) and
//! format 3. Migrating to the current format only verifies integrity.
//! Downgrades and unknown targets fail before any write.

use anyhow::Result as AnyResult;
use secrecy::SecretString;

use crate::audit::{AuditAction, AuditEvent};
use crate::error::{classified, classify_source};
use crate::format::{AuditRoot, V2_FORMAT_VERSION, V3_FORMAT_VERSION, V3StateFields};
use crate::store::VaultStore;
use crate::store::witness::TransactionKind;
use crate::{Result, VaultError, VaultErrorKind};

use super::VaultMigration;
use super::commit::prepare_v3_mutation_event;
use super::envelope::MigratedVaultEnvelope;
use super::transaction::InPlaceCommit;

impl VaultStore {
    pub(crate) fn migrate(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> Result<VaultMigration> {
        if !matches!(target_version, V2_FORMAT_VERSION | V3_FORMAT_VERSION) {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "unsupported vault migration target {target_version}; supported targets are {V2_FORMAT_VERSION} and {V3_FORMAT_VERSION}"
                ),
            ));
        }
        self.with_lock(|| self.migrate_unlocked(passphrase, target_version))
            .map_err(|error| self.map_open_error(error))
    }

    fn migrate_unlocked(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> AnyResult<VaultMigration> {
        let mut vault = self.open_unlocked(passphrase)?;
        vault.verify_audit_unlocked(self).map_err(|error| {
            classify_source(
                VaultErrorKind::AuditTampered,
                "vault audit chain verification failed",
                error,
            )
        })?;
        let from_version = vault.format_version();
        if from_version == target_version {
            return Ok(VaultMigration {
                from_version,
                to_version: target_version,
                changed: false,
            });
        }
        if from_version > target_version {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault format {from_version} cannot be migrated to {target_version}; downgrades are not supported"
                ),
            ));
        }

        let details = serde_json::json!({
            "from_version": from_version,
            "to_version": target_version,
        });
        let prepared = if target_version == V3_FORMAT_VERSION {
            // Seed the independent root with the legacy derived audit key so
            // every historical MAC still verifies under the version 3 root.
            let prepared = prepare_v3_mutation_event(
                self,
                vault.audit_key.as_ref(),
                AuditAction::VaultFormatMigrate,
                details.clone(),
                1,
            )?;
            vault.state.v3 = Some(V3StateFields {
                audit_root: AuditRoot::from_legacy_audit_key(&vault.audit_key),
                generation: 1,
                mutation_audit_mac: prepared.mac().to_owned(),
            });
            Some(prepared)
        } else {
            None
        };
        let envelope = MigratedVaultEnvelope::migrate(
            &vault.file,
            passphrase,
            &vault.dek,
            &vault.state,
            target_version,
        )?;
        let file_text = envelope.serialize_pretty()?;
        self.validate_vault_text_len(&file_text).map_err(|error| {
            classify_source(
                VaultErrorKind::InvalidInput,
                "vault format migration would exceed the persistent vault size limit",
                error,
            )
        })?;
        if let Some(prepared) = prepared {
            self.commit_in_place_unlocked(InPlaceCommit {
                kind: TransactionKind::Migrate,
                vault_id: &vault.file.header.vault_id,
                previous_envelope_sha256: Some(&vault.envelope_sha256),
                candidate: &file_text,
                generation: 1,
                prepared: &prepared,
            })?;
            return Ok(VaultMigration {
                from_version,
                to_version: target_version,
                changed: true,
            });
        }
        AuditEvent::append_unlocked(
            self,
            vault.audit_key.as_ref(),
            AuditAction::VaultFormatMigrate,
            details,
        )
        .map_err(|error| {
            classify_source(
                VaultErrorKind::AuditTampered,
                "vault audit append failed before format migration save",
                error,
            )
        })?;
        self.write_vault_text_unlocked(&file_text)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Io,
                    "vault format migration audit was appended, but state save failed",
                    error,
                )
            })?;
        Ok(VaultMigration {
            from_version,
            to_version: target_version,
            changed: true,
        })
    }
}
