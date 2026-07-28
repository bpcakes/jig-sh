use std::path::{Path, PathBuf};

use anyhow::Result as AnyResult;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::{ExposeSecret, SecretString};
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::audit::{AuditAction, AuditEvent, AuditVerification, verify_chain_unlocked};
use crate::broker::BrokeredRun;
use crate::crypto::{
    KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN, decode_array, derive_audit_key, derive_wrap_key, open,
    random_array, seal,
};
use crate::error::{
    ClassifiedVaultError, classified, classified_kind, classify_source, vault_error_from_anyhow,
};
use crate::format::{
    AEAD_ALGORITHM, AeadRole, FORMAT_VERSION, MAGIC, SecretEntry, VaultFile, VaultHeader,
    VaultState, decode_b64_array, payload_aad, validate_header,
};
use crate::redact::MIN_REDACTABLE_LEN;
use crate::run::{
    ResolvedBrokeredEnv, ResolvedBrokeredFile, ResolvedBrokeredRun, RunOutput, run_brokered,
};
use crate::store::VaultStore;
use crate::types::SecretName;
use crate::{Result, SecretBytes, VaultError, VaultErrorKind};

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

pub(crate) struct OpenVault {
    file: VaultFile,
    state: VaultState,
    dek: Zeroizing<[u8; KEY_LEN]>,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
}

pub(crate) struct PreparedBrokeredRun {
    pub(crate) handle: BrokeredRunHandle,
    pub(crate) resolved: ResolvedBrokeredRun,
}

pub(crate) struct BrokeredRunHandle {
    // Holds the opened vault, including DEK/audit key material, until the
    // brokered child finishes or fails so the matching audit event can be
    // appended after the vault lock is released for the child lifetime.
    vault: OpenVault,
    run_id: String,
}

