//! Audited state commits shared by initialization and ordinary edits.
//!
//! A version 3 state transition advances the generation once, prepares its
//! exact mutation audit event, and seals that event's MAC into the encrypted
//! state before the event is appended. Audit-only events and no-op edits never
//! advance the generation.

use anyhow::Result as AnyResult;
use secrecy::SecretString;
use serde_json::Value;

use crate::VaultErrorKind;
use crate::audit::{AuditAction, AuditEvent, PreparedAuditAppend, verify_chain_unlocked};
use crate::error::{classified, classify_source};
use crate::format::LATEST_FORMAT_VERSION;
use crate::store::VaultStore;
use crate::store::witness::TransactionKind;

use super::envelope::NewVaultMaterial;
use super::transaction::InPlaceCommit;
use super::{OpenVault, VaultEditPrecondition, now_ms};
use crate::passphrase_policy::validate_new_vault_passphrase_inner;

#[cfg(any(test, feature = "test-utils"))]
impl super::Vault {
    /// Initializes a fixture vault in an explicit format without applying the
    /// new-passphrase policy, modelling a vault written by an earlier release.
    ///
    /// Available only to this crate's tests and consumers that enable the
    /// `test-utils` feature.
    #[doc(hidden)]
    pub fn init_format_for_test(
        &self,
        passphrase: &SecretString,
        version: u32,
    ) -> crate::Result<()> {
        self.store
            .with_lock(|| {
                let material = NewVaultMaterial::generate(
                    version,
                    now_ms(),
                    self.store.initialization_kdf().clone(),
                )?;
                self.store.init_with_material_unlocked(passphrase, material)
            })
            .map_err(|error| crate::error::vault_error_from_anyhow(VaultErrorKind::Internal, error))
    }
}

/// Audit detail binding a version 3 mutation event to the generation it
/// commits.
pub(super) const GENERATION_DETAIL: &str = "generation";

/// Adds the committed generation to a version 3 mutation event.
pub(super) fn details_with_generation(details: Value, generation: u64) -> AnyResult<Value> {
    let Value::Object(mut map) = details else {
        return Err(classified(
            VaultErrorKind::Internal,
            "vault mutation audit details must be an object",
        ));
    };
    if map.contains_key(GENERATION_DETAIL) {
        return Err(classified(
            VaultErrorKind::Internal,
            "vault mutation audit details use the reserved generation key",
        ));
    }
    map.insert(GENERATION_DETAIL.into(), generation.into());
    Ok(Value::Object(map))
}

/// Prepares the mutation event that will commit `generation`.
pub(super) fn prepare_v3_mutation_event(
    store: &VaultStore,
    audit_key: &[u8],
    action: AuditAction,
    details: Value,
    generation: u64,
) -> AnyResult<PreparedAuditAppend> {
    let details = details_with_generation(details, generation)?;
    AuditEvent::prepare_append_unlocked(store, audit_key, action, details).map_err(|error| {
        classify_source(
            VaultErrorKind::AuditTampered,
            "vault audit append failed before state save",
            error,
        )
    })
}

impl OpenVault {
    /// Advances a version 3 state to its next generation and anchors it to
    /// the prepared mutation event. Returns `None` for earlier formats.
    pub(super) fn stage_v3_mutation(
        &mut self,
        store: &VaultStore,
        action: AuditAction,
        details: Value,
    ) -> AnyResult<Option<PreparedAuditAppend>> {
        let Some(current) = self.state.v3.as_ref().map(|fields| fields.generation) else {
            return Ok(None);
        };
        let generation = current.checked_add(1).ok_or_else(|| {
            classified(
                VaultErrorKind::Internal,
                "vault state generation cannot advance further",
            )
        })?;
        let prepared =
            prepare_v3_mutation_event(store, self.audit_key.as_ref(), action, details, generation)?;
        let fields = self
            .state
            .v3
            .as_mut()
            .expect("version 3 fields were present");
        fields.generation = generation;
        fields.mutation_audit_mac = prepared.mac().to_owned();
        Ok(Some(prepared))
    }

    fn v3_fingerprint(&self) -> AnyResult<Option<[u8; 32]>> {
        self.state
            .v3
            .as_ref()
            .map(|_| self.state.secrets_fingerprint())
            .transpose()
    }
}

impl VaultStore {
    pub(super) fn init_unlocked(&self, passphrase: &SecretString) -> AnyResult<()> {
        // A retried init resumes its own interrupted transaction, which the
        // original passphrase authenticates, instead of failing on the
        // partially written home.
        if self
            .recover_pending_unlocked(&[passphrase])?
            .is_some_and(|recovered| recovered.kind == TransactionKind::Init)
        {
            return Ok(());
        }
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
        let material = NewVaultMaterial::generate(
            LATEST_FORMAT_VERSION,
            now_ms(),
            self.initialization_kdf().clone(),
        )?;
        self.init_with_material_unlocked(passphrase, material)
    }

