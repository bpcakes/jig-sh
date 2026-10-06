//! Witnessed transactions for format 3 state changes.
//!
//! Every committed state transition first persists a target-keyed journal,
//! then the vault ID's pending marker naming that journal by digest, and
//! only then touches the live audit log and envelope before promoting the
//! marker to a committed checkpoint. The durable pending marker is the
//! irrevocable commit point: before it, failures leave the previous state
//! usable; after it, only completion of the recorded transaction is allowed.
//! Every completion step is idempotent, so recovery can replay it after a
//! crash at any sync or rename boundary.

use anyhow::{Result as AnyResult, bail};
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::VaultErrorKind;
use crate::audit::{PreparedAuditAppend, verify_exact_prefix};
use crate::crypto::KEY_LEN;
use crate::error::{classified, classify_source};
use crate::store::witness::{
    AuditTransition, Checkpoint, InPlacePayload, JOURNAL_SCHEMA, Journal, JournalPayload,
    JournalTarget, PendingMarker, TargetJournal, TransactionKind, WitnessRecord, WitnessStore,
    directory_identity, sha256_hex,
};
use crate::store::{AUDIT_TEXT_READ_LIMIT, FaultPoint, VaultStore};

use super::envelope::ParsedVaultEnvelope;

/// One in-place format 3 transition whose candidate is already sealed and
/// whose mutation event is already prepared under the held locks.
pub(super) struct InPlaceCommit<'a> {
    pub(super) kind: TransactionKind,
    pub(super) vault_id: &'a str,
    /// Digest of the exact live envelope replaced; `None` for initialization.
    pub(super) previous_envelope_sha256: Option<&'a str>,
    pub(super) candidate: &'a str,
    pub(super) generation: u64,
    pub(super) prepared: &'a PreparedAuditAppend,
}

/// The transaction a recovery finished, and which offered credential
/// authenticated its candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Recovered {
    pub(super) kind: TransactionKind,
    pub(super) credential_index: usize,
}

