//! Witnessed absent-target restore.
//!
//! A format 3 archive, or a format 2 archive whose vault ID is already
//! witnessed as format 3, is restored as an explicit recovery transaction:
//! the archive is authenticated in restore-only staging, resealed under a
//! fresh vault key at a generation above both the archive and the witness,
//! journaled against the final target, marked pending, installed without
//! replacement, and promoted. Every other copy of the vault ID is older and
//! is refused afterwards. An unwitnessed format 2 archive keeps the legacy
//! restore.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result as AnyResult, bail};
use secrecy::SecretString;

use crate::error::{classified, classified_recovery};
use crate::format::V2_FORMAT_VERSION;
use crate::store::witness::{
    self, JOURNAL_SCHEMA, Journal, JournalPayload, JournalTarget, PendingMarker, RestorePayload,
    TargetJournal, TargetLock, TransactionKind, WitnessLocation, WitnessRecord, WitnessStore,
    sha256_hex,
};
use crate::store::{AUDIT_TEXT_READ_LIMIT, FaultPoint, VaultStore};
use crate::vault::{
    RestoreSource, authenticate_restore_candidate_text, fail_closed, pending_publication_error,
    restore_pending_error,
};
use crate::{VaultErrorKind, VaultRecovery};

use super::super::payload::DecodedBackupArchive;
use super::super::{BackupRestoreResult, RestoreTarget};
use super::staging::{FreshStaging, StagingRef, require_recorded_private_home};
use super::{
    AUDIT_FILE, VAULT_FILE, restore_legacy, revalidate_target, sync_directory,
    validate_trusted_ancestors, vault_error_as_classified,
};

/// Whether a transaction is recorded for `home` as a final target. Never
/// creates or locks anything; it only allows a possible retry to reach
/// credential capture and never waives any later check.
pub(super) fn target_has_transaction(home: &Path) -> AnyResult<bool> {
    let location = WitnessLocation::for_home(home)?;
    let Some(witness) = location.open_existing()? else {
        return Ok(false);
    };
    witness.target_pending(&witness::target_key(home))
}

pub(in crate::backup) fn restore(
    passphrase: &SecretString,
    decoded: DecodedBackupArchive,
    target: RestoreTarget,
) -> AnyResult<BackupRestoreResult> {
    let location = WitnessLocation::for_home(&target.home)?;
    location.ensure_disjoint(&target.home)?;
    let witness = location.open_or_create()?;
    let target_key = witness::target_key(&target.home);
    let target_lock = witness.lock_target(&target_key)?;
    if let Some(result) =
        resume_matching_retry(&witness, &target_lock, &target, passphrase, &decoded)?
    {
        return Ok(result);
    }
    revalidate_target(&target)?;
    // Each path repeats this check under the ID lock; legacy restore also
    // rechecks after finalization and retains that lock through installation.
    let witnessed = witness.read_record(&decoded.source_vault_id)?.is_some();
    if decoded.source_format_version == V2_FORMAT_VERSION && !witnessed {
        return restore_legacy(passphrase, decoded, target, &witness);
    }
    restore_transactional(passphrase, &decoded, &target, &witness, &target_key)
}

