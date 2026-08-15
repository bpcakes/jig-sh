use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result as AnyResult};
use secrecy::SecretString;
use time::OffsetDateTime;
use zeroize::Zeroizing;

use crate::audit::{AuditAction, AuditEvent};
use crate::crypto::KEY_LEN;
use crate::error::{classified, vault_error_from_anyhow};
use crate::format::FORMAT_VERSION;
use crate::store::VaultStore;
use crate::{PreparedPrivateFile, Result, VaultError, VaultErrorKind};

mod codec;
mod payload;
#[cfg(target_os = "linux")]
mod restore_linux;
#[cfg(test)]
mod tests;

use codec::seal_archive;
#[cfg(target_os = "linux")]
use codec::{ParsedBackupArchive, decrypt_archive, parse_archive_bytes};
pub(crate) use payload::{inspect_embedded_vault, max_backup_audit_bytes};

pub const BACKUP_FORMAT_VERSION: u32 = 1;
pub const MAX_BACKUP_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const BACKUP_START_EVENT_RESERVE_BYTES: usize = 4 * 1024;

/// A validated, non-creating request to create one encrypted vault backup.
///
/// The request is opaque and consumed by [`crate::Vault::create_backup`].
pub struct BackupCreateRequest {
    pub(crate) store: VaultStore,
    pub(crate) output: PathBuf,
    pub(crate) overwrite: bool,
}

impl fmt::Debug for BackupCreateRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupCreateRequest")
            .field("source_home", &self.store.root())
            .field("output", &self.output)
            .field("overwrite", &self.overwrite)
            .field("contents", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// Metadata-only outcome of a completed encrypted backup creation.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupCreateResult {
    pub bytes_written: usize,
    pub backup_version: u32,
    pub created_at_ms: i128,
}

/// A bounded encrypted archive and absent restore target validated without a
/// passphrase or filesystem mutation.
///
/// The request is opaque and consumed by [`crate::Vault::restore_backup`].
pub struct BackupRestoreRequest {
    #[cfg(target_os = "linux")]
    archive: ParsedBackupArchive,
    #[cfg(target_os = "linux")]
    target: RestoreTarget,
}

impl fmt::Debug for BackupRestoreRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("BackupRestoreRequest");
        #[cfg(target_os = "linux")]
        debug
            .field("archive", &"[REDACTED]")
            .field("archive_len", &self.archive.serialized_len)
            .field("target_home", &self.target.home);
        debug.finish_non_exhaustive()
    }
}

/// Metadata-only outcome of a completed vault restore.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupRestoreResult {
    pub root: PathBuf,
    pub vault_id: String,
    pub format_version: u32,
}

#[cfg(target_os = "linux")]
struct RestoreTarget {
    home: PathBuf,
    parent: PathBuf,
    #[cfg(unix)]
    parent_device: u64,
    #[cfg(unix)]
    parent_inode: u64,
}

#[cfg(target_os = "linux")]
impl fmt::Debug for RestoreTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RestoreTarget")
            .field("home", &self.home)
            .field("parent", &self.parent)
            .finish_non_exhaustive()
    }
}

pub(crate) struct BackupSnapshot {
    pub(crate) store: VaultStore,
    pub(crate) audit_key: Zeroizing<[u8; KEY_LEN]>,
    pub(crate) operation_id: String,
    pub(crate) source_vault_id: String,
    pub(crate) source_format_version: u32,
    pub(crate) vault_bytes: Zeroizing<Vec<u8>>,
    pub(crate) audit_bytes: Zeroizing<Vec<u8>>,
}

struct BackupLifecycle {
    store: VaultStore,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
    operation_id: String,
}

