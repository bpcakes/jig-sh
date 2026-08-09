use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::Result as AnyResult;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::{ExposeSecret, SecretString};
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::audit::{AuditAction, AuditEvent, AuditVerification, verify_chain_unlocked};
use crate::broker::BrokeredRun;
use crate::crypto::KEY_LEN;
#[cfg(test)]
use crate::crypto::{NONCE_LEN, SALT_LEN, derive_wrap_key, open};
use crate::error::{
    ClassifiedVaultError, classified, classified_kind, classify_source, vault_error_from_anyhow,
};
#[cfg(test)]
use crate::format::{AeadRole, decode_b64_array, payload_aad};
use crate::format::{FORMAT_VERSION, SecretEntry, V1_FORMAT_VERSION, VaultFile, VaultState};
use crate::redact::MIN_REDACTABLE_LEN;
use crate::run::{
    ResolvedBrokeredEnv, ResolvedBrokeredFile, ResolvedBrokeredRun, RunOutput, run_brokered,
};
use crate::store::VaultStore;
use crate::types::{FieldKind, SecretName, VaultReference};
use crate::{Result, SecretBytes, VaultError, VaultErrorKind};

mod envelope;

use envelope::{
    MigratedVaultEnvelope, NewVaultEnvelope, ParsedVaultEnvelope, ResealedVaultEnvelope,
    UnlockedVaultEnvelope,
};

pub const MAX_SECRET_VALUE_LEN: usize = 1024 * 1024;
pub const MIN_MASTER_PASSPHRASE_LEN: usize = 12;