fn restore_transactional(
    passphrase: &SecretString,
    decoded: &DecodedBackupArchive,
    target: &RestoreTarget,
    witness: &WitnessStore,
    target_key: &str,
) -> AnyResult<BackupRestoreResult> {
    let staging_leaf = format!(
        ".jig-vault-restore-{}-{}.tmp",
        &target_key[..16],
        ulid::Ulid::new()
    );
    // This operation owns the staging, and cleans it up on any failure,
    // only until it hands ownership off just before publishing the journal.
    let staging = FreshStaging::create_named(target, &staging_leaf)?;
    let staged = (|| -> AnyResult<VaultStore> {
        staging.write_file(VAULT_FILE, decoded.vault_bytes())?;
        staging.write_file(AUDIT_FILE, decoded.audit_bytes())?;
        staging.sync()?;
        VaultStore::open_existing(staging.path().to_path_buf()).map_err(vault_error_as_classified)
    })();
    let staged = match staged {
        Ok(staged) => staged,
        Err(error) => return Err(staging.abandon(error)),
    };
    let staged_ref = &staged;
    let source = RestoreSource {
        vault_id: &decoded.source_vault_id,
        format_version: decoded.source_format_version,
        backup_created_at_ms: decoded.backup_created_at_ms,
    };
    // The journal and pending marker are written while the staging and ID
    // locks taken for the plan are still held.
    staged.prepare_restore_candidate(passphrase, &source, witness, move |plan| {
        let prepared = (|| -> AnyResult<Journal> {
            staging.sync()?;
            let audit = staged_ref
                .read_audit_bytes_bounded(AUDIT_TEXT_READ_LIMIT as usize)?
                .context("restored audit log disappeared from staging")?;
            let journal = Journal {
                schema: JOURNAL_SCHEMA,
                operation: TransactionKind::Restore,
                vault_id: decoded.source_vault_id.clone(),
                target: JournalTarget {
                    target_key: target_key.to_owned(),
                    parent_device: target.parent_device,
                    parent_inode: target.parent_inode,
                },
                previous: plan
                    .record
                    .as_ref()
                    .and_then(|record| record.committed.clone()),
                previous_envelope_sha256: None,
                next: plan.next.clone(),
                payload: JournalPayload::Restore(RestorePayload {
                    archive_sha256: decoded.archive_sha256.clone(),
                    staging_leaf: staging_leaf.clone(),
                    staging_device: staging.device(),
                    staging_inode: staging.inode(),
                    audit_sha256: sha256_hex(&audit),
                }),
            };
            // The target's parent holds the staging and receives the
            // install; an interrupted earlier restore may have created it
            // unsynced.
            crate::store::ensure_entry_chain_durable(&target.parent)?;
            Ok(journal)
        })();
        let journal = match prepared {
            Ok(journal) => journal,
            Err(error) => return Err(staging.abandon(error)),
        };
        // From here the staging is recovery data: every failure, including a
        // published journal whose sync failed, preserves it.
        staging.hand_off();
        let journal_sha256 = witness.write_journal(&journal)?;
        crate::store::fault(FaultPoint::AfterJournal)?;
        let mut record = plan
            .record
            .unwrap_or_else(|| WitnessRecord::new(&decoded.source_vault_id));
        record.pending = Some(PendingMarker {
            operation: TransactionKind::Restore,
            target_key: target_key.to_owned(),
            journal_sha256,
            next: journal.next.clone(),
        });
        witness
            .write_record(&record)
            .map_err(|error| pending_publication_error(TransactionKind::Restore, error))?;
        crate::store::fault(FaultPoint::AfterPending)
            .and_then(|()| finish_pending_restore(witness, &journal, record, &target.home))
            .map_err(restore_pending_error)?;
        Ok(result_for(
            &journal,
            decoded.source_format_version,
            &target.home,
        ))
    })
}

/// Resumes a pending restore when this command retries the exact archive
/// it started from. A journal established, under this acquisition of the
/// target lock, to have no authoritative pending marker is unlinked; any
/// staging it names is left alone.
fn resume_matching_retry(
    witness: &WitnessStore,
    target_lock: &TargetLock,
    target: &RestoreTarget,
    passphrase: &SecretString,
    decoded: &DecodedBackupArchive,
) -> AnyResult<Option<BackupRestoreResult>> {
    let (vault_id, record, journal) = match witness
        .classify_target_journal(target_lock)
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
    if witness.read_record(&vault_id)?.as_ref() != Some(&record) {
        return Err(classified(
            VaultErrorKind::AuditTampered,
            "the pending vault transaction changed while it was being recovered",
        ));
    }
    let JournalPayload::Restore(payload) = &journal.payload else {
        return Err(classified_recovery(
            VaultErrorKind::AlreadyExists,
            VaultRecovery::StorageConflict,
            "a different vault transaction is pending for this restore target; finish it with an authenticated vault command first",
        ));
    };
    if payload.archive_sha256 != decoded.archive_sha256 || vault_id != decoded.source_vault_id {
        return Err(classified_recovery(
            VaultErrorKind::AlreadyExists,
            VaultRecovery::StorageConflict,
            "a restore of a different archive is pending for this target; retry with the same archive or finish it with an authenticated vault command",
        ));
    }
    let candidate = read_candidate(&journal, payload, &target.home)?;
    authenticate_restore_candidate_text(&journal, &candidate, &[passphrase])?;
    finish_pending_restore(witness, &journal, record, &target.home)?;
    Ok(Some(result_for(
        &journal,
        decoded.source_format_version,
        &target.home,
    )))
}

fn result_for(journal: &Journal, source_format_version: u32, home: &Path) -> BackupRestoreResult {
    BackupRestoreResult {
        root: home.to_path_buf(),
        vault_id: journal.vault_id.clone(),
        format_version: crate::format::V3_FORMAT_VERSION,
        source_format_version,
        generation: Some(journal.next.generation),
    }
}

