//! Transactional state compaction backups and recovery.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::NamedTempFile;
use ulid::Ulid;

use crate::command::{StateCompactSessionsRequest, StateRestoreRequest};
use crate::context::RepoContext;

use super::MAINTENANCE_WRITER_COORDINATION_NOTE;
use super::compression::{
    GzipWriteReport, create_dir_all_synced, decompress_gzip_to_temp, gzip_file_atomic, sha256_file,
    sync_directory,
};
use super::jsonl::with_jsonl_write_lock;
use super::receipts::validate_receipt_stream;
use super::session_compaction::{
    SessionCompactionAnalysis, analyze_session_compaction, write_compacted_sessions,
};
use super::support::now_ms;

const BACKUP_MANIFEST_VERSION: u32 = 1;
const SESSIONS_STREAM: &str = "sessions";
const SESSIONS_SOURCE_PATH: &str = ".agent/state/sessions.jsonl";
const SESSIONS_BACKUP_FILE: &str = "sessions.jsonl.gz";
const RECEIPTS_STREAM: &str = "receipts";
const RECEIPTS_SOURCE_PATH: &str = ".agent/state/receipts.jsonl";
const RECEIPTS_BACKUP_FILE: &str = "receipts.jsonl.gz";
const BACKUP_MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Copy)]
struct BackupStream {
    name: &'static str,
    state_file: &'static str,
    source_path: &'static str,
    compressed_file: &'static str,
}

const SESSION_BACKUP_STREAM: BackupStream = BackupStream {
    name: SESSIONS_STREAM,
    state_file: "sessions.jsonl",
    source_path: SESSIONS_SOURCE_PATH,
    compressed_file: SESSIONS_BACKUP_FILE,
};

