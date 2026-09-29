//! Transactional state backups and recovery.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use ulid::Ulid;

use crate::command::StateRestoreRequest;
use crate::context::RepoContext;

use super::MAINTENANCE_WRITER_COORDINATION_NOTE;
use super::compression::{
    GzipWriteReport, create_dir_all_synced, decompress_gzip_to_temp, gzip_file_atomic, sha256_file,
    sync_directory,
};
use super::jsonl::with_jsonl_write_lock;
use super::support::now_ms;

const BACKUP_MANIFEST_VERSION: u32 = 1;
const RUNS_STREAM: &str = "runs";
const RUNS_SOURCE_PATH: &str = ".agent/state/runs.jsonl";
const RUNS_BACKUP_FILE: &str = "runs.jsonl.gz";
const BACKUP_MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Copy)]
struct BackupStream {
    name: &'static str,
    state_file: &'static str,
    source_path: &'static str,
    compressed_file: &'static str,
}

const RUN_BACKUP_STREAM: BackupStream = BackupStream {
    name: RUNS_STREAM,
    state_file: "runs.jsonl",
    source_path: RUNS_SOURCE_PATH,
    compressed_file: RUNS_BACKUP_FILE,
};

#[derive(Debug, Deserialize, Serialize)]
struct StateBackupManifest {
    version: u32,
    stream: String,
    source_path: String,
    compressed_file: String,
    created_at_ms: u64,
    original_bytes: u64,
    original_sha256: String,
    compressed_bytes: u64,
}

fn read_state_backup_manifest(path: &Path) -> Result<StateBackupManifest> {
    let manifest_text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&manifest_text)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