impl BrokeredRunHandle {
    pub(crate) fn record_finish(&self, store: &VaultStore, output: &RunOutput) -> AnyResult<()> {
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

    pub(crate) fn failure_error(
        &self,
        store: &VaultStore,
        stage: &'static str,
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

    fn record_failure(&self, store: &VaultStore, stage: &'static str) -> AnyResult<()> {
        self.vault.append_audit(
            store,
            AuditAction::BrokeredRunFailed,
            brokered_run_failure_details(&self.run_id, stage),
        )?;
        Ok(())
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

        let now = now_ms();
        let salt = random_array::<SALT_LEN>()?;
        let dek = Zeroizing::new(random_array::<KEY_LEN>()?);
        let header = VaultHeader {
            magic: MAGIC.into(),
            version: FORMAT_VERSION,
            vault_id: ulid::Ulid::new().to_string(),
            created_at_ms: now,
            kdf: KdfParams::default(),
            salt_b64: B64.encode(salt),
            aead: AEAD_ALGORITHM.into(),
        };
        validate_header(&header).map_err(|error| {
            classify_source(
                VaultErrorKind::Internal,
                "constructed vault header is invalid",
                error,
            )
        })?;
        let wrapped_dek_aad = payload_aad(&header, AeadRole::WrappedDek);
        let state_aad = payload_aad(&header, AeadRole::State);
        let wrap_key = derive_wrap_key(passphrase, &salt, &header.kdf)?;
        let wrapped_dek_nonce = random_array::<NONCE_LEN>()?;
        let wrapped_dek = seal(
            &wrap_key,
            &wrapped_dek_nonce,
            &wrapped_dek_aad,
            dek.as_ref(),
        )?;
        let state_nonce = random_array::<NONCE_LEN>()?;
        let state_plaintext = Zeroizing::new(serde_json::to_vec(&VaultState::default())?);
        let state = seal(&dek, &state_nonce, &state_aad, &state_plaintext)?;
        let file = VaultFile {
            header,
            wrapped_dek_nonce_b64: B64.encode(wrapped_dek_nonce),
            wrapped_dek_b64: B64.encode(wrapped_dek),
            state_nonce_b64: B64.encode(state_nonce),
            state_b64: B64.encode(state),
        };
        let file_text = serde_json::to_string_pretty(&file)?;
        let audit_key = derive_audit_key(&dek)?;
        if let Err(error) = AuditEvent::append_unlocked(
            self,
            audit_key.as_ref(),
            AuditAction::VaultInitialized,
            serde_json::json!({
                "vault_id": file.header.vault_id,
            }),
        ) {
            let cleanup_error = rollback_failed_init(self);
            let error = error.context("failed to initialize vault audit log");
            return match cleanup_error {
                Some(cleanup_error) => Err(error.context(cleanup_error)),
                None => Err(error),
            };
        }
        if let Err(error) = self.write_vault_text_unlocked(&file_text) {
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
            AuditEvent::append_unlocked(self, vault.audit_key.as_ref(), action, details(&result))
                .map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit append failed before state save",
                    error,
                )
            })?;
            vault.save_unlocked(self).map_err(|error| {
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

    pub(crate) fn prepare_brokered_run(
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
            let resolved = resolve_brokered_run(&vault, request).map_err(|error| {
                brokered_failure_error_unlocked(self, &vault, &run_id, "resolve", error)
            })?;
            Ok(PreparedBrokeredRun {
                handle: BrokeredRunHandle { vault, run_id },
                resolved,
            })
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
        match run_brokered(prepared.resolved) {
            Ok(output) => {
                prepared
                    .handle
                    .record_finish(self, &output)
                    .map_err(|error| {
                        crate::VaultError::from_anyhow(VaultErrorKind::AuditTampered, error)
                    })?;
                Ok(output)
            }
            Err(error) => {
                Err(prepared
                    .handle
                    .failure_error(self, "process", VaultErrorKind::Process, error))
            }
        }
    }

    fn open_unlocked(&self, passphrase: &SecretString) -> AnyResult<OpenVault> {
        let text = self.read_vault_text()?.ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault does not exist at {}", self.vault_path().display()),
            )
        })?;
        let file: VaultFile = serde_json::from_str(&text).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "failed to parse vault file",
                error.into(),
            )
        })?;
        validate_header(&file.header).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault header is invalid",
                error,
            )
        })?;
        let wrapped_dek_aad = payload_aad(&file.header, AeadRole::WrappedDek);
        let state_aad = payload_aad(&file.header, AeadRole::State);
        let salt =
            decode_b64_array::<SALT_LEN>("vault salt", &file.header.salt_b64).map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault salt is invalid",
                    error,
                )
            })?;
        let wrap_key = derive_wrap_key(passphrase, &salt, &file.header.kdf).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault KDF parameters are invalid",
                error,
            )
        })?;
        let wrapped_dek_nonce =
            decode_b64_array::<NONCE_LEN>("wrapped vault key nonce", &file.wrapped_dek_nonce_b64)
                .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "wrapped vault key nonce is invalid",
                    error,
                )
            })?;
        let wrapped_dek = B64.decode(&file.wrapped_dek_b64).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "wrapped vault key is not valid base64",
                error.into(),
            )
        })?;
        let dek_plaintext = open(
            &wrap_key,
            &wrapped_dek_nonce,
            &wrapped_dek_aad,
            &wrapped_dek,
        )
        .map_err(|error| {
            classify_source(
                VaultErrorKind::Authentication,
                "failed to unlock vault key",
                error,
            )
        })?;
        let dek = Zeroizing::new(
            decode_array::<KEY_LEN>("vault key", &dek_plaintext).map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault key has invalid length",
                    error,
                )
            })?,
        );
        let state_nonce = decode_b64_array::<NONCE_LEN>("vault state nonce", &file.state_nonce_b64)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault state nonce is invalid",
                    error,
                )
            })?;
        let state_ciphertext = B64.decode(&file.state_b64).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault state is not valid base64",
                error.into(),
            )
        })?;
        let state_plaintext =
            open(&dek, &state_nonce, &state_aad, &state_ciphertext).map_err(|error| {
                classify_source(
                    VaultErrorKind::Authentication,
                    "failed to decrypt vault state",
                    error,
                )
            })?;
        let state = serde_json::from_slice(&state_plaintext).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "failed to parse vault state",
                error.into(),
            )
        })?;
        let audit_key = derive_audit_key(&dek).map_err(|error| {
            classify_source(
                VaultErrorKind::Internal,
                "failed to derive vault audit key",
                error,
            )
        })?;

        Ok(OpenVault {
            file,
            state,
            dek,
            audit_key,
        })
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

    pub(crate) fn set_secret(
        &mut self,
        name: &SecretName,
        mut value: SecretBytes,
    ) -> AnyResult<()> {
        validate_secret_value_len(value.len())?;
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
        };
        value.zeroize();
        // Replaced entries are dropped here; `SecretEntry::drop` zeroizes the
        // displaced base64 value.
        self.state.secrets.insert(name.as_str().to_string(), entry);
        Ok(())
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
        validate_serialized_secret_value_len(name, entry)?;
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

    fn save_unlocked(&self, store: &VaultStore) -> AnyResult<()> {
        // Keep the state AAD derived from the immutable, validated header that
        // was parsed at open/init time. Header-changing migrations must update
        // wrapped key and state encryption together.
        let aad = payload_aad(&self.file.header, AeadRole::State);
        let state_nonce = random_array::<NONCE_LEN>()?;
        let state_plaintext = Zeroizing::new(serde_json::to_vec(&self.state)?);
        let encrypted_state = seal(&self.dek, &state_nonce, &aad, &state_plaintext)?;
        let file = VaultFile {
            header: self.file.header.clone(),
            wrapped_dek_nonce_b64: self.file.wrapped_dek_nonce_b64.clone(),
            wrapped_dek_b64: self.file.wrapped_dek_b64.clone(),
            state_nonce_b64: B64.encode(state_nonce),
            state_b64: B64.encode(encrypted_state),
        };
        store.write_vault_text_unlocked(&serde_json::to_string_pretty(&file)?)?;
        Ok(())
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

fn brokered_run_failure_details(run_id: &str, stage: &'static str) -> serde_json::Value {
    serde_json::json!({
        "run_id": run_id,
        "stage": stage,
        // Do not record the original error text here. Spawn/process errors can
        // include argv or paths, and audit logs are value-free metadata.
        "error": "brokered run failed",
    })
}

fn brokered_failure_error_unlocked(
    store: &VaultStore,
    vault: &OpenVault,
    run_id: &str,
    stage: &'static str,
    error: anyhow::Error,
) -> anyhow::Error {
    let kind = classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
    if let Err(audit_error) = vault.append_audit_unlocked(
        store,
        AuditAction::BrokeredRunFailed,
        brokered_run_failure_details(run_id, stage),
    ) {
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

fn validate_serialized_secret_value_len(name: &SecretName, entry: &SecretEntry) -> AnyResult<()> {
    if entry.value_len < MIN_REDACTABLE_LEN || entry.value_len > MAX_SECRET_VALUE_LEN {
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