#[derive(Clone, Debug)]
pub struct Vault {
    store: VaultStore,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultStatus {
    pub root: PathBuf,
    pub exists: bool,
}

impl Vault {
    /// Resolves a vault handle without opening encrypted state.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault home is invalid, unsafe, or cannot be
    /// resolved and hardened.
    pub fn resolve(explicit_home: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            store: VaultStore::resolve(explicit_home)?,
        })
    }

    pub fn root(&self) -> &Path {
        self.store.root()
    }

    /// Reports whether the vault state file exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the protected state path cannot be inspected
    /// safely.
    pub fn exists(&self) -> Result<bool> {
        self.store.exists()
    }

    /// Resolves a vault home and reports whether initialized state exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault home or state path is invalid, unsafe,
    /// or cannot be inspected.
    pub fn status(explicit_home: Option<PathBuf>) -> Result<VaultStatus> {
        let (root, exists) = VaultStore::inspect(explicit_home)?;
        Ok(VaultStatus { root, exists })
    }

    /// Initializes a new encrypted vault and audit chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the passphrase is invalid, vault state already
    /// exists, stale audit state is present, cryptography fails, or protected
    /// files cannot be created safely.
    pub fn init(&self, passphrase: &SecretString) -> Result<()> {
        self.store.init(passphrase)
    }

    /// Creates or updates one encrypted secret.
    ///
    /// # Errors
    ///
    /// Returns an error when the name or value is invalid, unlocking or audit
    /// verification fails, or the audit and encrypted state cannot be written
    /// safely.
    pub fn set_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
    ) -> Result<()> {
        self.store.set_secret(passphrase, name, value)
    }

    /// Removes one encrypted secret when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is invalid, unlocking or audit
    /// verification fails, or the audit and encrypted state cannot be written
    /// safely.
    pub fn remove_secret(&self, passphrase: &SecretString, name: &str) -> Result<bool> {
        self.store.remove_secret(passphrase, name)
    }

    /// Lists secret metadata without returning plaintext values.
    ///
    /// # Errors
    ///
    /// Returns an error when vault state cannot be read, authenticated,
    /// decrypted, decoded, or validated safely.
    pub fn list(&self, passphrase: &SecretString) -> Result<Vec<SecretRecord>> {
        self.store.list(passphrase)
    }

    /// Explicitly upgrades a version 1 vault envelope to the current format.
    ///
    /// Version 1 vaults remain readable for compatibility, but field-oriented
    /// mutations require this deliberate one-way upgrade so older binaries do
    /// not silently discard field kinds.
    ///
    /// # Errors
    ///
    /// Returns an error when the target is unsupported, the vault cannot be
    /// opened and audit-verified, or the audit/state transition cannot be
    /// written atomically.
    pub fn migrate(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> Result<VaultMigration> {
        self.store.migrate(passphrase, target_version)
    }

    /// Lists canonical field metadata without returning encrypted values.
    ///
    /// Legacy secret names that cannot be represented as `jig://ITEM/FIELD`
    /// remain available through [`Vault::list`] and are intentionally omitted
    /// from this field-oriented view.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be opened safely.
    pub fn list_fields(&self, passphrase: &SecretString) -> Result<Vec<FieldRecord>> {
        self.store.list_fields(passphrase)
    }

    /// Creates or updates one canonical encrypted field.
    ///
    /// Fields can be set only after an explicit v1-to-v2 migration. Text
    /// fields remain encrypted and may be empty; concealed fields retain the
    /// existing minimum length required for reliable redaction.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation when the field is invalid, the vault
    /// needs migration, audit verification fails, or persistence fails.
    pub fn set_field(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
    ) -> Result<FieldBatchResult> {
        self.store
            .apply_field_batch(passphrase, vec![FieldMutation::set(reference, kind, value)])
    }

    /// Removes one canonical field when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation when the vault needs migration or
    /// cannot be opened, audit-verified, and saved safely.
    pub fn remove_field(
        &self,
        passphrase: &SecretString,
        reference: VaultReference,
    ) -> Result<FieldBatchResult> {
        self.store
            .apply_field_batch(passphrase, vec![FieldMutation::remove(reference)])
    }

    /// Applies validated field changes as one audited vault-state transition.
    ///
    /// Set mutations create or replace fields. Duplicate references within a
    /// batch are rejected before any audit append or state save.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation when validation, migration, unlock,
    /// or audit verification fails. A state-save failure can leave the audit
    /// intent ahead of state, matching the vault's existing mutation invariant.
    pub fn apply_field_batch(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
    ) -> Result<FieldBatchResult> {
        self.store.apply_field_batch(passphrase, mutations)
    }

    /// Verifies the vault's tamper-evident audit chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be unlocked or the audit log
    /// cannot be read and verified.
    pub fn verify_audit(&self, passphrase: &SecretString) -> Result<AuditVerification> {
        self.store.verify_audit(passphrase)
    }

    /// Runs a command with brokered secret environment and file mappings.
    ///
    /// # Errors
    ///
    /// Returns an error when the vault cannot be unlocked, audit or secret
    /// resolution fails, process-tree supervision is unavailable, the command
    /// fails to start, output exceeds safety bounds, or cleanup is incomplete.
    pub fn run_brokered(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> Result<RunOutput> {
        self.store.run_brokered(passphrase, request)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretRecord {
    pub name: String,
    pub created_at_ms: i128,
    pub updated_at_ms: i128,
    pub value_len: usize,
}

/// Metadata for one canonical vault field, without its encrypted value.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldRecord {
    pub reference: VaultReference,
    pub kind: FieldKind,
    pub created_at_ms: i128,
    pub updated_at_ms: i128,
    pub value_len: usize,
}

/// One atomic field change.
///
/// `SecretBytes` owns and zeroizes the supplied value when this mutation is
/// consumed or dropped. Its `Debug` implementation intentionally hides bytes.
#[derive(Debug)]
pub enum FieldMutation {
    Set {
        reference: VaultReference,
        kind: FieldKind,
        value: SecretBytes,
    },
    Remove {
        reference: VaultReference,
    },
}

impl FieldMutation {
    /// Creates a field set mutation.
    pub fn set(reference: VaultReference, kind: FieldKind, value: SecretBytes) -> Self {
        Self::Set {
            reference,
            kind,
            value,
        }
    }

    /// Creates a field removal mutation.
    pub fn remove(reference: VaultReference) -> Self {
        Self::Remove { reference }
    }

    fn reference(&self) -> &VaultReference {
        match self {
            Self::Set { reference, .. } | Self::Remove { reference } => reference,
        }
    }
}

/// Metadata-only outcome of an atomic field change.
#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FieldBatchResult {
    /// References created or updated by set mutations.
    pub changed: Vec<VaultReference>,
    /// References that existed and were removed by remove mutations.
    pub removed: Vec<VaultReference>,
}

/// Outcome of an explicit vault envelope migration.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub changed: bool,
}