pub(crate) fn restore_backup(ctx: &RepoContext, request: StateRestoreRequest) -> Result<Value> {
    let manifest_path = resolve_manifest_path(&request.backup);
    let manifest = read_state_backup_manifest(&manifest_path)?;
    let stream = validate_manifest(&manifest)?;
    let backup_dir = manifest_path
        .parent()
        .context("Backup manifest must have a parent directory")?;
    let compressed_path = backup_dir.join(&manifest.compressed_file);
    let compressed_bytes = fs::metadata(&compressed_path)
        .with_context(|| format!("Failed to inspect {}", compressed_path.display()))?
        .len();
    if compressed_bytes != manifest.compressed_bytes {
        bail!(
            "Backup compressed-size mismatch for {}; refusing to restore",
            compressed_path.display()
        );
    }
    let state_path = ctx.state_file(stream.state_file);
    let parent = state_path
        .parent()
        .context("State stream must have a parent directory")?;
    create_dir_all_synced(parent)?;

    let (mut restored, report) =
        decompress_gzip_to_temp(&compressed_path, parent, Some(manifest.original_bytes))?;
    if report.uncompressed_bytes != manifest.original_bytes
        || report.uncompressed_sha256 != manifest.original_sha256
    {
        bail!(
            "Backup checksum mismatch for {}; refusing to restore",
            compressed_path.display()
        );
    }
    validate_restored_stream(stream, restored.path()).with_context(|| {
        format!(
            "Backup {} is not valid {} state",
            compressed_path.display(),
            stream.name
        )
    })?;
    if let Ok(metadata) = fs::metadata(&state_path) {
        fs::set_permissions(restored.path(), metadata.permissions())
            .context("Failed to preserve state stream permissions")?;
    }
    restored
        .as_file_mut()
        .sync_all()
        .context("Failed to sync restored state")?;

    let mut recovery_hint = None;
    let result = with_jsonl_write_lock(&state_path, |guard| {
        let before = sha256_file_or_empty(&state_path)?;
        if before.uncompressed_bytes == report.uncompressed_bytes
            && before.uncompressed_sha256 == report.uncompressed_sha256
        {
            return Ok(json!({
                "ok": true,
                "command": "state restore",
                "stream": stream.name,
                "backup_path": backup_dir.display().to_string(),
                "source_path": stream.source_path,
                "changed": false,
                "bytes_restored": report.uncompressed_bytes,
                "sha256_restored": report.uncompressed_sha256,
                "replaced_bytes": before.uncompressed_bytes,
                "replaced_sha256": before.uncompressed_sha256,
                "recovery_backup_path": null,
                "writer_coordination_note": MAINTENANCE_WRITER_COORDINATION_NOTE,
            }));
        }
        if stream.name == RUNS_STREAM {
            super::runs::ensure_run_stream_replaceable(ctx, &state_path, guard)?;
        }
        let recovery_dir = if state_path.exists() {
            Some(
                create_state_backup(
                    ctx,
                    &state_path,
                    &format!("{}-restore-recovery", stream.name),
                    stream,
                    Some((before.uncompressed_bytes, &before.uncompressed_sha256)),
                )?
                .0,
            )
        } else {
            None
        };
        recovery_hint = recovery_dir.clone();
        restored
            .persist(&state_path)
            .map_err(|error| error.error)
            .with_context(|| {
                let recovery = recovery_dir.as_ref().map_or_else(
                    || "no prior state existed".into(),
                    |path| format!("current state is backed up at {}", path.display()),
                );
                format!("Failed to restore {}; {recovery}", state_path.display())
            })?;
        sync_directory(parent).with_context(|| {
            let recovery = recovery_dir.as_ref().map_or_else(
                || "no prior state existed".into(),
                |path| format!("replaced state is backed up at {}", path.display()),
            );
            format!(
                "Restored {} but failed to sync its directory; {recovery}",
                state_path.display()
            )
        })?;
        Ok(json!({
            "ok": true,
            "command": "state restore",
            "stream": stream.name,
            "backup_path": backup_dir.display().to_string(),
            "source_path": stream.source_path,
            "changed": true,
            "bytes_restored": report.uncompressed_bytes,
            "sha256_restored": report.uncompressed_sha256,
            "replaced_bytes": before.uncompressed_bytes,
            "replaced_sha256": before.uncompressed_sha256,
            "recovery_backup_path": recovery_dir.map(|path| path.display().to_string()),
            "writer_coordination_note": MAINTENANCE_WRITER_COORDINATION_NOTE,
        }))
    });
    result.map_err(|error| {
        let recovery = recovery_hint.as_ref().map_or_else(
            || "no replaced-state recovery backup was needed or completed".into(),
            |path| format!("replaced-state recovery backup: {}", path.display()),
        );
        anyhow!(
            "{error:#}\nState restore recovery context: source backup: {}; {recovery}",
            backup_dir.display(),
        )
    })
}

pub(super) fn create_runs_backup(
    ctx: &RepoContext,
    source: &Path,
    directory_prefix: &str,
    expected: Option<(u64, &str)>,
) -> Result<(PathBuf, GzipWriteReport)> {
    create_state_backup(ctx, source, directory_prefix, RUN_BACKUP_STREAM, expected)
}

fn create_state_backup(
    ctx: &RepoContext,
    source: &Path,
    directory_prefix: &str,
    stream: BackupStream,
    expected: Option<(u64, &str)>,
) -> Result<(PathBuf, GzipWriteReport)> {
    let backup_dir = ctx
        .root()
        .join(".agent/.cache/state-backups")
        .join(format!("{directory_prefix}-{}", Ulid::new()));
    create_dir_all_synced(&backup_dir).with_context(|| {
        format!(
            "Failed to create recovery backup directory {}",
            backup_dir.display()
        )
    })?;
    let compressed_path = backup_dir.join(stream.compressed_file);
    let backup = gzip_file_atomic(source, &compressed_path).with_context(|| {
        format!(
            "Failed to create recovery backup; incomplete artifacts may remain at {}",
            backup_dir.display()
        )
    })?;
    if let Some((expected_bytes, expected_sha256)) = expected
        && (backup.uncompressed_bytes != expected_bytes
            || backup.uncompressed_sha256 != expected_sha256)
    {
        bail!(
            "{} state changed while its recovery backup was being written; backup retained at {}",
            stream.name,
            backup_dir.display()
        );
    }
    let manifest = StateBackupManifest {
        version: BACKUP_MANIFEST_VERSION,
        stream: stream.name.into(),
        source_path: stream.source_path.into(),
        compressed_file: stream.compressed_file.into(),
        created_at_ms: now_ms(),
        original_bytes: backup.uncompressed_bytes,
        original_sha256: backup.uncompressed_sha256.clone(),
        compressed_bytes: backup.compressed_bytes,
    };
    write_manifest_atomic(&backup_dir, &manifest).with_context(|| {
        format!(
            "Recovery data was written but its manifest failed; incomplete backup retained at {}",
            backup_dir.display()
        )
    })?;
    Ok((backup_dir, backup))
}