const RECEIPT_BACKUP_STREAM: BackupStream = BackupStream {
    name: RECEIPTS_STREAM,
    state_file: "receipts.jsonl",
    source_path: RECEIPTS_SOURCE_PATH,
    compressed_file: RECEIPTS_BACKUP_FILE,
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

pub(crate) fn compact_sessions(
    ctx: &RepoContext,
    request: StateCompactSessionsRequest,
) -> Result<Value> {
    let sessions_path = ctx.state_file("sessions.jsonl");
    if !sessions_path.exists() {
        return Ok(compaction_value(
            &SessionCompactionAnalysis::empty(),
            request.dry_run,
            None,
        ));
    }
    if request.dry_run {
        let analysis = analyze_session_compaction(&sessions_path)?;
        return Ok(compaction_value(&analysis, true, None));
    }

    let mut backup_hint = None;
    let result = with_jsonl_write_lock(&sessions_path, |_guard| {
        let analysis = analyze_session_compaction(&sessions_path)?;
        if !analysis.needs_rewrite() {
            return Ok(compaction_value(&analysis, false, None));
        }

        let (backup_dir, _) = create_state_backup(
            ctx,
            &sessions_path,
            "sessions",
            SESSION_BACKUP_STREAM,
            Some((analysis.source_bytes, &analysis.source_sha256)),
        )?;
        backup_hint = Some(backup_dir.clone());

        let parent = sessions_path
            .parent()
            .context("sessions.jsonl must have a parent directory")?;
        let source_permissions = fs::metadata(&sessions_path)
            .with_context(|| format!("Failed to inspect {}", sessions_path.display()))?
            .permissions();
        let mut compacted = NamedTempFile::new_in(parent)
            .with_context(|| format!("Failed to create compacted state in {}", parent.display()))?;
        write_compacted_sessions(&sessions_path, &analysis, &mut compacted)?;
        fs::set_permissions(compacted.path(), source_permissions)
            .context("Failed to preserve session state permissions")?;
        compacted
            .as_file_mut()
            .sync_all()
            .context("Failed to sync compacted session state and permissions")?;

        let compacted_analysis = analyze_session_compaction(compacted.path())?;
        validate_compacted_analysis(&analysis, &compacted_analysis)?;
        compacted
            .persist(&sessions_path)
            .map_err(|error| error.error)
            .with_context(|| format!("Failed to publish {}", sessions_path.display()))?;
        sync_directory(parent).with_context(|| {
            format!(
                "Compacted state was published but its directory sync failed; exact recovery backup: {}",
                backup_dir.display()
            )
        })?;

        Ok(compaction_value(
            &analysis,
            false,
            Some(backup_dir.display().to_string()),
        ))
    });
    result.map_err(|error| {
        let recovery = backup_hint.as_ref().map_or_else(
            || "Session compaction failed before creating a recovery backup".into(),
            |path| format!("Session compaction recovery backup: {}", path.display()),
        );
        anyhow!("{error:#}\n{recovery}")
    })
}

pub(crate) fn restore_backup(ctx: &RepoContext, request: StateRestoreRequest) -> Result<Value> {
    let manifest_path = resolve_manifest_path(&request.backup);
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: StateBackupManifest = serde_json::from_str(&manifest_text)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
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
    let result = with_jsonl_write_lock(&state_path, |_guard| {
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

pub(super) fn create_receipts_backup(
    ctx: &RepoContext,
    source: &Path,
    directory_prefix: &str,
    expected: Option<(u64, &str)>,
) -> Result<(PathBuf, GzipWriteReport)> {
    create_state_backup(
        ctx,
        source,
        directory_prefix,
        RECEIPT_BACKUP_STREAM,
        expected,
    )
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
    if let Some((expected_bytes, expected_sha256)) = expected {
        if backup.uncompressed_bytes != expected_bytes
            || backup.uncompressed_sha256 != expected_sha256
        {
            bail!(
                "{} state changed while its recovery backup was being written; backup retained at {}",
                stream.name,
                backup_dir.display()
            );
        }
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

fn compaction_value(
    analysis: &SessionCompactionAnalysis,
    dry_run: bool,
    backup_path: Option<String>,
) -> Value {
    json!({
        "ok": true,
        "command": "state compact sessions",
        "dry_run": dry_run,
        "source_path": SESSIONS_SOURCE_PATH,
        "physical_records": analysis.physical_records,
        "logical_records": analysis.logical_records,
        "duplicate_records": analysis.duplicate_records,
        "records_changed": analysis.records_changed,
        "recursive_references": analysis.recursive_references,
        "bytes_before": analysis.source_bytes,
        "bytes_after": analysis.compacted_bytes,
        "bytes_reclaimable": analysis.bytes_reclaimable(),
        "source_sha256": analysis.source_sha256,
        "backup_path": backup_path,
        "git_history_rewritten": false,
        "history_note": "Working-tree compaction does not remove reachable Git blobs.",
        "writer_coordination_note": MAINTENANCE_WRITER_COORDINATION_NOTE,
    })
}

fn validate_compacted_analysis(
    original: &SessionCompactionAnalysis,
    compacted: &SessionCompactionAnalysis,
) -> Result<()> {
    if compacted.physical_records != original.logical_records
        || compacted.logical_records != original.logical_records
        || compacted.duplicate_records != 0
        || compacted.records_changed != 0
        || compacted.recursive_references != 0
        || compacted.source_bytes != original.compacted_bytes
        || !original.same_logical_state(compacted)
    {
        bail!("Compacted session state failed validation; the original file was not replaced");
    }
    Ok(())
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
        (SESSIONS_STREAM, SESSIONS_SOURCE_PATH) => SESSION_BACKUP_STREAM,
        (RECEIPTS_STREAM, RECEIPTS_SOURCE_PATH) => RECEIPT_BACKUP_STREAM,
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
    match stream.name {
        SESSIONS_STREAM => analyze_session_compaction(path).map(|_| ()),
        RECEIPTS_STREAM => validate_receipt_stream(path),
        _ => unreachable!("validated backup streams are exhaustive"),
    }
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

    fn write_recursive_sessions(ctx: &RepoContext) -> Vec<u8> {
        fs::create_dir_all(ctx.state_dir()).unwrap();
        let first = json!({
            "id": "event-1",
            "session_id": "session-1",
            "event": "start",
            "timestamp_ms": 1,
            "outcome": null,
            "summary": {
                "default_branch": "main",
                "open_plans": [],
                "recent_decisions": [],
                "recent_receipts": [],
                "recent_sessions": [],
                "repo_name": "fixture",
                "source_commit": "abc",
                "source_path": "fixture"
            }
        });
        let second = json!({
            "id": "event-2",
            "session_id": "session-2",
            "event": "start",
            "timestamp_ms": 2,
            "outcome": null,
            "summary": {
                "default_branch": "main",
                "open_plans": [],
                "recent_decisions": [],
                "recent_receipts": [],
                "recent_sessions": [first],
                "repo_name": "fixture",
                "source_commit": "abc",
                "source_path": "fixture"
            }
        });
        let bytes = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        )
        .into_bytes();
        fs::write(ctx.state_file("sessions.jsonl"), &bytes).unwrap();
        bytes
    }

    #[test]
    fn compact_backup_and_restore_round_trip_exactly() {
        let temp = tempdir().unwrap();
        TestRepoBuilder::new(temp.path()).write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let original = write_recursive_sessions(&ctx);

        let dry_run =
            compact_sessions(&ctx, StateCompactSessionsRequest { dry_run: true }).unwrap();
        assert_eq!(
            fs::read(ctx.state_file("sessions.jsonl")).unwrap(),
            original
        );
        assert!(dry_run["backup_path"].is_null());

        let compacted =
            compact_sessions(&ctx, StateCompactSessionsRequest { dry_run: false }).unwrap();
        let backup = PathBuf::from(compacted["backup_path"].as_str().unwrap());
        let compacted_bytes = fs::read(ctx.state_file("sessions.jsonl")).unwrap();
        assert!(compacted_bytes.len() < original.len());
        let repeated =
            compact_sessions(&ctx, StateCompactSessionsRequest { dry_run: false }).unwrap();
        assert_eq!(repeated["records_changed"], 0);
        assert!(repeated["backup_path"].is_null());
        let appended = json!({
            "id": "event-3",
            "session_id": "session-3",
            "event": "end",
            "timestamp_ms": 3,
            "outcome": "done"
        });
        let mut current_with_append = compacted_bytes;
        current_with_append.extend_from_slice(
            format!("{}\n", serde_json::to_string(&appended).unwrap()).as_bytes(),
        );
        fs::write(ctx.state_file("sessions.jsonl"), &current_with_append).unwrap();

        let restored = restore_backup(&ctx, StateRestoreRequest { backup }).unwrap();
        assert_eq!(restored["changed"], true);
        assert_eq!(restored["bytes_restored"], original.len() as u64);
        assert_eq!(
            fs::read(ctx.state_file("sessions.jsonl")).unwrap(),
            original
        );
        let recovery = PathBuf::from(restored["recovery_backup_path"].as_str().unwrap());
        let recovered = restore_backup(&ctx, StateRestoreRequest { backup: recovery }).unwrap();
        assert_eq!(recovered["changed"], true);
        assert_eq!(
            fs::read(ctx.state_file("sessions.jsonl")).unwrap(),
            current_with_append
        );
    }
}