pub(crate) struct OpenVault {
    file: VaultFile,
    state: VaultState,
    dek: Zeroizing<[u8; KEY_LEN]>,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrokeredRunStage {
    Resolve,
    Process,
}

impl BrokeredRunStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Process => "process",
        }
    }
}

struct StartedBrokeredRun {
    // Holds the opened vault, including DEK/audit key material, until the
    // brokered child finishes or fails so the matching audit event can be
    // appended after the vault lock is released for the child lifetime.
    vault: OpenVault,
    run_id: String,
}

struct PreparedBrokeredRun {
    started: StartedBrokeredRun,
    resolved: ResolvedBrokeredRun,
}

impl StartedBrokeredRun {
    fn resolve_unlocked(
        self,
        store: &VaultStore,
        request: BrokeredRun,
    ) -> AnyResult<PreparedBrokeredRun> {
        let resolved = match resolve_brokered_run(&self.vault, request) {
            Ok(resolved) => resolved,
            Err(error) => {
                return Err(self.failure_error_unlocked(store, BrokeredRunStage::Resolve, error));
            }
        };
        Ok(PreparedBrokeredRun {
            started: self,
            resolved,
        })
    }

    fn record_finish(&self, store: &VaultStore, output: &RunOutput) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFinish,
            serde_json::json!({
                "run_id": self.run_id,
                "exit_status": output.exit_status,
                "exit_signal": output.exit_signal,
            }),
        )?;
        Ok(())
    }

    fn failure_error(
        &self,
        store: &VaultStore,
        stage: BrokeredRunStage,
        kind: VaultErrorKind,
        error: anyhow::Error,
    ) -> VaultError {
        if let Err(audit_error) = self.record_failure(store, stage) {
            return VaultError::from_anyhow(
                kind,
                error.context(format!(
                    "brokered run failed; additionally failed to append failure audit event: {audit_error}"
                )),
            );
        }
        VaultError::from_anyhow(kind, error)
    }

    fn record_failure(&self, store: &VaultStore, stage: BrokeredRunStage) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFailed,
            brokered_run_failure_details(&self.run_id, stage),
        )?;
        Ok(())
    }

    fn failure_error_unlocked(
        &self,
        store: &VaultStore,
        stage: BrokeredRunStage,
        error: anyhow::Error,
    ) -> anyhow::Error {
        let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
        if let Err(audit_error) = self.record_failure_unlocked(store, stage) {
            return classify_source(
                kind,
                "brokered run failed; additionally failed to append failure audit event",
                error.context(format!(
                    "additional audit failure while recording brokered run failure: {audit_error}"
                )),
            );
        }
        error
    }

    fn record_failure_unlocked(
        &self,
        store: &VaultStore,
        stage: BrokeredRunStage,
    ) -> AnyResult<()> {
        self.vault.append_audit_unlocked(
            store,
            AuditAction::BrokeredRunFailed,
            brokered_run_failure_details(&self.run_id, stage),
        )?;
        Ok(())
    }
}

impl PreparedBrokeredRun {
    fn execute(self, store: &VaultStore) -> Result<RunOutput> {
        let Self { started, resolved } = self;
        match run_brokered(resolved) {
            Ok(output) => {
                started.record_finish(store, &output).map_err(|error| {
                    VaultError::from_anyhow(VaultErrorKind::AuditTampered, error)
                })?;
                Ok(output)
            }
            Err(error) => Err(started.failure_error(
                store,
                BrokeredRunStage::Process,
                VaultErrorKind::Process,
                error,
            )),
        }
    }
}

impl std::fmt::Debug for OpenVault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenVault")
            .field("vault_id", &self.file.header.vault_id)
            .field("secret_count", &self.state.secrets.len())
            .field("dek", &"[REDACTED]")
            .field("audit_key", &"[REDACTED]")
            .finish()
    }
}

impl OpenVault {
    fn from_unlocked(envelope: UnlockedVaultEnvelope) -> Self {
        let UnlockedVaultEnvelope {
            file,
            state,
            dek,
            audit_key,
        } = envelope;
        Self {
            file,
            state,
            dek,
            audit_key,
        }
    }
}

