//! Whether a target's journal is still referenced by an authoritative
//! pending marker.
//!
//! The journal's own claims (its vault ID above all) are never trusted to
//! locate the marker that might reference it: a corrupted journal must not
//! be mistaken for an orphan while a durable marker still needs its
//! recovery data. Classification reads every witness record instead.
//!
//! It runs under one acquisition of the target's lock, which the result
//! borrows. Every writer that creates or clears a marker naming a target,
//! or writes that target's journal, holds the same lock, and records are
//! only replaced by atomic renames. No record naming the target can
//! therefore appear, change, or vanish between classification and a
//! deletion made through the proof it returns.
//!
//! An orphan proof authorizes unlinking only the target's journal file. It
//! never authorizes deleting staging or any directory a journal names: that
//! no transaction needs a journal says nothing about who owns what it
//! describes.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result as AnyResult, bail};

use crate::VaultErrorKind;
use crate::error::classified;

use super::{HeldLock, IDS_DIR, Journal, WitnessRecord, WitnessStore, id_key, record};

/// One acquisition of a final target's lock, held while this value lives.
pub(crate) struct TargetLock {
    root: PathBuf,
    key: String,
    _held: HeldLock,
}

impl TargetLock {
    pub(super) fn new(root: PathBuf, key: &str, held: HeldLock) -> Self {
        Self {
            root,
            key: key.to_owned(),
            _held: held,
        }
    }
}

/// What the witness says about one target's journal.
pub(crate) enum TargetJournal<'lock> {
    /// No journal is recorded for the target, and no marker names it.
    Absent,
    /// No authoritative marker names the target.
    Orphan(OrphanJournal<'lock>),
    /// Exactly one marker names the target and binds this exact journal.
    Referenced {
        vault_id: String,
        record: Box<WitnessRecord>,
        journal: Box<Journal>,
    },
}

/// Proof, bound to the target-lock acquisition that established it, that
/// no authoritative pending marker references the target's journal. It
/// authorizes unlinking only that journal file, and is the only way to do
/// so outside the promotion that finishes a transaction.
pub(crate) struct OrphanJournal<'lock> {
    lock: &'lock TargetLock,
}

impl WitnessStore {
    /// Classifies the journal of the target `lock` holds against every
    /// authoritative marker, whether or not the journal still exists. Fails
    /// closed when enumeration or record validation fails, when a marker
    /// names the target but its journal is missing or does not match it, or
    /// when several markers name it.
    pub(crate) fn classify_target_journal<'lock>(
        &self,
        lock: &'lock TargetLock,
    ) -> AnyResult<TargetJournal<'lock>> {
        self.require_own_lock(lock)?;
        let target_key = lock.key.as_str();
        let journal = self.read_journal(target_key)?;
        let mut naming = self.records_with_pending_for(target_key)?;
        let Some((journal, digest)) = journal else {
            if naming.is_empty() {
                // An earlier removal may have unlinked the journal without
                // making that durable; the absence this reports must be.
                self.sync_journals()?;
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
            return Ok(TargetJournal::Orphan(OrphanJournal { lock }));
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
            record: Box::new(record),
            journal: Box::new(journal),
        })
    }

    /// Unlinks an established orphan's journal file, and nothing else,
    /// while the lock acquisition that established it is still held.
    pub(crate) fn delete_orphan_journal(&self, orphan: OrphanJournal<'_>) -> AnyResult<()> {
        self.require_own_lock(orphan.lock)?;
        self.remove_journal(&orphan.lock.key)
    }

    fn require_own_lock(&self, lock: &TargetLock) -> AnyResult<()> {
        if lock.root != self.root {
            bail!("a target lock of another vault witness cannot classify this witness's journals");
        }
        Ok(())
    }

    /// Whether a transaction is recorded for `target_key`: its journal
    /// exists, or an authoritative marker names it although the journal is
    /// missing. Read-only discovery that never creates, locks, or syncs; it
    /// only keeps absent targets absent and lets a possible retry reach
    /// credential capture.
    pub(crate) fn target_pending(&self, target_key: &str) -> AnyResult<bool> {
        if self.journal_exists(target_key) {
            return Ok(true);
        }
        self.target_has_pending_marker(target_key)
    }

    /// Status presentation only: an orphan journal cannot finish a change.
    /// Keep the journal-inclusive probe above for resolution and preflight.
    pub(crate) fn target_has_pending_marker(&self, target_key: &str) -> AnyResult<bool> {
        Ok(self
            .scan_pending_for(target_key)?
            .is_some_and(|naming| !naming.is_empty()))
    }

    /// Up to two records whose pending marker names `target_key`, made
    /// durable before returning. Two are enough to establish ambiguity.
    fn records_with_pending_for(&self, target_key: &str) -> AnyResult<Vec<WitnessRecord>> {
        let Some(naming) = self.scan_pending_for(target_key)? else {
            return Ok(Vec::new());
        };
        // What was read is now durable, so no conclusion rests on a record
        // an interrupted write published without syncing.
        self.sync_records()?;
        Ok(naming)
    }

    /// Up to two records whose pending marker names `target_key`, or `None`
    /// when the records directory does not exist yet: an interrupted first creation
    /// can leave the witness root without it, and with no records no marker
    /// names anything; the next witness open finishes the tree. Only
    /// `<id key>.json` entries are records; interrupted atomic writes leave
    /// temporary names that were never published and are not read. Stream
    /// the complete directory: retained history must not exhaust a lifetime
    /// entry quota. Individual reads remain size-bounded, and retaining at
    /// most two matching records bounds memory independently of history.
    fn scan_pending_for(&self, target_key: &str) -> AnyResult<Option<Vec<WitnessRecord>>> {
        let entries = match fs::read_dir(self.root.join(IDS_DIR)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut naming = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Some(key) = record_key(&entry.file_name()) else {
                continue;
            };
            let record = self.read_record_for_scan(&entry.path(), &key)?;
            if record
                .pending
                .as_ref()
                .is_some_and(|pending| pending.target_key == target_key)
                && naming.len() < 2
            {
                naming.push(record);
            }
        }
        Ok(Some(naming))
    }

    fn read_record_for_scan(&self, path: &Path, key: &str) -> AnyResult<WitnessRecord> {
        let read = || {
            let record = self
                .read_record_file(path)?
                .ok_or_else(|| anyhow::anyhow!("a vault witness record vanished while scanning"))?;
            record.validate(&record.vault_id)?;
            if id_key(&record.vault_id) != key {
                bail!("a vault witness record is stored under another vault's key");
            }
            Ok(record)
        };
        read().map_err(|error| record::read_error(path, error))
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