impl VaultStore {
    pub(super) fn commit_in_place_unlocked(&self, commit: InPlaceCommit<'_>) -> AnyResult<()> {
        let InPlaceCommit {
            kind,
            vault_id,
            previous_envelope_sha256,
            candidate,
            generation,
            prepared,
        } = commit;
        let witness = self.witness().open_or_create().map_err(record_error)?;
        let _id = witness.lock_id(vault_id).map_err(record_error)?;
        let record = witness.read_record(vault_id).map_err(record_error)?;
        if record
            .as_ref()
            .is_some_and(|record| record.pending.is_some())
        {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                "another vault transaction is pending for this vault; finish it before starting a new one",
            ));
        }
        let next = Checkpoint {
            generation,
            envelope_sha256: sha256_hex(candidate.as_bytes()),
            mutation_audit_mac: prepared.mac().to_owned(),
        };
        let journal = Journal {
            schema: JOURNAL_SCHEMA,
            operation: kind,
            vault_id: vault_id.to_owned(),
            target: self.journal_target().map_err(record_error)?,
            previous: record.as_ref().and_then(|record| record.committed.clone()),
            previous_envelope_sha256: previous_envelope_sha256.map(str::to_owned),
            next: next.clone(),
            payload: JournalPayload::InPlace(InPlacePayload {
                candidate_envelope: candidate.to_owned(),
                audit: prepared.transition(),
            }),
        };
        // The home receives the audit append and envelope, and the journal
        // binds the current audit prefix and predecessor envelope; an
        // interrupted earlier attempt (such as an init's home creation or an
        // audit-only append) may have left any of them unsynced.
        crate::store::ensure_entry_chain_durable(self.root()).map_err(record_error)?;
        self.sync_existing_state_unlocked().map_err(record_error)?;
        self.fault(FaultPoint::BeforeJournal)
            .map_err(record_error)?;
        let journal_sha256 = witness.write_journal(&journal).map_err(record_error)?;
        self.fault(FaultPoint::AfterJournal).map_err(record_error)?;
        let mut pending = record.unwrap_or_else(|| WitnessRecord::new(vault_id));
        pending.pending = Some(PendingMarker {
            operation: kind,
            target_key: journal.target.target_key.clone(),
            journal_sha256,
            next,
        });
        witness.write_record(&pending).map_err(record_error)?;
        // The transaction is now irrevocable: report failures as pending.
        self.fault(FaultPoint::AfterPending)
            .and_then(|()| self.finish_in_place(&witness, &journal, pending, None))
            .map_err(|error| pending_error(kind, error))
    }

    /// Finishes an in-place transaction. Each step accepts its own completed
    /// result, so replay after any crash converges on the recorded successor.
    fn finish_in_place(
        &self,
        witness: &WitnessStore,
        journal: &Journal,
        mut record: WitnessRecord,
        verify_with: Option<&[u8; KEY_LEN]>,
    ) -> AnyResult<()> {
        let JournalPayload::InPlace(payload) = &journal.payload else {
            bail!("vault transaction journal does not describe an in-place change");
        };
        self.apply_audit_transition(&payload.audit, verify_with)?;
        self.fault(FaultPoint::AfterAudit)?;
        self.install_candidate(journal, &payload.candidate_envelope)?;
        self.fault(FaultPoint::AfterEnvelope)?;
        witness.promote(journal, &mut record)
    }

    /// Brings the audit log from the recorded prefix to exactly the recorded
    /// successor. Only the captured torn suffix or a byte-matching partial
    /// intended append may be replaced; anything else fails closed.
    fn apply_audit_transition(
        &self,
        transition: &AuditTransition,
        verify_with: Option<&[u8; KEY_LEN]>,
    ) -> AnyResult<()> {
        let current = self
            .read_audit_bytes_bounded(AUDIT_TEXT_READ_LIMIT as usize)?
            .map(|bytes| bytes.to_vec())
            .unwrap_or_default();
        let prefix_len = usize::try_from(transition.prefix_len)?;
        if current.len() < prefix_len {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "vault audit log is shorter than the pending transaction's recorded prefix",
            ));
        }
        if let Some(key) = verify_with {
            let tip = verify_exact_prefix(&current[..prefix_len], key).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit prefix of the pending transaction failed verification",
                    error,
                )
            })?;
            if tip != transition.prefix_tip_mac {
                return Err(classified(
                    VaultErrorKind::AuditTampered,
                    "vault audit prefix no longer matches the pending transaction",
                ));
            }
        }
        let suffix = &current[prefix_len..];
        let append = transition.append.as_bytes();
        if suffix == append {
            return self.sync_state_file_unlocked(&self.audit_path());
        }
        let captured_torn_suffix = transition.torn_suffix_len > 0
            && suffix.len() as u64 == transition.torn_suffix_len
            && transition.torn_suffix_sha256.as_deref() == Some(sha256_hex(suffix).as_str());
        if !captured_torn_suffix && !append.starts_with(suffix) {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "vault audit log has unexpected bytes after the pending transaction's prefix",
            ));
        }
        if !suffix.is_empty() {
            self.truncate_audit_unlocked(transition.prefix_len)?;
        }
        if let Err(error) = self.fault(FaultPoint::PartialAudit) {
            self.append_audit_bytes_unlocked(&append[..append.len() / 2])?;
            return Err(error);
        }
        self.append_audit_bytes_unlocked(append)
    }

    /// Installs the recorded candidate only over the exact recorded
    /// predecessor, accepting an already installed candidate once it is
    /// durable.
    fn install_candidate(&self, journal: &Journal, candidate: &str) -> AnyResult<()> {
        let current = self
            .read_vault_bytes()?
            .map(|bytes| sha256_hex(bytes.as_slice()));
        if current.as_deref() == Some(journal.next.envelope_sha256.as_str()) {
            return self.sync_state_file_unlocked(&self.vault_path());
        }
        if current != journal.previous_envelope_sha256 {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "vault envelope changed while a transaction was pending; refusing to overwrite it",
            ));
        }
        self.write_vault_text_unlocked(candidate)
    }

    pub(super) fn journal_target(&self) -> AnyResult<JournalTarget> {
        let parent = self
            .root()
            .parent()
            .ok_or_else(|| anyhow::anyhow!("vault home has no parent directory"))?;
        let (parent_device, parent_inode) = directory_identity(parent)?;
        Ok(JournalTarget {
            target_key: self.target_key(),
            parent_device,
            parent_inode,
        })
    }

    /// Finishes a pending transaction recorded for this target, after
    /// authenticating its candidate with one of `credentials`. Whether the
    /// target's journal is still referenced is established from every
    /// witness record, never from the journal's own claims; only an
    /// established orphan is discarded.
    pub(super) fn recover_pending_unlocked(
        &self,
        credentials: &[&SecretString],
    ) -> AnyResult<Option<Recovered>> {
        let Some(witness) = self.witness().open_existing()? else {
            return Ok(None);
        };
        let target_key = self.target_key();
        // Classification and any orphan deletion happen under this one
        // acquisition of the target lock (reentrant under `with_lock`).
        let target_lock = witness.lock_target(&target_key)?;
        let header_pending = match self.header_vault_id_for_lock() {
            Some(id) => {
                let _id = witness.lock_id(&id)?;
                witness.read_record(&id)?.and_then(|record| record.pending)
            }
            None => None,
        };
        if let Some(pending) = &header_pending
            && pending.target_key != target_key
        {
            return Err(classified(
                VaultErrorKind::AlreadyExists,
                format!(
                    "a vault {} for this vault is pending at another vault home; finish it there before using this copy",
                    pending.operation.label()
                ),
            ));
        }
        let (vault_id, record, journal) = match witness
            .classify_target_journal(&target_lock)
            .map_err(fail_closed)?
        {
            TargetJournal::Absent => return Ok(None),
            TargetJournal::Orphan(orphan) => {
                witness.delete_orphan_journal(orphan)?;
                return Ok(None);
            }
            TargetJournal::Referenced {
                vault_id,
                record,
                journal,
            } => (vault_id, *record, journal),
        };
        let _id = witness.lock_id(&vault_id)?;
        // Only this target's lock holder may change a marker naming it;
        // re-reading under the ID lock confirms nothing else did.
        if witness.read_record(&vault_id)?.as_ref() != Some(&record) {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "the pending vault transaction changed while it was being recovered",
            ));
        }
        let credential_index = match &journal.payload {
            JournalPayload::InPlace(_) => {
                self.check_journal_target(&journal)?;
                let (key, index) = authenticate_candidate(&journal, credentials)?;
                self.finish_in_place(&witness, &journal, record, Some(&key))?;
                index
            }
            JournalPayload::Restore(_) => {
                self.recover_pending_restore(&witness, &journal, record, credentials)?
            }
        };
        Ok(Some(Recovered {
            kind: journal.operation,
            credential_index,
        }))
    }

    fn check_journal_target(&self, journal: &Journal) -> AnyResult<()> {
        let current = self.journal_target()?;
        if current != journal.target {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "the pending vault transaction was recorded for a different directory at this path",
            ));
        }
        Ok(())
    }
}