fn write_manifest_atomic(directory: &Path, manifest: &StateBackupManifest) -> Result<()> {
    let mut temp = NamedTempFile::new_in(directory).with_context(|| {
        format!(
            "Failed to create backup manifest in {}",
            directory.display()
        )
    })?;
    serde_json::to_writer_pretty(&mut temp, manifest)?;
    temp.write_all(b"\n")?;
    temp.as_file_mut()
        .sync_all()
        .context("Failed to sync state backup manifest")?;
    temp.persist(directory.join(BACKUP_MANIFEST_FILE))
        .map_err(|error| error.error)
        .context("Failed to publish state backup manifest")?;
    sync_directory(directory)
}

fn resolve_manifest_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(BACKUP_MANIFEST_FILE)
    } else {
        path.to_path_buf()
    }
}

fn validate_manifest(manifest: &StateBackupManifest) -> Result<BackupStream> {
    if manifest.version != BACKUP_MANIFEST_VERSION {
        bail!(
            "Unsupported state backup manifest version {}",
            manifest.version
        );
    }
    let stream = match (manifest.stream.as_str(), manifest.source_path.as_str()) {
        (RUNS_STREAM, RUNS_SOURCE_PATH) => RUN_BACKUP_STREAM,
        _ => {
            bail!(
                "Backup is for unsupported stream {} at {}",
                manifest.stream,
                manifest.source_path
            );
        }
    };
    let compressed = Path::new(&manifest.compressed_file);
    if compressed.components().count() != 1
        || !matches!(compressed.components().next(), Some(Component::Normal(_)))
    {
        bail!("Backup manifest contains an unsafe compressed_file path");
    }
    if manifest.compressed_file != stream.compressed_file {
        bail!(
            "Backup manifest names unexpected compressed file {} for {}",
            manifest.compressed_file,
            stream.name
        );
    }
    Ok(stream)
}

fn validate_restored_stream(stream: BackupStream, path: &Path) -> Result<()> {
    debug_assert_eq!(stream.name, RUNS_STREAM);
    super::runs::validate_run_stream(path)
}

fn sha256_file_or_empty(path: &Path) -> Result<super::compression::GzipReadReport> {
    if path.exists() {
        sha256_file(path)
    } else {
        Ok(super::compression::GzipReadReport {
            uncompressed_bytes: 0,
            uncompressed_sha256: {
                use sha2::{Digest, Sha256};
                let bytes = Sha256::digest(b"");
                bytes.iter().map(|byte| format!("{byte:02x}")).collect()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use crate::test_env::TestRepoBuilder;

    use super::*;

    #[test]
    fn session_backups_are_no_longer_restorable() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let backup = ctx.root().join(".agent/state/backups/sessions-example");
        fs::create_dir_all(&backup).unwrap();
        fs::write(
            backup.join(BACKUP_MANIFEST_FILE),
            json!({
                "version": BACKUP_MANIFEST_VERSION,
                "stream": "sessions",
                "source_path": ".agent/state/sessions.jsonl",
                "compressed_file": "sessions.jsonl.gz",
                "created_at_ms": 1,
                "original_bytes": 0,
                "original_sha256": "",
                "compressed_bytes": 0,
            })
            .to_string(),
        )
        .unwrap();

        let error = restore_backup(&ctx, StateRestoreRequest { backup }).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Backup is for unsupported stream sessions at .agent/state/sessions.jsonl"
        );
    }
}