    /// Appends the initialization event and writes the sealed empty vault.
    /// Callers own existence and passphrase-policy checks.
    pub(super) fn init_with_material_unlocked(
        &self,
        passphrase: &SecretString,
        material: NewVaultMaterial,
    ) -> AnyResult<()> {
        let mut details = serde_json::json!({ "vault_id": material.vault_id() });
        if material.is_v3() {
            details = details_with_generation(details, 1)?;
        }
        let prepared = AuditEvent::prepare_append_unlocked(
            self,
            material.audit_key(),
            AuditAction::VaultInitialized,
            details,
        )
        .map_err(|error| error.context("failed to initialize vault audit log"))?;
        let mutation_audit_mac = material.is_v3().then(|| prepared.mac().to_owned());
        let vault_id = material.vault_id().to_owned();
        let is_v3 = material.is_v3();
        let envelope = material.seal(passphrase, mutation_audit_mac)?;
        if is_v3 {
            // Nothing reaches the home before the durable pending marker, so
            // a failure leaves no partial vault to roll back.
            return self.commit_in_place_unlocked(InPlaceCommit {
                kind: TransactionKind::Init,
                vault_id: &vault_id,
                previous_envelope_sha256: None,
                candidate: &envelope.file_text,
                generation: 1,
                prepared: &prepared,
            });
        }
        if let Err(error) = prepared.commit_unlocked(self) {
            return Err(with_init_rollback(
                self,
                error.context("failed to initialize vault audit log"),
            ));
        }
        if let Err(error) = self.write_vault_text_unlocked(&envelope.file_text) {
            return Err(with_init_rollback(
                self,
                error.context("failed to write initialized vault file"),
            ));
        }
        Ok(())
    }

    pub(super) fn edit_with_audit_if_precondition<R>(
        &self,
        passphrase: &SecretString,
        precondition: VaultEditPrecondition<'_>,
        action: AuditAction,
        edit: impl FnOnce(&mut OpenVault) -> AnyResult<R>,
        should_commit: impl FnOnce(&R) -> bool,
        details: impl FnOnce(&R) -> Value,
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
            precondition.enforce(
                &vault,
                "vault state changed since the metadata snapshot; refresh and retry",
            )?;
            let before = vault.v3_fingerprint()?;
            let result = edit(&mut vault)?;
            if !should_commit(&result) {
                return Ok(result);
            }
            let details = details(&result);
            if before.is_some() && before == vault.v3_fingerprint()? {
                // A version 3 edit that leaves the state unchanged is only an
                // audit record: no generation advance and no state save.
                vault
                    .append_audit_unlocked(self, action, details)
                    .map_err(|error| {
                        classify_source(
                            VaultErrorKind::AuditTampered,
                            "vault audit append failed",
                            error,
                        )
                    })?;
                return Ok(result);
            }
            self.commit_state_unlocked(&mut vault, action, details)?;
            Ok(result)
        })
    }

    /// Persists an edited state: audit intent first, then the envelope. A
    /// crash between the two can leave the audit ahead of state, never the
    /// state ahead of its audit.
    fn commit_state_unlocked(
        &self,
        vault: &mut OpenVault,
        action: AuditAction,
        details: Value,
    ) -> AnyResult<()> {
        let prepared = vault.stage_v3_mutation(self, action, details.clone())?;
        let envelope = vault.prepare_save_unlocked()?;
        let file_text = envelope.serialize_pretty()?;
        self.validate_vault_text_len(&file_text).map_err(|error| {
            classify_source(
                VaultErrorKind::InvalidInput,
                "vault state is too large to save safely",
                error,
            )
        })?;
        if let Some(prepared) = prepared {
            let generation = vault
                .state
                .v3
                .as_ref()
                .map_or(0, |fields| fields.generation);
            return self.commit_in_place_unlocked(InPlaceCommit {
                kind: TransactionKind::Edit,
                vault_id: &vault.file.header.vault_id,
                previous_envelope_sha256: Some(&vault.envelope_sha256),
                candidate: &file_text,
                generation,
                prepared: &prepared,
            });
        }
        vault
            .append_audit_unlocked(self, action, details)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit append failed before state save",
                    error,
                )
            })?;
        self.write_vault_text_unlocked(&file_text).map_err(|error| {
            classify_source(
                VaultErrorKind::Io,
                "vault audit was appended, but state save failed",
                error,
            )
        })
    }
}

fn with_init_rollback(store: &VaultStore, error: anyhow::Error) -> anyhow::Error {
    match rollback_failed_init(store) {
        Some(cleanup_error) => error.context(cleanup_error),
        None => error,
    }
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