impl VaultStore {
    pub(crate) fn init(&self, passphrase: &SecretString) -> Result<()> {
        self.with_lock(|| self.init_unlocked(passphrase))
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn init_unlocked(&self, passphrase: &SecretString) -> AnyResult<()> {
        if self.read_vault_text()?.is_some() {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!("vault already exists at {}", self.vault_path().display()),
            ));
        }
        if self.audit_exists()? {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                format!(
                    "vault audit log already exists at {}; remove the stale vault home before init",
                    self.audit_path().display()
                ),
            ));
        }
        validate_new_vault_passphrase_inner(passphrase)?;

        let envelope = NewVaultEnvelope::seal(passphrase, now_ms())?;
        if let Err(error) = AuditEvent::append_unlocked(
            self,
            envelope.audit_key.as_ref(),
            AuditAction::VaultInitialized,
            serde_json::json!({
                "vault_id": envelope.file.header.vault_id,
            }),
        ) {
            let cleanup_error = rollback_failed_init(self);
            let error = error.context("failed to initialize vault audit log");
            return match cleanup_error {
                Some(cleanup_error) => Err(error.context(cleanup_error)),
                None => Err(error),
            };
        }
        if let Err(error) = self.write_vault_text_unlocked(&envelope.file_text) {
            let cleanup_error = rollback_failed_init(self);
            let error = error.context("failed to write initialized vault file");
            return match cleanup_error {
                Some(cleanup_error) => Err(error.context(cleanup_error)),
                None => Err(error),
            };
        }
        Ok(())
    }

    /// Runs a vault mutation while preserving the audit invariant: open under
    /// lock, verify the current chain, mutate in memory, append the audit
    /// intent, then save state before releasing the lock. If the process dies
    /// mid-operation, the audit may lead state but state must not lead audit.
    pub(crate) fn edit_with_audit<R>(
        &self,
        passphrase: &SecretString,
        action: AuditAction,
        edit: impl FnOnce(&mut OpenVault) -> AnyResult<R>,
        details: impl FnOnce(&R) -> serde_json::Value,
    ) -> AnyResult<R> {
        self.with_lock(|| {
            let mut vault = self.open_unlocked(passphrase)?;
            verify_chain_unlocked(self, vault.audit_key.as_ref()).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
            let result = edit(&mut vault)?;
            let envelope = vault.prepare_save_unlocked()?;
            let file_text = envelope.serialize_pretty()?;
            self.validate_vault_text_len(&file_text).map_err(|error| {
                classify_source(
                    VaultErrorKind::InvalidInput,
                    "vault state is too large to save safely",
                    error,
                )
            })?;
            AuditEvent::append_unlocked(self, vault.audit_key.as_ref(), action, details(&result))
                .map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit append failed before state save",
                    error,
                )
            })?;
            self.write_vault_text_unlocked(&file_text)
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::Io,
                        "vault audit was appended, but state save failed",
                        error,
                    )
                })?;
            Ok(result)
        })
    }

    pub(crate) fn set_secret(
        &self,
        passphrase: &SecretString,
        name: &str,
        value: SecretBytes,
    ) -> Result<()> {
        let name = SecretName::parse(name)?;
        self.set_secret_inner(passphrase, name, value)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn set_secret_inner(
        &self,
        passphrase: &SecretString,
        name: SecretName,
        value: SecretBytes,
    ) -> AnyResult<()> {
        // Reject too-short values before unlocking; `OpenVault::set_secret`
        // repeats this guard for internal callers that already hold a handle.
        validate_secret_value_len(value.len())?;
        self.edit_with_audit(
            passphrase,
            AuditAction::SecretSet,
            |vault| vault.set_secret(&name, value),
            |()| {
                serde_json::json!({
                    "secret_name": name.as_str(),
                })
            },
        )
    }

    pub(crate) fn remove_secret(&self, passphrase: &SecretString, name: &str) -> Result<bool> {
        let name = SecretName::parse(name)?;
        self.remove_secret_inner(passphrase, name)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    fn remove_secret_inner(&self, passphrase: &SecretString, name: SecretName) -> AnyResult<bool> {
        self.edit_with_audit(
            passphrase,
            AuditAction::SecretRemove,
            |vault| Ok(vault.remove_secret(&name)),
            |removed| {
                serde_json::json!({
                    "secret_name": name.as_str(),
                    "removed": removed,
                })
            },
        )
    }

    pub(crate) fn list(&self, passphrase: &SecretString) -> Result<Vec<SecretRecord>> {
        self.with_lock(|| self.open_unlocked(passphrase).map(|vault| vault.list()))
            .map_err(|error| self.map_open_error(error))
    }

    pub(crate) fn migrate(
        &self,
        passphrase: &SecretString,
        target_version: u32,
    ) -> Result<VaultMigration> {
        if target_version != FORMAT_VERSION {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "unsupported vault migration target {target_version}; run `jig vault migrate --to {FORMAT_VERSION}`"
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
        let vault = self.open_unlocked(passphrase)?;
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
        if from_version != V1_FORMAT_VERSION {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!("vault format {from_version} cannot be migrated to {target_version}"),
            ));
        }

        let envelope =
            MigratedVaultEnvelope::v1_to_v2(&vault.file, passphrase, &vault.dek, &vault.state)?;
        let file_text = envelope.serialize_pretty()?;
        self.validate_vault_text_len(&file_text).map_err(|error| {
            classify_source(
                VaultErrorKind::InvalidInput,
                "vault format migration would exceed the persistent vault size limit",
                error,
            )
        })?;
        AuditEvent::append_unlocked(
            self,
            vault.audit_key.as_ref(),
            AuditAction::VaultFormatMigrate,
            serde_json::json!({
                "from_version": from_version,
                "to_version": target_version,
            }),
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

    pub(crate) fn list_fields(&self, passphrase: &SecretString) -> Result<Vec<FieldRecord>> {
        self.with_lock(|| {
            self.open_unlocked(passphrase)
                .map(|vault| vault.list_fields())
        })
        .map_err(|error| self.map_open_error(error))
    }

    pub(crate) fn apply_field_batch(
        &self,
        passphrase: &SecretString,
        mutations: Vec<FieldMutation>,
    ) -> Result<FieldBatchResult> {
        validate_field_mutations(&mutations)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        let audit_sets = field_batch_set_audit_metadata(&mutations);
        let audit_removes = field_batch_remove_audit_metadata(&mutations);
        self.edit_with_audit(
            passphrase,
            AuditAction::FieldBatchApply,
            |vault| {
                vault.ensure_field_format_v2()?;
                Ok(vault.apply_validated_field_batch(mutations))
            },
            |result| field_batch_audit_details(&audit_sets, &audit_removes, result),
        )
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }

    pub(crate) fn verify_audit(&self, passphrase: &SecretString) -> Result<AuditVerification> {
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            vault.verify_audit_unlocked(self)
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                VaultError::from_anyhow(VaultErrorKind::AuditTampered, error)
            }
        })
    }

    /// Maps open-time failures while preserving classified vault errors.
    ///
    /// If a secondary `exists` probe fails, the public kind becomes `Io` but
    /// the original open failure remains the source for diagnostics.
    pub(crate) fn map_open_error(&self, error: anyhow::Error) -> VaultError {
        if error.is::<ClassifiedVaultError>() {
            return vault_error_from_anyhow(VaultErrorKind::Internal, error);
        }
        let default = match self.exists() {
            Ok(false) => VaultErrorKind::NotFound,
            Ok(true) => VaultErrorKind::Internal,
            // Preserve the original open failure as the source; this probe only refines the kind.
            Err(_) => VaultErrorKind::Io,
        };
        VaultError::from_anyhow(default, error)
    }

    fn prepare_brokered_run(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> AnyResult<PreparedBrokeredRun> {
        let run_id = ulid::Ulid::new().to_string();
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            let start_details = brokered_run_start_details(&request, &run_id);
            vault
                .append_audit_unlocked(self, AuditAction::BrokeredRunStart, start_details)
                .map_err(|error| {
                    classify_source(
                        VaultErrorKind::AuditTampered,
                        "failed to append brokered run start audit event",
                        error,
                    )
                })?;
            StartedBrokeredRun { vault, run_id }.resolve_unlocked(self, request)
        })
    }

    pub(crate) fn run_brokered(
        &self,
        passphrase: &SecretString,
        request: BrokeredRun,
    ) -> Result<RunOutput> {
        let prepared = self
            .prepare_brokered_run(passphrase, request)
            .map_err(|error| {
                if error.is::<ClassifiedVaultError>() {
                    // Classified errors already carry their public kind; the default
                    // only applies if a future classified source omits one.
                    vault_error_from_anyhow(VaultErrorKind::Internal, error)
                } else {
                    self.map_open_error(error)
                }
            })?;
        // Preparation returns only after the vault lock is released. The
        // prepared state owns normal terminal audit recording, while abrupt
        // process termination intentionally may leave its start unmatched.
        prepared.execute(self)
    }

    fn open_unlocked(&self, passphrase: &SecretString) -> AnyResult<OpenVault> {
        let text = self.read_vault_text()?.ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault does not exist at {}", self.vault_path().display()),
            )
        })?;
        let parsed = ParsedVaultEnvelope::parse(&text)?;
        let validated = parsed.validate()?;
        let unlocked = validated.unlock(passphrase)?;
        Ok(OpenVault::from_unlocked(unlocked))
    }
}