/// Authenticates a pending candidate and checks every binding the marker
/// and journal record. Returns its audit root for prefix verification.
fn authenticate_candidate(
    journal: &Journal,
    credentials: &[&SecretString],
) -> AnyResult<(Zeroizing<[u8; KEY_LEN]>, usize)> {
    let JournalPayload::InPlace(payload) = &journal.payload else {
        bail!("vault transaction journal does not describe an in-place change");
    };
    for (index, credential) in credentials.iter().enumerate() {
        let unlocked = ParsedVaultEnvelope::parse(&payload.candidate_envelope)?
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
            &payload.candidate_envelope,
        )?;
        return Ok((unlocked.audit_key, index));
    }
    Err(pending_credential_error(journal.operation))
}

pub(super) fn check_candidate_bindings(
    journal: &Journal,
    vault_id: &str,
    fields: Option<&crate::format::V3StateFields>,
    candidate: &str,
) -> AnyResult<()> {
    let Some(fields) = fields else {
        bail!("pending vault candidate is not a format 3 state");
    };
    if vault_id != journal.vault_id
        || fields.generation != journal.next.generation
        || fields.mutation_audit_mac != journal.next.mutation_audit_mac
        || sha256_hex(candidate.as_bytes()) != journal.next.envelope_sha256
    {
        return Err(classified(
            VaultErrorKind::AuditTampered,
            "pending vault candidate does not match its recorded transaction",
        ));
    }
    Ok(())
}

pub(super) fn pending_credential_error(kind: TransactionKind) -> anyhow::Error {
    let credential = match kind {
        TransactionKind::PassphraseChange => "the new passphrase",
        TransactionKind::Restore => "the backup's passphrase",
        _ => "the passphrase it started with",
    };
    classified(
        VaultErrorKind::Authentication,
        format!(
            "a vault {} is pending for this home; unlock with {credential} to finish it",
            kind.label()
        ),
    )
}

/// Keeps a classified failure's own kind and message; anything else that
/// prevents establishing the target's transaction state fails closed.
pub(crate) fn fail_closed(error: anyhow::Error) -> anyhow::Error {
    if crate::error::classified_kind(&error).is_some() {
        return error;
    }
    classify_source(
        VaultErrorKind::AuditTampered,
        "the vault transaction state for this home could not be established; refusing to guess its outcome",
        error,
    )
}

/// Failures after the pending marker is durable never roll back.
pub(super) fn pending_error(kind: TransactionKind, error: anyhow::Error) -> anyhow::Error {
    let credential = if kind == TransactionKind::PassphraseChange {
        " with the new passphrase"
    } else {
        ""
    };
    classify_source(
        VaultErrorKind::Io,
        format!(
            "the vault {} was recorded but did not finish; run an authenticated vault command again{credential} to finish it",
            kind.label()
        ),
        error,
    )
}

fn record_error(error: anyhow::Error) -> anyhow::Error {
    if crate::error::classified_kind(&error).is_some() {
        return error;
    }
    classify_source(
        VaultErrorKind::Io,
        "failed to record the vault transaction before changing the vault",
        error,
    )
}
