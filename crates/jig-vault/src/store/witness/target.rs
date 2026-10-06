//! Whether a target's journal is still referenced by an authoritative
//! pending marker.
//!
//! The journal's own claims (its vault ID above all) are never trusted to
//! locate the marker that might reference it: a corrupted journal must not
//! be mistaken for an orphan while a durable marker still needs its
//! recovery data. Classification reads every witness record instead.
//!
//! It must run under the target's lock. Every writer that creates or clears
//! a marker naming a target, or writes that target's journal, holds the
//! same lock, and records are only replaced by atomic renames. No record
//! naming the target can therefore appear, change, or vanish between
//! classification and a deletion made under the same lock.

use std::fs;
use std::path::Path;

use anyhow::{Result as AnyResult, bail};

use crate::VaultErrorKind;
use crate::error::classified;

use super::{IDS_DIR, Journal, WitnessRecord, WitnessStore, id_key, record};

/// Records are scanned up to this many directory entries. Reaching the
/// bound never authorizes a deletion.
#[cfg(not(test))]
const MAX_RECORD_SCAN_ENTRIES: usize = 100_000;
#[cfg(test)]
pub(super) const MAX_RECORD_SCAN_ENTRIES: usize = 64;

/// What the witness says about one target's journal.
pub(crate) enum TargetJournal {
    /// No journal is recorded for the target, and no marker names it.
    Absent,
    /// No authoritative marker names the target.
    Orphan(OrphanJournal),
    /// Exactly one marker names the target and binds this exact journal.
    Referenced {
        vault_id: String,
        record: WitnessRecord,
        journal: Box<Journal>,
    },
}

/// Proof, established under the target lock, that no authoritative pending
/// marker references a journal: the only way to delete a journal (and its
/// restore staging) outside the promotion that finishes it.
pub(crate) struct OrphanJournal {
    journal: Journal,
}

impl OrphanJournal {
    pub(crate) fn journal(&self) -> &Journal {
        &self.journal
    }
}

impl WitnessStore {
    /// Classifies the journal recorded for `target_key` against every
    /// authoritative marker, whether or not the journal still exists. Fails
    /// closed when any record cannot be read and validated, when the scan
    /// reaches its bound, when a marker names the target but its journal is
    /// missing or does not match it, when several markers name it, or when a
    /// journal exists but the target lock is not held.
    pub(crate) fn classify_target_journal(&self, target_key: &str) -> AnyResult<TargetJournal> {
        let journal = self.read_journal(target_key)?;
        if journal.is_some() {
            // Only a journal's classification can authorize a deletion.
            self.require_target_lock(target_key)?;
        }
        let mut naming = self.records_with_pending_for(target_key)?;
        let Some((journal, digest)) = journal else {
            if naming.is_empty() {
                return Ok(TargetJournal::Absent);
            }
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "the pending vault transaction's journal is missing; refusing to guess its outcome",
            ));
        };
        if naming.len() > 1 {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "several pending vault transactions name this target; refusing to guess which one owns its journal",
            ));
        }
        let Some(record) = naming.pop() else {
            return Ok(TargetJournal::Orphan(OrphanJournal { journal }));
        };
        let pending = record
            .pending
            .as_ref()
            .expect("selected for its pending marker");
        if pending.journal_sha256 != digest
            || pending.next != journal.next
            || pending.operation != journal.operation
            || journal.vault_id != record.vault_id
        {
            return Err(classified(
                VaultErrorKind::AuditTampered,
                "the pending vault transaction's journal does not match its marker",
            ));
        }
        Ok(TargetJournal::Referenced {
            vault_id: record.vault_id.clone(),
            record,
            journal: Box::new(journal),
        })
    }

    /// Removes an established orphan's journal, still under the lock that
    /// established it.
    pub(crate) fn delete_orphan_journal(&self, orphan: OrphanJournal) -> AnyResult<()> {
        let target_key = orphan.journal.target.target_key;
        self.require_target_lock(&target_key)?;
        self.remove_journal(&target_key)
    }

    fn require_target_lock(&self, target_key: &str) -> AnyResult<()> {
        if !self.holds_target_lock(target_key) {
            bail!("the vault transaction journal can only be classified under its target lock");
        }
        Ok(())
    }

    /// Every record whose pending marker names `target_key`, made durable
    /// before returning. Only
    /// `<id key>.json` entries are records; interrupted atomic writes leave
    /// temporary names that were never published and are not read.
    fn records_with_pending_for(&self, target_key: &str) -> AnyResult<Vec<WitnessRecord>> {
        let mut naming = Vec::new();
        for (index, entry) in fs::read_dir(self.root.join(IDS_DIR))?.enumerate() {
            if index >= MAX_RECORD_SCAN_ENTRIES {
                bail!(
                    "the vault witness has too many records to establish that no pending transaction needs this journal"
                );
            }
            let entry = entry?;
            let Some(key) = record_key(&entry.file_name()) else {
                continue;
            };
            let record = self
                .read_record_file(&entry.path())?
                .ok_or_else(|| anyhow::anyhow!("a vault witness record vanished while scanning"))?;
            record.validate(&record.vault_id)?;
            if id_key(&record.vault_id) != key {
                bail!("a vault witness record is stored under another vault's key");
            }
            if record
                .pending
                .as_ref()
                .is_some_and(|pending| pending.target_key == target_key)
            {
                naming.push(record);
            }
        }
        // What was read is now durable, so no conclusion rests on a record
        // an interrupted write published without syncing.
        self.sync_records()?;
        Ok(naming)
    }
}

/// The ID key of a published record entry, if the name is one.
fn record_key(name: &std::ffi::OsStr) -> Option<String> {
    let key = Path::new(name).file_stem()?.to_str()?;
    let published = Path::new(name).extension().is_some_and(|ext| ext == "json")
        && record::is_hex_digest(key)
        && name.len() == key.len() + ".json".len();
    published.then(|| key.to_owned())
}