/// Validates a passphrase for new vault creation.
///
/// # Errors
///
/// Returns an error when the passphrase is shorter than
/// [`MIN_MASTER_PASSPHRASE_LEN`] bytes.
pub fn validate_new_vault_passphrase(passphrase: &SecretString) -> Result<()> {
    validate_new_vault_passphrase_inner(passphrase).map_err(|error| {
        VaultError::new(
            classified_kind(&error).unwrap_or(VaultErrorKind::InvalidInput),
            error.to_string(),
        )
    })
}

impl OpenVault {
    pub(crate) fn list(&self) -> Vec<SecretRecord> {
        self.state
            .secrets
            .iter()
            .map(|(name, entry)| SecretRecord {
                name: name.clone(),
                created_at_ms: entry.created_at_ms,
                updated_at_ms: entry.updated_at_ms,
                value_len: entry.value_len,
            })
            .collect()
    }

    pub(crate) fn list_fields(&self) -> Vec<FieldRecord> {
        self.state
            .secrets
            .iter()
            .filter_map(|(name, entry)| {
                let name = SecretName::parse(name).ok()?;
                let reference = VaultReference::from_secret_name(&name)?;
                Some(FieldRecord {
                    reference,
                    kind: entry.kind,
                    created_at_ms: entry.created_at_ms,
                    updated_at_ms: entry.updated_at_ms,
                    value_len: entry.value_len,
                })
            })
            .collect()
    }

