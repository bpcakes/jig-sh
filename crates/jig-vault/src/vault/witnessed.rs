//! Authenticated open and audit-only append boundaries against the
//! out-of-home witness.
//!
//! Every authenticated read or edit first finishes any pending transaction
//! recorded for this home, then checks the unlocked state against the
//! witness: a format 3 state must match its committed checkpoint exactly and
//! be anchored to its mutation audit event, and an older-format copy of a
//! witnessed ID is refused as a replay. The first authenticated use of an
//! unwitnessed format 3 vault enrolls it. Status, info, and doctor stay
//! read-only header observations and never reach this boundary.

use anyhow::Result as AnyResult;
use secrecy::SecretString;

use crate::VaultErrorKind;
use crate::audit::{RetainedAuditKey, find_verified_event_unlocked};
use crate::error::{classified, classify_source};
use crate::format::V3_FORMAT_VERSION;
use crate::store::VaultStore;
use crate::store::witness::{Checkpoint, WitnessRecord, sha256_hex};

use super::OpenVault;
use super::commit::GENERATION_DETAIL;
use super::envelope::ParsedVaultEnvelope;
use super::transaction::Recovered;

impl VaultStore {
    /// Opens the live vault with `credentials[0]` after finishing any
    /// pending transaction any of `credentials` authenticates.
    pub(super) fn open_witnessed_unlocked(
        &self,
        credentials: &[&SecretString],
    ) -> AnyResult<(OpenVault, Option<Recovered>)> {
        let recovered = self.recover_pending_unlocked(credentials)?;
        let bytes = self.read_vault_bytes()?.ok_or_else(|| {
            classified(
                VaultErrorKind::NotFound,
                format!("vault does not exist at {}", self.vault_path().display()),
            )
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "failed to parse vault file",
                error.into(),
            )
        })?;
        let unlocked = ParsedVaultEnvelope::parse(text)?
            .validate()?
            .unlock(credentials[0])?;
        let vault = OpenVault::from_unlocked(unlocked, sha256_hex(&bytes));
        self.verify_witnessed_state_unlocked(&vault)?;
        Ok((vault, recovered))
    }

    fn verify_witnessed_state_unlocked(&self, vault: &OpenVault) -> AnyResult<()> {
        let vault_id = &vault.file.header.vault_id;
        let Some(fields) = vault.state.v3.as_ref() else {
            let Some(witness) = self.witness().open_existing()? else {
                return Ok(());
            };
            let _id = witness.lock_id(vault_id)?;
            if witness.read_record(vault_id)?.is_some() {
                return Err(legacy_replay_error());
            }
            return Ok(());
        };
        self.verify_mutation_anchor_unlocked(vault)?;
        let witness = self.witness().open_or_create()?;
        let _id = witness.lock_id(vault_id)?;
        let checkpoint = Checkpoint {
            generation: fields.generation,
            envelope_sha256: vault.envelope_sha256.clone(),
            mutation_audit_mac: fields.mutation_audit_mac.clone(),
        };
        let Some(record) = witness.read_record(vault_id)? else {
            // First authenticated use on this profile establishes the
            // baseline. A missing witness is indistinguishable from first
            // use and is outside the rollback guarantee.
            let mut record = WitnessRecord::new(vault_id);
            record.committed = Some(checkpoint);
            return witness.write_record(&record).map_err(|error| {
                classify_source(
                    VaultErrorKind::Io,
                    "failed to enroll the vault in its rollback witness",
                    error,
                )
            });
        };
        if let Some(pending) = &record.pending {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!(
                    "a vault {} for this vault is pending at another vault home; finish it there before using this copy",
                    pending.operation.label()
                ),
            ));
        }
        let committed = record
            .committed
            .as_ref()
            .expect("validated records without pending have a checkpoint");
        compare_with_checkpoint(&checkpoint, committed)
    }

    pub(super) fn verify_mutation_anchor_unlocked(&self, vault: &OpenVault) -> AnyResult<()> {
        let fields = vault.state.v3.as_ref().expect("format 3 state");
        self.verify_anchor_unlocked(
            vault.audit_key.as_ref(),
            &fields.mutation_audit_mac,
            fields.generation,
        )
    }

    /// Requires `mutation_audit_mac` to name an event in the verified chain
    /// that committed exactly `generation`.
    fn verify_anchor_unlocked(
        &self,
        audit_key: &[u8],
        mutation_audit_mac: &str,
        generation: u64,
    ) -> AnyResult<()> {
        if !self.audit_exists()? {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                format!(
                    "vault audit log is missing at {}; restore audit.jsonl before continuing",
                    self.audit_path().display()
                ),
            ));
        }
        let (_, anchor) = find_verified_event_unlocked(self, audit_key, mutation_audit_mac)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
        let anchored = anchor.is_some_and(|event| {
            event
                .details
                .get(GENERATION_DETAIL)
                .and_then(serde_json::Value::as_u64)
                == Some(generation)
        });
        if !anchored {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "vault state is not anchored to its mutation audit event",
            ));
        }
        Ok(())
    }

    /// Guards appends from retained reveal, exec, broker, and backup
    /// handles, which never reopen the vault with a passphrase. The identity
    /// the handle authenticated, never the current public header, selects
    /// the witness record. They compare the current persisted envelope, not
    /// the handle's old state, so an in-flight operation can still finish
    /// after a completed rotation; the audit root they retain is stable
    /// across rotation and verifies the committed mutation anchor.
    pub(crate) fn guard_audit_only_append_unlocked(
        &self,
        retained: &RetainedAuditKey,
    ) -> AnyResult<()> {
        let vault_id = retained.vault_id();
        let current = match self.read_vault_bytes()? {
            Some(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|error| {
                    classify_source(
                        VaultErrorKind::Serialization,
                        "failed to parse vault file",
                        error.into(),
                    )
                })?;
                let header = ParsedVaultEnvelope::parse(text)?.into_header();
                if header.vault_id != vault_id {
                    return Err(classified(
                        VaultErrorKind::AuditTampered,
                        "vault identity changed after this operation authenticated it; refusing to record more activity",
                    ));
                }
                Some((header.version == V3_FORMAT_VERSION, bytes))
            }
            None => None,
        };
        let witness = self.witness().open_existing()?;
        let _id = witness
            .as_ref()
            .map(|witness| witness.lock_id(vault_id))
            .transpose()?;
        let record = match &witness {
            Some(witness) => witness.read_record(vault_id)?,
            None => None,
        };
        match (record, current) {
            (Some(record), _) if record.pending.is_some() => Err(classified(
                VaultErrorKind::AuditTampered,
                "a vault transaction is pending; finish it with an authenticated vault command before recording more activity",
            )),
            (Some(record), Some((true, bytes))) => {
                let committed = record.committed.expect("validated record");
                if sha256_hex(&bytes) != committed.envelope_sha256 {
                    return Err(classified(
                        VaultErrorKind::AuditTampered,
                        "vault state no longer matches its witnessed checkpoint",
                    ));
                }
                self.verify_anchor_unlocked(
                    retained.key(),
                    &committed.mutation_audit_mac,
                    committed.generation,
                )
            }
            (Some(_), Some((false, _))) => Err(legacy_replay_error()),
            (Some(_), None) => Err(classified(
                VaultErrorKind::AuditTampered,
                "vault state no longer matches its witnessed checkpoint",
            )),
            (None, Some((true, _))) => Err(unwitnessed_error()),
            (None, _) => Ok(()),
        }
    }
}