impl fmt::Debug for BackupSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupSnapshot")
            .field("operation_id", &self.operation_id)
            .field("source_vault_id", &self.source_vault_id)
            .field("source_format_version", &self.source_format_version)
            .field("vault_len", &self.vault_bytes.len())
            .field("audit_len", &self.audit_bytes.len())
            .field("audit_key", &"[REDACTED]")
            .field("contents", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl BackupLifecycle {
    fn record_finish(&self, bytes_written: usize, created_at_ms: i128) -> AnyResult<()> {
        AuditEvent::append(
            &self.store,
            self.audit_key.as_ref(),
            AuditAction::BackupFinish,
            serde_json::json!({
                "operation_id": self.operation_id,
                "bytes_written": bytes_written,
                "backup_version": BACKUP_FORMAT_VERSION,
                "created_at_ms": created_at_ms,
            }),
        )?;
        Ok(())
    }

    fn record_failure(&self, stage: &str) -> AnyResult<()> {
        AuditEvent::append(
            &self.store,
            self.audit_key.as_ref(),
            AuditAction::BackupFailed,
            serde_json::json!({
                "operation_id": self.operation_id,
                "stage": stage,
                "error": "vault backup failed",
            }),
        )?;
        Ok(())
    }

    fn failure_error(&self, stage: &str, kind: VaultErrorKind, error: anyhow::Error) -> VaultError {
        match self.record_failure(stage) {
            Ok(()) => VaultError::from_anyhow(kind, error),
            Err(audit_error) => VaultError::from_anyhow(
                kind,
                error.context(format!(
                    "vault backup failed; additionally failed to append terminal audit event: {audit_error}"
                )),
            ),
        }
    }

    fn finish_error(&self, error: anyhow::Error) -> VaultError {
        match self.record_failure("audit_finish") {
            Ok(()) => VaultError::from_anyhow(
                VaultErrorKind::AuditTampered,
                error.context(
                    "backup output was installed, but its finish audit event failed",
                ),
            ),
            Err(failure_error) => VaultError::from_anyhow(
                VaultErrorKind::AuditTampered,
                error.context(format!(
                    "backup output was installed, but both finish and failure audit events failed: {failure_error}"
                )),
            ),
        }
    }
}

impl fmt::Debug for BackupLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupLifecycle")
            .field("operation_id", &self.operation_id)
            .field("audit_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

// These direct-operation entry points stay in this module so neither the
// facade nor the CLI can obtain decrypted archive or vault bytes.
pub(crate) fn preflight_create(
    source_home: PathBuf,
    output: &Path,
    overwrite: bool,
) -> Result<BackupCreateRequest> {
    PreparedPrivateFile::preflight(output, overwrite)?;
    let store = VaultStore::open_existing(source_home)?;
    validate_create_request(&store, output, overwrite)
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
    Ok(BackupCreateRequest {
        store,
        output: output.to_path_buf(),
        overwrite,
    })
}

pub(crate) fn create(
    passphrase: &SecretString,
    request: BackupCreateRequest,
) -> Result<BackupCreateResult> {
    let BackupCreateRequest {
        store,
        output,
        overwrite,
    } = request;
    validate_create_request(&store, &output, overwrite)
        .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
    let snapshot = store.prepare_backup_snapshot(passphrase, ulid::Ulid::new().to_string())?;
    let BackupSnapshot {
        store,
        audit_key,
        operation_id,
        source_vault_id,
        source_format_version,
        vault_bytes,
        audit_bytes,
    } = snapshot;
    let lifecycle = BackupLifecycle {
        store,
        audit_key,
        operation_id,
    };
    let created_at_ms = now_ms();
    let sealed = seal_archive(
        passphrase,
        &source_vault_id,
        source_format_version,
        &vault_bytes,
        &audit_bytes,
        created_at_ms,
    )
    .map_err(|error| {
        let kind = crate::error::classified_kind(&error).unwrap_or(VaultErrorKind::Internal);
        lifecycle.failure_error("encrypt", kind, error)
    })?;
    let bytes_written = sealed.bytes.len();
    let prepared =
        PreparedPrivateFile::prepare(&output, sealed.bytes, overwrite).map_err(|error| {
            let kind = error.kind();
            lifecycle.failure_error("sink_prepare", kind, anyhow::Error::new(error))
        })?;
    lifecycle
        .store
        .validate_external_output(&output, "backup")
        .map_err(|error| {
            lifecycle.failure_error("sink_preflight", VaultErrorKind::InvalidInput, error)
        })?;
    prepared.install().map_err(|error| {
        let kind = error.kind();
        lifecycle.failure_error("sink_install", kind, anyhow::Error::new(error))
    })?;
    lifecycle
        .record_finish(bytes_written, sealed.created_at_ms)
        .map_err(|error| lifecycle.finish_error(error))?;
    Ok(BackupCreateResult {
        bytes_written,
        backup_version: BACKUP_FORMAT_VERSION,
        created_at_ms: sealed.created_at_ms,
    })
}

pub(crate) fn preflight_restore(
    input: &Path,
    target_home: PathBuf,
) -> Result<BackupRestoreRequest> {
    if input == Path::new("-") {
        return Err(VaultError::new(
            VaultErrorKind::InvalidInput,
            "backup restore requires a bounded regular input file; stdin is unsupported",
        ));
    }
    #[cfg(target_os = "linux")]
    {
        let archive_bytes = restore_linux::read_archive(input)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Io, error))?;
        let archive = parse_archive_bytes(archive_bytes)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Serialization, error))?;
        let target = restore_linux::preflight_target(target_home)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::InvalidInput, error))?;
        Ok(BackupRestoreRequest { archive, target })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (input, target_home);
        Err(unsupported_restore_platform())
    }
}

pub(crate) fn restore(
    passphrase: &SecretString,
    request: BackupRestoreRequest,
) -> Result<BackupRestoreResult> {
    #[cfg(target_os = "linux")]
    {
        let BackupRestoreRequest { archive, target } = request;
        let decoded = decrypt_archive(passphrase, archive)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Authentication, error))?;
        restore_linux::restore(passphrase, decoded, target)
            .map_err(|error| vault_error_from_anyhow(VaultErrorKind::Io, error))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (passphrase, request);
        Err(unsupported_restore_platform())
    }
}

#[cfg(not(target_os = "linux"))]
fn unsupported_restore_platform() -> VaultError {
    VaultError::new(
        VaultErrorKind::InvalidInput,
        "vault backup restore is unsupported on this platform because Jig cannot guarantee owner-only staging, reparse-point refusal, and atomic absent-target directory installation",
    )
}

fn validate_create_request(store: &VaultStore, output: &Path, overwrite: bool) -> AnyResult<()> {
    store.revalidate_existing()?;
    PreparedPrivateFile::preflight(output, overwrite).map_err(anyhow::Error::new)?;
    store.validate_external_output(output, "backup")?;
    let vault_bytes = store
        .read_vault_bytes()?
        .context("existing vault state disappeared during backup preflight")?;
    let (vault_id, version) = inspect_embedded_vault(&vault_bytes)?;
    if version != FORMAT_VERSION {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!(
                "vault format {version} cannot be backed up; run `jig vault migrate --to {FORMAT_VERSION}` first"
            ),
        ));
    }
    let max_audit = max_backup_audit_bytes(vault_id.len(), vault_bytes.len())?
        .saturating_sub(BACKUP_START_EVENT_RESERVE_BYTES);
    let audit_len = store
        .audit_len()?
        .context("existing vault audit log is missing")?;
    if audit_len > max_audit as u64 {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!(
                "vault audit log is {audit_len} bytes; this one-shot backup supports at most {max_audit} bytes before its start event"
            ),
        ));
    }
    Ok(())
}

fn now_ms() -> i128 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000
}