    fn format_version(&self) -> u32 {
        self.file.header.version
    }

    fn ensure_field_format_v2(&self) -> AnyResult<()> {
        if self.format_version() != FORMAT_VERSION {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!(
                    "vault format {} does not support field mutations; run `jig vault migrate --to {FORMAT_VERSION}`",
                    self.format_version()
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn set_secret(&mut self, name: &SecretName, value: SecretBytes) -> AnyResult<()> {
        validate_secret_value_len(value.len())?;
        self.set_field_value_unchecked(name, FieldKind::Concealed, value);
        Ok(())
    }

    fn apply_validated_field_batch(&mut self, mutations: Vec<FieldMutation>) -> FieldBatchResult {
        let mut result = FieldBatchResult::default();
        for mutation in mutations {
            match mutation {
                FieldMutation::Set {
                    reference,
                    kind,
                    value,
                } => {
                    let name = reference.to_secret_name();
                    self.set_field_value_unchecked(&name, kind, value);
                    result.changed.push(reference);
                }
                FieldMutation::Remove { reference } => {
                    let name = reference.to_secret_name();
                    if self.remove_secret(&name) {
                        result.removed.push(reference);
                    }
                }
            }
        }
        result
    }

    fn set_field_value_unchecked(
        &mut self,
        name: &SecretName,
        kind: FieldKind,
        mut value: SecretBytes,
    ) {
        let now = now_ms();
        let created_at_ms = self
            .state
            .secrets
            .get(name.as_str())
            .map(|entry| entry.created_at_ms)
            .unwrap_or(now);
        let mut value_b64 = Zeroizing::new(String::with_capacity(padded_base64_len(value.len())));
        B64.encode_string(value.as_slice(), &mut value_b64);
        debug_assert_eq!(value_b64.capacity(), padded_base64_len(value.len()));
        let entry = SecretEntry {
            value_b64: std::mem::take(&mut *value_b64),
            value_len: value.len(),
            created_at_ms,
            updated_at_ms: now,
            kind,
        };
        value.zeroize();
        // Replaced entries are dropped here; `SecretEntry::drop` zeroizes the
        // displaced base64 value.
        self.state.secrets.insert(name.as_str().to_string(), entry);
    }

    pub(crate) fn remove_secret(&mut self, name: &SecretName) -> bool {
        self.state.secrets.remove(name.as_str()).is_some()
    }

    pub(crate) fn secret_value(&self, name: &SecretName) -> AnyResult<SecretBytes> {
        let entry = self.state.secrets.get(name.as_str()).ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault secret '{}' does not exist", name.as_str()),
            )
        })?;
        validate_serialized_field_value_len(name, entry)?;
        // `decoded_len_estimate` may overestimate by a couple of bytes; the
        // buffer starts zeroed and is truncated to the decoded length below.
        let mut value = SecretBytes::zeroed(base64::decoded_len_estimate(entry.value_b64.len()));
        let decoded_len = B64
            .decode_slice(entry.value_b64.as_bytes(), value.as_mut_slice())
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    format!("vault secret '{}' value is not valid base64", name.as_str()),
                    error.into(),
                )
            })?;
        value.truncate(decoded_len);
        if value.len() != entry.value_len {
            return Err(classified(
                VaultErrorKind::Serialization,
                format!(
                    "vault secret '{}' value length metadata is invalid",
                    name.as_str()
                ),
            ));
        }
        Ok(value)
    }

    fn prepare_save_unlocked(&self) -> AnyResult<ResealedVaultEnvelope> {
        ResealedVaultEnvelope::seal(&self.file, &self.dek, &self.state)
    }

    pub(crate) fn append_audit(
        &self,
        store: &VaultStore,
        action: AuditAction,
        details: serde_json::Value,
    ) -> AnyResult<AuditEvent> {
        AuditEvent::append(store, self.audit_key.as_ref(), action, details)
    }

    pub(crate) fn append_audit_unlocked(
        &self,
        store: &VaultStore,
        action: AuditAction,
        details: serde_json::Value,
    ) -> AnyResult<AuditEvent> {
        AuditEvent::append_unlocked(store, self.audit_key.as_ref(), action, details)
    }

    pub(crate) fn verify_audit_unlocked(&self, store: &VaultStore) -> AnyResult<AuditVerification> {
        verify_chain_unlocked(store, self.audit_key.as_ref())
    }
}