/// The candidate envelope text of a pending restore: its staging until
/// installation renames that staging away, then the installed target. An
/// unrelated occupant of the target is never read as the candidate.
pub(crate) fn read_candidate(
    journal: &Journal,
    payload: &RestorePayload,
    home: &Path,
) -> AnyResult<String> {
    let staging = staging_path(journal, payload, home)?;
    let path = if fs::symlink_metadata(&staging).is_ok() {
        staging.join(VAULT_FILE)
    } else {
        home.join(VAULT_FILE)
    };
    let bytes = read_bounded(&path)?;
    String::from_utf8(bytes).context("pending restore candidate is not valid UTF-8")
}

/// Installs a pending restore's staging at its final target, or recognizes
/// an already installed exact successor, then promotes the witness. A
/// different occupant of the target is never overwritten.
pub(crate) fn finish_pending_restore(
    witness: &WitnessStore,
    journal: &Journal,
    mut record: WitnessRecord,
    home: &Path,
) -> AnyResult<()> {
    let JournalPayload::Restore(payload) = &journal.payload else {
        bail!("vault transaction journal does not describe a restore");
    };
    let parent = home
        .parent()
        .context("restore target has no parent directory")?;
    match fs::symlink_metadata(home) {
        Ok(metadata) => {
            if !installed_successor(journal, payload, home, &metadata)? {
                return Err(classified_recovery(
                    VaultErrorKind::AlreadyExists,
                    VaultRecovery::StorageConflict,
                    "the restore target is occupied by different contents; the pending restore stays in its staging directory until the target is free",
                ));
            }
            // A crash may have preceded the installation's own entry sync.
            sync_directory(parent)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The marker binds this journal, so the staging it names is this
            // transaction's recovery data: installable, never deletable.
            let staging = StagingRef::adopt(
                staging_path(journal, payload, home)?,
                parent.to_path_buf(),
                payload.staging_device,
                payload.staging_inode,
            )?;
            if sha256_hex(&read_bounded(&staging.path().join(VAULT_FILE))?)
                != journal.next.envelope_sha256
                || sha256_hex(&read_bounded(&staging.path().join(AUDIT_FILE))?)
                    != payload.audit_sha256
            {
                return Err(classified(
                    VaultErrorKind::AuditTampered,
                    "pending restore staging no longer matches its recorded transaction",
                ));
            }
            staging.require_installable()?;
            validate_trusted_ancestors(parent)?;
            staging.install(home)?;
            sync_directory(parent).with_context(|| {
                format!(
                    "restored vault was installed at {}, but its parent directory could not be synced",
                    home.display()
                )
            })?;
            crate::store::fault(FaultPoint::AfterEnvelope)?;
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect restore target {}", home.display()));
        }
    }
    witness.promote(journal, &mut record)
}

fn staging_path(journal: &Journal, payload: &RestorePayload, home: &Path) -> AnyResult<PathBuf> {
    let parent = home
        .parent()
        .context("restore target has no parent directory")?;
    let (device, inode) = witness::directory_identity(parent)?;
    if device != journal.target.parent_device || inode != journal.target.parent_inode {
        return Err(classified(
            VaultErrorKind::AuditTampered,
            "the pending restore was recorded for a different directory at this path",
        ));
    }
    Ok(parent.join(&payload.staging_leaf))
}

/// Whether `home` is this restore's own installed successor. Installation
/// renames the recorded staging directory itself into place, so only that
/// directory identity, still private and holding the recorded bytes, counts.
fn installed_successor(
    journal: &Journal,
    payload: &RestorePayload,
    home: &Path,
    metadata: &fs::Metadata,
) -> AnyResult<bool> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.dev() != payload.staging_device
        || metadata.ino() != payload.staging_inode
    {
        return Ok(false);
    }
    require_recorded_private_home(home, payload.staging_device, payload.staging_inode)?;
    let vault = match read_bounded_optional(&home.join(VAULT_FILE))? {
        Some(bytes) => bytes,
        None => return Ok(false),
    };
    let audit = match read_bounded_optional(&home.join(AUDIT_FILE))? {
        Some(bytes) => bytes,
        None => return Ok(false),
    };
    Ok(sha256_hex(&vault) == journal.next.envelope_sha256
        && sha256_hex(&audit) == payload.audit_sha256)
}

fn read_bounded(path: &Path) -> AnyResult<Vec<u8>> {
    read_bounded_optional(path)?
        .with_context(|| format!("pending restore file is missing: {}", path.display()))
}

fn read_bounded_optional(path: &Path) -> AnyResult<Option<Vec<u8>>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to open {}", path.display()));
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > AUDIT_TEXT_READ_LIMIT {
        bail!(
            "pending restore file is not a bounded regular file: {}",
            path.display()
        );
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(AUDIT_TEXT_READ_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}