impl OpenVault {
    /// The audit root and authenticated identity a retained handle keeps.
    pub(super) fn into_retained_audit_key(self) -> RetainedAuditKey {
        let Self {
            file, audit_key, ..
        } = self;
        RetainedAuditKey::new(audit_key, file.header.vault_id)
    }

    pub(super) fn retained_audit_key(&self) -> RetainedAuditKey {
        RetainedAuditKey::new(self.audit_key.clone(), self.file.header.vault_id.clone())
    }
}

fn compare_with_checkpoint(current: &Checkpoint, committed: &Checkpoint) -> AnyResult<()> {
    if current.generation < committed.generation {
        return Err(classified(
            VaultErrorKind::AuditTampered,
            format!(
                "vault state generation {} is older than its witnessed generation {}; refusing a rolled-back copy. {STALE_COPY_RECOVERY_GUIDANCE}",
                current.generation, committed.generation
            ),
        ));
    }
    if current.generation > committed.generation {
        return Err(classified(
            VaultErrorKind::AuditTampered,
            format!(
                "vault state is newer than its witnessed checkpoint without a recorded transaction; refusing an unwitnessed fork. {PROFILE_RECOVERY_GUIDANCE}"
            ),
        ));
    }
    if current != committed {
        return Err(classified(
            VaultErrorKind::AuditTampered,
            format!(
                "vault state forks from its witnessed checkpoint at the same generation. {PROFILE_RECOVERY_GUIDANCE}"
            ),
        ));
    }
    Ok(())
}

const PROFILE_RECOVERY_GUIDANCE: &str = "Operator step: if another user profile can still authenticate the intended vault, create an encrypted backup there and restore it to an absent target on this profile using the documented recovery procedure. Agents must ask the operator. Never delete or edit the rollback witness to bypass this refusal.";

const STALE_COPY_RECOVERY_GUIDANCE: &str = "Operator step: use the current vault home, or the restored home if a backup was restored. If the current copy is unavailable, use the documented authenticated backup recovery procedure to restore to an absent target. Agents must ask the operator. Never delete or edit the rollback witness or its journals to bypass this refusal.";

fn legacy_replay_error() -> anyhow::Error {
    classified(
        VaultErrorKind::AuditTampered,
        format!(
            "this vault ID was already witnessed as format 3; refusing an older-format copy. {STALE_COPY_RECOVERY_GUIDANCE}"
        ),
    )
}

fn unwitnessed_error() -> anyhow::Error {
    classified(
        VaultErrorKind::AuditTampered,
        "this format 3 vault has no witnessed checkpoint; reopen it with its passphrase first",
    )
}