fn brokered_run_start_details(request: &BrokeredRun, run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "env": request.env().iter().map(|mapping| serde_json::json!({
            "var": mapping.var().as_str(),
            "secret_name": mapping.secret_name().as_str(),
        })).collect::<Vec<_>>(),
        "files": request.files().iter().map(|mapping| serde_json::json!({
            "var": mapping.var().as_str(),
            "secret_name": mapping.secret_name().as_str(),
        })).collect::<Vec<_>>(),
    })
}

fn brokered_run_failure_details(run_id: &str, stage: BrokeredRunStage) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "stage": stage.as_str(),
        // Do not record the original error text here. Spawn/process errors can
        // include argv or paths, and audit logs are value-free metadata.
        "error": "brokered run failed",
    })
}

fn resolve_brokered_run(vault: &OpenVault, request: BrokeredRun) -> AnyResult<ResolvedBrokeredRun> {
    let (command, env_mappings, file_mappings) = request.into_parts();
    let mut env = Vec::with_capacity(env_mappings.len());
    for mapping in env_mappings {
        let (var, secret_name) = mapping.into_parts();
        let value = vault.secret_value(&secret_name)?;
        env.push(ResolvedBrokeredEnv {
            var,
            secret_name,
            value,
        });
    }
    let mut files = Vec::with_capacity(file_mappings.len());
    for mapping in file_mappings {
        let (var, secret_name) = mapping.into_parts();
        let value = vault.secret_value(&secret_name)?;
        files.push(ResolvedBrokeredFile {
            var,
            secret_name,
            value,
        });
    }
    Ok(ResolvedBrokeredRun {
        command,
        env,
        files,
    })
}

fn now_ms() -> i128 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000
}

fn validate_field_mutations(mutations: &[FieldMutation]) -> AnyResult<()> {
    if mutations.is_empty() {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "field batch must contain at least one mutation",
        ));
    }
    let mut references = BTreeSet::new();
    for mutation in mutations {
        let reference = mutation.reference();
        if !references.insert(reference) {
            return Err(classified(
                VaultErrorKind::InvalidInput,
                format!("field batch contains duplicate reference '{reference}'"),
            ));
        }
        if let FieldMutation::Set { kind, value, .. } = mutation {
            validate_field_value_len(*kind, value.len())?;
        }
    }
    Ok(())
}

fn field_batch_set_audit_metadata(mutations: &[FieldMutation]) -> Vec<(VaultReference, FieldKind)> {
    mutations
        .iter()
        .filter_map(|mutation| match mutation {
            FieldMutation::Set {
                reference, kind, ..
            } => Some((reference.clone(), *kind)),
            FieldMutation::Remove { .. } => None,
        })
        .collect()
}

fn field_batch_remove_audit_metadata(mutations: &[FieldMutation]) -> Vec<VaultReference> {
    mutations
        .iter()
        .filter_map(|mutation| match mutation {
            FieldMutation::Set { .. } => None,
            FieldMutation::Remove { reference } => Some(reference.clone()),
        })
        .collect()
}

fn field_batch_audit_details(
    sets: &[(VaultReference, FieldKind)],
    removes: &[VaultReference],
    result: &FieldBatchResult,
) -> serde_json::Value {
    serde_json::json!({
        "set": sets.iter().map(|(reference, kind)| serde_json::json!({
            "reference": reference.to_string(),
            "kind": kind.as_str(),
        })).collect::<Vec<_>>(),
        "remove": removes.iter().map(|reference| serde_json::json!({
            "reference": reference.to_string(),
            "removed": result.removed.contains(reference),
        })).collect::<Vec<_>>(),
    })
}

const fn padded_base64_len(len: usize) -> usize {
    len.div_ceil(3) * 4
}

fn validate_secret_value_len(len: usize) -> AnyResult<()> {
    if len < MIN_REDACTABLE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "secret value must be at least 4 bytes so redaction can match it safely",
        ));
    }
    if len > MAX_SECRET_VALUE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("secret value must be at most {MAX_SECRET_VALUE_LEN} bytes"),
        ));
    }
    Ok(())
}

fn validate_field_value_len(kind: FieldKind, len: usize) -> AnyResult<()> {
    if kind == FieldKind::Concealed {
        return validate_secret_value_len(len);
    }
    if len > MAX_SECRET_VALUE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("field value must be at most {MAX_SECRET_VALUE_LEN} bytes"),
        ));
    }
    Ok(())
}

fn validate_serialized_field_value_len(name: &SecretName, entry: &SecretEntry) -> AnyResult<()> {
    let too_short = entry.kind == FieldKind::Concealed && entry.value_len < MIN_REDACTABLE_LEN;
    if too_short || entry.value_len > MAX_SECRET_VALUE_LEN {
        return Err(classified(
            VaultErrorKind::Serialization,
            format!(
                "vault secret '{}' value length metadata is outside supported bounds",
                name.as_str()
            ),
        ));
    }
    if entry.value_b64.len() > padded_base64_len(MAX_SECRET_VALUE_LEN) {
        return Err(classified(
            VaultErrorKind::Serialization,
            format!(
                "vault secret '{}' encoded value is outside supported bounds",
                name.as_str()
            ),
        ));
    }
    Ok(())
}

fn validate_new_vault_passphrase_inner(passphrase: &SecretString) -> AnyResult<()> {
    if passphrase.expose_secret().len() < MIN_MASTER_PASSPHRASE_LEN {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("vault passphrase must be at least {MIN_MASTER_PASSPHRASE_LEN} bytes"),
        ));
    }
    Ok(())
}

fn rollback_failed_init(store: &VaultStore) -> Option<String> {
    let mut failures = Vec::new();
    for path in [store.vault_path(), store.audit_path()] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!("failed to remove {}: {error}", path.display())),
        }
    }
    if failures.is_empty() {
        None
    } else {
        Some(format!(
            "vault init rollback left partial state; inspect or remove {} and {} before retrying: {}",
            store.vault_path().display(),
            store.audit_path().display(),
            failures.join("; ")
        ))
    }
}

#[cfg(test)]
// Keep the broad vault facade tests out of this already-central module body.
#[path = "vault_tests.rs"]
mod tests;
