//! Append-only JSONL persistence, locking, snapshots, and bounded receipt reads.
//!
//! Durable schemas live in `records`; this module owns only their storage mechanics.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;
use serde::{
    Serialize,
    de::{DeserializeOwned, IgnoredAny},
};
use tempfile::NamedTempFile;

use crate::cancellation::ensure_status_collection_active;

use super::records::ReceiptRecord;

const JSONL_READ_CHUNK: usize = 16 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) struct RawJsonlRecord<'a> {
    pub(super) line_number: u64,
    pub(super) bytes: &'a [u8],
    pub(super) terminated: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct JsonlScanStats {
    pub(super) file_bytes: u64,
    pub(super) physical_lines: u64,
    pub(super) records: u64,
    pub(super) blank_lines: u64,
    pub(super) max_line_bytes: u64,
    pub(super) max_line_number: Option<u64>,
    pub(super) unterminated_final_record: bool,
}

#[derive(Debug)]
pub(super) enum RawJsonlRewrite {
    Keep,
    // The migration rewriter contract includes replacement even when a build
    // happens to contain only retain/drop maintenance callers.
    #[allow(dead_code)]
    Replace(Vec<u8>),
    Drop,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct JsonlRewriteStats {
    pub(super) input: JsonlScanStats,
    pub(super) output: JsonlScanStats,
    pub(super) kept_records: u64,
    pub(super) replaced_records: u64,
    pub(super) dropped_records: u64,
}

pub(super) fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    with_jsonl_write_lock(path, |_guard| {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        Ok(())
    })
}

pub(super) fn append_text(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    with_jsonl_write_lock(path, |_guard| {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?;
        file.write_all(content)?;
        file.sync_data()?;
        Ok(())
    })
}

pub(super) struct JsonlWriteGuard {
    lock_file: File,
    legacy_lock_file: Option<File>,
}

#[cfg(test)]
pub(super) fn write_jsonl_locked<T: Serialize>(
    _guard: &JsonlWriteGuard,
    path: &Path,
    values: &[T],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temp file in {}", parent.display()))?;
    let source_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    for value in values {
        serde_json::to_writer(&mut temp, value)?;
        temp.write_all(b"\n")?;
    }
    temp.as_file_mut().flush()?;
    if let Some(permissions) = source_permissions {
        temp.as_file().set_permissions(permissions)?;
    }
    temp.as_file_mut().sync_all()?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    sync_parent_directory(parent)?;
    Ok(())
}

pub(super) fn rewrite_jsonl_raw_locked(
    _guard: &JsonlWriteGuard,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    mut transform: impl FnMut(RawJsonlRecord<'_>) -> Result<RawJsonlRewrite>,
    validate: impl FnOnce(&Path) -> Result<()>,
) -> Result<JsonlRewriteStats> {
    ensure_state_read_active(cancelled)?;
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(JsonlRewriteStats::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect {}", path.display()));
        }
    };
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let source = File::open(path)
        .with_context(|| format!("Failed to open {} for locked rewrite", path.display()))?;
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temp file in {}", parent.display()))?;
    let mut kept_records = 0u64;
    let mut replaced_records = 0u64;
    let mut dropped_records = 0u64;

    let input = scan_jsonl_reader_all(
        BufReader::with_capacity(JSONL_READ_CHUNK, source),
        path,
        cancelled,
        &mut |record, blank| {
            if blank {
                temp.as_file_mut().write_all(record.bytes)?;
                if record.terminated {
                    temp.as_file_mut().write_all(b"\n")?;
                }
                return Ok(());
            }

            serde_json::from_slice::<IgnoredAny>(record.bytes).with_context(|| {
                format!(
                    "Failed to parse source JSONL record {} in {}",
                    record.line_number,
                    path.display()
                )
            })?;
            match transform(record)? {
                RawJsonlRewrite::Keep => {
                    temp.as_file_mut().write_all(record.bytes)?;
                    if record.terminated {
                        temp.as_file_mut().write_all(b"\n")?;
                    }
                    kept_records += 1;
                }
                RawJsonlRewrite::Replace(bytes) => {
                    validate_replacement_record(&bytes, record.line_number, path)?;
                    temp.as_file_mut().write_all(&bytes)?;
                    if record.terminated {
                        temp.as_file_mut().write_all(b"\n")?;
                    }
                    replaced_records += 1;
                }
                RawJsonlRewrite::Drop => {
                    dropped_records += 1;
                }
            }
            Ok(())
        },
    )?;
    if input.unterminated_final_record {
        bail!(
            "Refusing to rewrite {} because its final JSONL record is not newline-terminated",
            path.display()
        );
    }

    temp.as_file_mut().flush()?;
    temp.as_file().set_permissions(metadata.permissions())?;
    temp.as_file_mut().sync_all()?;
    let output_file = File::open(temp.path()).with_context(|| {
        format!(
            "Failed to reopen rewritten JSONL stream {}",
            temp.path().display()
        )
    })?;
    let output = scan_jsonl_file(&output_file, temp.path(), cancelled, &mut |record| {
        serde_json::from_slice::<IgnoredAny>(record.bytes).with_context(|| {
            format!(
                "Failed to validate rewritten JSONL record {} in {}",
                record.line_number,
                path.display()
            )
        })?;
        Ok(())
    })?;
    validate(temp.path())
        .with_context(|| format!("Rewritten JSONL validation failed for {}", path.display()))?;
    ensure_state_read_active(cancelled)?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    sync_parent_directory(parent)?;

    Ok(JsonlRewriteStats {
        input,
        output,
        kept_records,
        replaced_records,
        dropped_records,
    })
}

fn validate_replacement_record(bytes: &[u8], line_number: u64, path: &Path) -> Result<()> {
    if bytes.contains(&b'\n') {
        anyhow::bail!(
            "Replacement for JSONL record {} in {} contains a newline",
            line_number,
            path.display()
        );
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        anyhow::bail!(
            "Replacement for JSONL record {} in {} is blank",
            line_number,
            path.display()
        );
    }
    Ok(())
}

fn sync_parent_directory(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("Failed to sync state directory {}", parent.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
    }
    Ok(())
}

pub(super) fn with_jsonl_write_lock<T>(
    path: &Path,
    operation: impl FnOnce(&JsonlWriteGuard) -> Result<T>,
) -> Result<T> {
    let legacy_lock_file = legacy_lock_for_path(path)?;
    if let Some(file) = &legacy_lock_file
        && let Err(error) = file.lock_exclusive()
    {
        return Err(error).context("Failed to lock legacy state file");
    }
    let lock_file = match lock_for_path(path) {
        Ok(file) => file,
        Err(error) => {
            if let Some(file) = &legacy_lock_file {
                let _ = FileExt::unlock(file);
            }
            return Err(error);
        }
    };
    if let Err(error) = lock_file.lock_exclusive() {
        if let Some(file) = &legacy_lock_file {
            let _ = FileExt::unlock(file);
        }
        return Err(error).context("Failed to lock state file");
    }
    let guard = JsonlWriteGuard {
        lock_file,
        legacy_lock_file,
    };
    let result = operation(&guard);
    let legacy_unlock_result = guard
        .legacy_lock_file
        .as_ref()
        .map(FileExt::unlock)
        .unwrap_or(Ok(()));
    let unlock_result = FileExt::unlock(&guard.lock_file);
    match (result, legacy_unlock_result, unlock_result) {
        (Ok(value), Ok(()), Ok(())) => Ok(value),
        (Err(error), _, _) => Err(error),
        (Ok(_), Err(error), _) => Err(error).context("Failed to unlock legacy state file"),
        (Ok(_), Ok(()), Err(error)) => Err(error).context("Failed to unlock state file"),
    }
}

fn lock_for_path(path: &Path) -> Result<File> {
    let lock_path = state_lock_path(path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(&lock_path)
        .with_context(|| format!("Failed to open lock {}", lock_path.display()))
}

fn legacy_lock_for_path(path: &Path) -> Result<Option<File>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    fs::create_dir_all(parent)?;
    // Older Jig versions locked the state file itself. Keep taking that lock
    // during the cache-lock cutover so mixed-version writers still serialize.
    match OpenOptions::new()
        .create(false)
        .truncate(false)
        .write(true)
        .read(true)
        .open(path)
    {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to open legacy lock {}", path.display()))
        }
    }
}

pub(super) fn state_lock_path(path: &Path) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path.with_extension("lock");
    };
    let Some(file_name) = path.file_name() else {
        return path.with_extension("lock");
    };
    if parent.file_name().and_then(|name| name.to_str()) == Some("state")
        && let Some(agent_dir) = parent.parent()
        && agent_dir.file_name().and_then(|name| name.to_str()) == Some(".agent")
    {
        let mut lock_name = file_name.to_os_string();
        lock_name.push(".lock");
        return agent_dir.join(".cache").join("state-locks").join(lock_name);
    }
    let mut lock_name = file_name.to_os_string();
    lock_name.push(".lock");
    parent.join(lock_name)
}

pub(super) fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    read_jsonl_with_cancellation(path, &|| false)
}

pub(super) fn read_jsonl_with_cancellation<T: DeserializeOwned>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>> {
    ensure_state_read_active(cancelled)?;
    loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to open {}", path.display()));
            }
        };
        ensure_state_read_active(cancelled)?;
        loop {
            match FileExt::try_lock_shared(&file) {
                Ok(true) => break,
                Ok(false) => thread::sleep(DATA_LOCK_RETRY_DELAY),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    return read_stable_unlocked_snapshot_with_cancellation(path, cancelled);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to shared-lock {}", path.display()));
                }
            }
            ensure_state_read_active(cancelled)?;
        }

        // Match the writer's data-file-then-cache-lock acquisition order. The
        // cache lock is deliberately opportunistic: reads never create state,
        // but wait cancellably when an existing writer owns it.
        let cache_lock = match lock_existing_cache_with_cancellation(path, cancelled) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = FileExt::unlock(&file);
                return Err(error);
            }
        };
        let is_current = opened_file_is_current(&file, path)?;
        let result =
            is_current.then(|| parse_jsonl_file_with_cancellation(&file, path, false, cancelled));
        let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
        let data_unlock = FileExt::unlock(&file);
        match (result, cache_unlock, data_unlock) {
            (Some(Ok(items)), Ok(()), Ok(())) => return Ok(items),
            (Some(Err(error)), _, _) => return Err(error),
            (Some(Ok(_)), Err(error), _) => {
                return Err(error).context("Failed to unlock state cache file");
            }
            (Some(Ok(_)), Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock state data file");
            }
            (None, Ok(()), Ok(())) => continue,
            (None, Err(error), _) => {
                return Err(error).context("Failed to unlock stale state cache file");
            }
            (None, Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock stale state data file");
            }
        }
    }
}

fn opened_file_is_current(file: &File, path: &Path) -> Result<bool> {
    let opened = file
        .metadata()
        .with_context(|| format!("Failed to inspect open state file {}", path.display()))?;
    let current = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to inspect current state file {}", path.display())
            });
        }
    };
    Ok(same_file_snapshot(&opened, &current))
}

const DATA_LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);

fn lock_existing_cache_with_cancellation(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<File>> {
    let Ok(lock) = File::open(state_lock_path(path)) else {
        return Ok(None);
    };
    loop {
        ensure_state_read_active(cancelled)?;
        match FileExt::try_lock_shared(&lock) {
            Ok(true) => return Ok(Some(lock)),
            Ok(false) => thread::sleep(DATA_LOCK_RETRY_DELAY),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Ok(None),
        }
    }
}

pub(super) fn scan_jsonl_raw(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    mut visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    ensure_state_read_active(cancelled)?;
    loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(JsonlScanStats::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to open {}", path.display()));
            }
        };
        ensure_state_read_active(cancelled)?;
        loop {
            match FileExt::try_lock_shared(&file) {
                Ok(true) => break,
                Ok(false) => thread::sleep(DATA_LOCK_RETRY_DELAY),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    let snapshot = stable_unlocked_raw_snapshot(path, cancelled)?;
                    return scan_jsonl_reader(
                        BufReader::with_capacity(JSONL_READ_CHUNK, snapshot),
                        path,
                        cancelled,
                        &mut visitor,
                    );
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to shared-lock {}", path.display()));
                }
            }
            ensure_state_read_active(cancelled)?;
        }

        let cache_lock = match lock_existing_cache_with_cancellation(path, cancelled) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = FileExt::unlock(&file);
                return Err(error);
            }
        };
        let is_current = opened_file_is_current(&file, path)?;
        let result = is_current.then(|| scan_jsonl_file(&file, path, cancelled, &mut visitor));
        let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
        let data_unlock = FileExt::unlock(&file);
        match (result, cache_unlock, data_unlock) {
            (Some(Ok(stats)), Ok(()), Ok(())) => return Ok(stats),
            (Some(Err(error)), _, _) => return Err(error),
            (Some(Ok(_)), Err(error), _) => {
                return Err(error).context("Failed to unlock state cache file");
            }
            (Some(Ok(_)), Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock state data file");
            }
            (None, Ok(()), Ok(())) => continue,
            (None, Err(error), _) => {
                return Err(error).context("Failed to unlock stale state cache file");
            }
            (None, Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock stale state data file");
            }
        }
    }
}

pub(super) fn scan_jsonl_raw_locked(
    _guard: &JsonlWriteGuard,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    mut visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(JsonlScanStats::default());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to open {} for locked scan", path.display()));
        }
    };
    scan_jsonl_file(&file, path, cancelled, &mut visitor)
}

pub(super) fn read_receipt_window(path: &Path, limit: usize) -> Result<Vec<ReceiptRecord>> {
    read_receipts_reverse(path, limit, |_| true).map(|(receipts, _)| receipts)
}

pub(super) fn receipts_for_plan(
    path: &Path,
    plan_id: &str,
    limit: usize,
) -> Result<Vec<ReceiptRecord>> {
    read_receipts_reverse(path, limit, |receipt| {
        receipt.plan_id.as_deref() == Some(plan_id)
    })
    .map(|(receipts, _)| receipts)
}

#[cfg(test)]
pub(super) fn read_receipt_window_with_bytes(
    path: &Path,
    limit: usize,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    read_receipts_reverse(path, limit, |_| true)
}

#[cfg(test)]
pub(super) fn receipts_for_plan_with_lock(
    path: &Path,
    plan_id: &str,
    limit: usize,
    lock_data: impl FnMut(&File) -> io::Result<()>,
) -> Result<Vec<ReceiptRecord>> {
    read_receipts_reverse_with_lock(
        path,
        limit,
        |receipt| receipt.plan_id.as_deref() == Some(plan_id),
        lock_data,
    )
    .map(|(receipts, _)| receipts)
}

pub(super) fn read_receipts_reverse(
    path: &Path,
    limit: usize,
    predicate: impl Fn(&ReceiptRecord) -> bool,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    read_receipts_reverse_with_lock(path, limit, predicate, FileExt::lock_shared)
}

fn read_receipts_reverse_with_lock(
    path: &Path,
    limit: usize,
    predicate: impl Fn(&ReceiptRecord) -> bool,
    mut lock_data: impl FnMut(&File) -> io::Result<()>,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to open {}", path.display()));
            }
        };
        loop {
            match lock_data(&file) {
                Ok(()) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    let mut read_snapshot = |path: &Path| {
                        fs::read(path).with_context(|| {
                            format!("Failed to read unlocked snapshot {}", path.display())
                        })
                    };
                    let receipts =
                        read_stable_unlocked_snapshot::<ReceiptRecord>(path, &mut read_snapshot)?;
                    let selected = receipts
                        .into_iter()
                        .rev()
                        .filter(&predicate)
                        .take(limit)
                        .collect();
                    return Ok((selected, 0));
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to shared-lock {}", path.display()));
                }
            }
        }
        let cache_lock = match lock_existing_cache_with_cancellation(path, &|| false) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = FileExt::unlock(&file);
                return Err(error);
            }
        };
        let is_current = opened_file_is_current(&file, path)?;
        let result = is_current.then(|| scan_jsonl_reverse(&file, path, limit, &predicate));
        let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
        let data_unlock = FileExt::unlock(&file);
        match (result, cache_unlock, data_unlock) {
            (Some(Ok(value)), Ok(()), Ok(())) => return Ok(value),
            (Some(Err(error)), _, _) => return Err(error),
            (Some(Ok(_)), Err(error), _) => {
                return Err(error).context("Failed to unlock receipt cache lock");
            }
            (Some(Ok(_)), Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock receipt state file");
            }
            (None, Ok(()), Ok(())) => continue,
            (None, Err(error), _) => {
                return Err(error).context("Failed to unlock stale receipt cache lock");
            }
            (None, Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock stale receipt state file");
            }
        }
    }
}

const REVERSE_READ_CHUNK: usize = 16 * 1024;

fn scan_jsonl_reverse(
    file: &File,
    path: &Path,
    limit: usize,
    predicate: impl Fn(&ReceiptRecord) -> bool,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    if limit == 0 {
        return Ok((Vec::new(), 0));
    }
    let mut file = file;
    let mut cursor = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("Failed to seek {}", path.display()))?;
    let mut carry = Vec::new();
    let mut selected = Vec::with_capacity(limit);
    let mut bytes_read = 0u64;
    while cursor > 0 && selected.len() < limit {
        let read_len =
            usize::try_from(cursor.min(REVERSE_READ_CHUNK as u64)).unwrap_or(REVERSE_READ_CHUNK);
        cursor -= read_len as u64;
        file.seek(SeekFrom::Start(cursor))
            .with_context(|| format!("Failed to seek {}", path.display()))?;
        let mut chunk = vec![0u8; read_len];
        file.read_exact(&mut chunk)
            .with_context(|| format!("Failed to read receipt tail {}", path.display()))?;
        bytes_read += read_len as u64;
        chunk.extend_from_slice(&carry);
        let split_at = if cursor == 0 {
            0
        } else {
            chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(chunk.len(), |index| index + 1)
        };
        let complete = &chunk[split_at..];
        for record in complete.split(|byte| *byte == b'\n').rev() {
            if record.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let receipt: ReceiptRecord = serde_json::from_slice(record)
                .with_context(|| format!("Failed to parse receipt tail in {}", path.display()))?;
            if predicate(&receipt) {
                selected.push(receipt);
                if selected.len() == limit {
                    break;
                }
            }
        }
        carry = chunk[..split_at].to_vec();
    }
    Ok((selected, bytes_read))
}

#[cfg(test)]
pub(super) fn read_jsonl_with_data_lock<T: DeserializeOwned>(
    path: &Path,
    lock_data: impl FnMut(&File) -> io::Result<()>,
) -> Result<Vec<T>> {
    read_jsonl_with_io(path, lock_data, |path| {
        fs::read(path)
            .with_context(|| format!("Failed to read unlocked snapshot {}", path.display()))
    })
}

#[cfg(test)]
pub(super) fn read_jsonl_with_io<T: DeserializeOwned>(
    path: &Path,
    mut lock_data: impl FnMut(&File) -> io::Result<()>,
    mut read_snapshot: impl FnMut(&Path) -> Result<Vec<u8>>,
) -> Result<Vec<T>> {
    loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to open {}", path.display()));
            }
        };

        loop {
            match lock_data(&file) {
                Ok(()) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    return read_stable_unlocked_snapshot(path, &mut read_snapshot);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to shared-lock {}", path.display()));
                }
            }
        }

        // Match the writer's data-file-then-cache-lock acquisition order. The
        // cache lock is deliberately opportunistic: reads never create state.
        let cache_lock = File::open(state_lock_path(path))
            .ok()
            .and_then(|lock| FileExt::lock_shared(&lock).ok().map(|()| lock));
        let is_current = opened_file_is_current(&file, path)?;
        let result =
            is_current.then(|| parse_jsonl_file_with_cancellation(&file, path, false, &|| false));
        let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
        let data_unlock = FileExt::unlock(&file);
        match (result, cache_unlock, data_unlock) {
            (Some(Ok(items)), Ok(()), Ok(())) => return Ok(items),
            (Some(Err(error)), _, _) => return Err(error),
            (Some(Ok(_)), Err(error), _) => {
                return Err(error).context("Failed to unlock state cache file");
            }
            (Some(Ok(_)), Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock state data file");
            }
            (None, Ok(()), Ok(())) => continue,
            (None, Err(error), _) => {
                return Err(error).context("Failed to unlock stale state cache file");
            }
            (None, Ok(()), Err(error)) => {
                return Err(error).context("Failed to unlock stale state data file");
            }
        }
    }
}

const UNLOCKED_SNAPSHOT_SAMPLES: usize = 3;
const UNLOCKED_RAW_SNAPSHOT_ATTEMPTS: usize = 3;

fn read_stable_unlocked_snapshot<T: DeserializeOwned>(
    path: &Path,
    read_snapshot: &mut impl FnMut(&Path) -> Result<Vec<u8>>,
) -> Result<Vec<T>> {
    let mut previous_invalid = None;
    for sample in 0..UNLOCKED_SNAPSHOT_SAMPLES {
        let bytes = read_snapshot(path)?;
        match parse_jsonl_snapshot(&bytes, path, true)? {
            ParsedSnapshot::Complete(items) => return Ok(items),
            ParsedSnapshot::InvalidFinal { prefix, error } => {
                if previous_invalid.as_ref() == Some(&bytes) {
                    return Err(error);
                }
                if sample + 1 == UNLOCKED_SNAPSHOT_SAMPLES {
                    return Ok(prefix);
                }
                previous_invalid = Some(bytes);
            }
        }
    }
    unreachable!("unlocked snapshot sampling always returns")
}

fn read_stable_unlocked_snapshot_with_cancellation<T: DeserializeOwned>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>> {
    let mut previous_invalid = None;
    for sample in 0..UNLOCKED_SNAPSHOT_SAMPLES {
        ensure_state_read_active(cancelled)?;
        let bytes = read_snapshot_with_cancellation(path, cancelled)?;
        match parse_jsonl_snapshot_with_cancellation(&bytes, path, true, cancelled)? {
            ParsedSnapshot::Complete(items) => return Ok(items),
            ParsedSnapshot::InvalidFinal { prefix, error } => {
                if previous_invalid.as_ref() == Some(&bytes) {
                    return Err(error);
                }
                if sample + 1 == UNLOCKED_SNAPSHOT_SAMPLES {
                    return Ok(prefix);
                }
                previous_invalid = Some(bytes);
            }
        }
    }
    unreachable!("unlocked snapshot sampling always returns")
}

fn stable_unlocked_raw_snapshot(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<File> {
    for _ in 0..UNLOCKED_RAW_SNAPSHOT_ATTEMPTS {
        ensure_state_read_active(cancelled)?;
        let source = File::open(path)
            .with_context(|| format!("Failed to open unlocked snapshot {}", path.display()))?;
        let before = source
            .metadata()
            .with_context(|| format!("Failed to inspect unlocked snapshot {}", path.display()))?;
        let path_before = fs::metadata(path)
            .with_context(|| format!("Failed to inspect unlocked snapshot {}", path.display()))?;
        if !same_file_snapshot(&before, &path_before) {
            continue;
        }

        let mut snapshot =
            tempfile::tempfile().context("Failed to create bounded unlocked JSONL snapshot")?;
        let mut reader = &source;
        let mut copied = 0u64;
        let mut chunk = [0u8; JSONL_READ_CHUNK];
        loop {
            ensure_state_read_active(cancelled)?;
            let read = reader
                .read(&mut chunk)
                .with_context(|| format!("Failed to read unlocked snapshot {}", path.display()))?;
            if read == 0 {
                break;
            }
            snapshot
                .write_all(&chunk[..read])
                .context("Failed to stage bounded unlocked JSONL snapshot")?;
            copied += read as u64;
        }
        snapshot
            .flush()
            .context("Failed to flush bounded unlocked JSONL snapshot")?;

        let after = source
            .metadata()
            .with_context(|| format!("Failed to inspect unlocked snapshot {}", path.display()))?;
        let Ok(path_after) = fs::metadata(path) else {
            continue;
        };
        if copied == before.len()
            && same_file_snapshot(&before, &after)
            && same_file_snapshot(&after, &path_after)
        {
            snapshot
                .seek(SeekFrom::Start(0))
                .context("Failed to rewind bounded unlocked JSONL snapshot")?;
            return Ok(snapshot);
        }
    }

    bail!(
        "Failed to obtain a stable unlocked snapshot of {} after {} attempts",
        path.display(),
        UNLOCKED_RAW_SNAPSHOT_ATTEMPTS
    )
}

fn same_file_snapshot(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    if left.len() != right.len() || left.modified().ok() != right.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if left.dev() != right.dev() || left.ino() != right.ino() {
            return false;
        }
    }
    true
}

fn read_snapshot_with_cancellation(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to read unlocked snapshot {}", path.display()))?;
    read_file_with_cancellation(&file, path, cancelled)
}

fn read_file_with_cancellation(
    file: &File,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let mut reader = file;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; JSONL_READ_CHUNK];
    loop {
        ensure_state_read_active(cancelled)?;
        let read = reader
            .read(&mut chunk)
            .with_context(|| format!("Failed to read state stream {}", path.display()))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        ensure_state_read_active(cancelled)?;
    }
    Ok(bytes)
}

fn parse_jsonl_file_with_cancellation<T: DeserializeOwned>(
    file: &File,
    path: &Path,
    allow_partial_final: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    scan_jsonl_file(file, path, cancelled, &mut |record| {
        match serde_json::from_slice(record.bytes) {
            Ok(value) => items.push(value),
            Err(_error) if allow_partial_final && !record.terminated => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to parse JSONL record {} in {}",
                        record.line_number,
                        path.display()
                    )
                });
            }
        }
        Ok(())
    })?;
    Ok(items)
}

fn scan_jsonl_file(
    file: &File,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader(
        BufReader::with_capacity(JSONL_READ_CHUNK, file),
        path,
        cancelled,
        visitor,
    )
}

fn scan_jsonl_reader(
    reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_all(reader, path, cancelled, &mut |record, blank| {
        if blank { Ok(()) } else { visitor(record) }
    })
}

fn scan_jsonl_reader_all(
    mut reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>, bool) -> Result<()>,
) -> Result<JsonlScanStats> {
    let mut stats = JsonlScanStats::default();
    let mut line = Vec::new();

    loop {
        ensure_state_read_active(cancelled)?;
        let available = reader
            .fill_buf()
            .with_context(|| format!("Failed to read state stream {}", path.display()))?;
        if available.is_empty() {
            if !line.is_empty() {
                finish_raw_line(&mut stats, &line, false, visitor)?;
            }
            break;
        }

        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            line.extend_from_slice(&available[..newline]);
            let consumed = newline + 1;
            reader.consume(consumed);
            stats.file_bytes += consumed as u64;
            finish_raw_line(&mut stats, &line, true, visitor)?;
            line.clear();
        } else {
            let consumed = available.len();
            line.extend_from_slice(available);
            reader.consume(consumed);
            stats.file_bytes += consumed as u64;
        }
        ensure_state_read_active(cancelled)?;
    }

    ensure_state_read_active(cancelled)?;
    Ok(stats)
}

fn finish_raw_line(
    stats: &mut JsonlScanStats,
    bytes: &[u8],
    terminated: bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>, bool) -> Result<()>,
) -> Result<()> {
    stats.physical_lines += 1;
    let line_number = stats.physical_lines;
    let line_bytes = bytes.len() as u64 + u64::from(terminated);
    if line_bytes > stats.max_line_bytes {
        stats.max_line_bytes = line_bytes;
        stats.max_line_number = Some(line_number);
    }
    let blank = bytes.iter().all(u8::is_ascii_whitespace);
    if blank {
        stats.blank_lines += 1;
    } else {
        stats.records += 1;
        if !terminated {
            stats.unterminated_final_record = true;
        }
    }
    visitor(
        RawJsonlRecord {
            line_number,
            bytes,
            terminated,
        },
        blank,
    )
}

enum ParsedSnapshot<T> {
    Complete(Vec<T>),
    InvalidFinal {
        prefix: Vec<T>,
        error: anyhow::Error,
    },
}

fn parse_jsonl_snapshot<T: DeserializeOwned>(
    bytes: &[u8],
    path: &Path,
    allow_partial_final: bool,
) -> Result<ParsedSnapshot<T>> {
    parse_jsonl_snapshot_with_cancellation(bytes, path, allow_partial_final, &|| false)
}

fn parse_jsonl_snapshot_with_cancellation<T: DeserializeOwned>(
    bytes: &[u8],
    path: &Path,
    allow_partial_final: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<ParsedSnapshot<T>> {
    ensure_state_read_active(cancelled)?;
    let final_record_is_unterminated = !bytes.is_empty() && !bytes.ends_with(b"\n");
    let mut records = bytes.split(|byte| *byte == b'\n').peekable();
    let mut items = Vec::new();
    let mut index = 0;
    while let Some(record) = records.next() {
        ensure_state_read_active(cancelled)?;
        index += 1;
        if record.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice(record) {
            Ok(value) => items.push(value),
            Err(error)
                if allow_partial_final
                    && final_record_is_unterminated
                    && records.peek().is_none() =>
            {
                return Ok(ParsedSnapshot::InvalidFinal {
                    prefix: items,
                    error: anyhow::Error::new(error).context(format!(
                        "Failed to parse JSONL record {} in {}",
                        index,
                        path.display()
                    )),
                });
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to parse JSONL record {} in {}",
                        index,
                        path.display()
                    )
                });
            }
        }
    }
    ensure_state_read_active(cancelled)?;
    Ok(ParsedSnapshot::Complete(items))
}

fn ensure_state_read_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}

#[cfg(test)]
mod streaming_tests {
    use std::cell::Cell;
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn raw_scanner_visits_one_record_at_a_time_and_reports_exact_stats() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        let first = serde_json::to_vec(&json!({ "payload": "x".repeat(96 * 1024) })).unwrap();
        let final_record =
            serde_json::to_vec(&json!({ "id": 2, "payload": "y".repeat(64 * 1024) })).unwrap();
        let mut source = first.clone();
        source.extend_from_slice(b"\n \t\n");
        source.extend_from_slice(&final_record);
        fs::write(&path, &source).unwrap();
        let mut visited = Vec::new();

        let stats = scan_jsonl_raw(&path, &|| false, |record| {
            visited.push((
                record.line_number,
                record.bytes.len(),
                record.bytes.len() as u64 + u64::from(record.terminated),
                record.terminated,
            ));
            Ok(())
        })
        .unwrap();

        assert_eq!(
            visited,
            vec![
                (1, first.len(), first.len() as u64 + 1, true),
                (3, final_record.len(), final_record.len() as u64, false,),
            ]
        );
        assert_eq!(stats.file_bytes, source.len() as u64);
        assert_eq!(stats.physical_lines, 3);
        assert_eq!(stats.records, 2);
        assert_eq!(stats.blank_lines, 1);
        assert_eq!(stats.max_line_bytes, first.len() as u64 + 1);
        assert_eq!(stats.max_line_number, Some(1));
        assert!(stats.unterminated_final_record);
    }

    #[test]
    fn raw_scanner_checks_cancellation_while_building_a_large_record() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        fs::write(&path, vec![b'x'; JSONL_READ_CHUNK * 64]).unwrap();
        let checks = Cell::new(0usize);
        let visited = Cell::new(false);

        let error = scan_jsonl_raw(
            &path,
            &|| {
                let current = checks.get();
                checks.set(current + 1);
                current >= 12
            },
            |_| {
                visited.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "status collection was cancelled");
        assert!(!visited.get());
        assert!(checks.get() > 12);
    }

    #[test]
    fn raw_rewrite_preserves_kept_bytes_and_publishes_validated_output() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        let first = br#"{"id":1,"unknown":{"nested":true}}   "#;
        let mut source = first.to_vec();
        source.extend_from_slice(b"\n\n{\"id\":2}\n{\"id\":3}\n");
        fs::write(&path, &source).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        }

        let stats = with_jsonl_write_lock(&path, |guard| {
            rewrite_jsonl_raw_locked(
                guard,
                &path,
                &|| false,
                |record| {
                    let value: Value = serde_json::from_slice(record.bytes)?;
                    Ok(match value["id"].as_u64().unwrap() {
                        1 => RawJsonlRewrite::Keep,
                        2 => RawJsonlRewrite::Replace(br#"{"id":2,"changed":true}"#.to_vec()),
                        3 => RawJsonlRewrite::Drop,
                        _ => unreachable!(),
                    })
                },
                |temp_path| {
                    let values = read_jsonl::<Value>(temp_path)?;
                    assert_eq!(values.len(), 2);
                    assert_eq!(values[1], json!({ "id": 2, "changed": true }));
                    Ok(())
                },
            )
        })
        .unwrap();

        let mut expected = first.to_vec();
        expected.extend_from_slice(b"\n\n{\"id\":2,\"changed\":true}\n");
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(stats.input.records, 3);
        assert_eq!(stats.output.records, 2);
        assert_eq!(stats.kept_records, 1);
        assert_eq!(stats.replaced_records, 1);
        assert_eq!(stats.dropped_records, 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }

    #[test]
    fn raw_rewrite_validation_failure_leaves_source_exactly_unchanged() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        let source = b"{\"id\":1}\n".to_vec();
        fs::write(&path, &source).unwrap();

        let error = with_jsonl_write_lock(&path, |guard| {
            rewrite_jsonl_raw_locked(
                guard,
                &path,
                &|| false,
                |_| Ok(RawJsonlRewrite::Replace(br#"{"id":2}"#.to_vec())),
                |_| bail!("semantic validator rejected output"),
            )
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Rewritten JSONL validation failed")
        );
        assert_eq!(fs::read(&path).unwrap(), source);
    }

    #[test]
    fn raw_rewrite_rejects_malformed_input_and_unterminated_tail_without_mutation() {
        for source in [
            b"{\"id\":1}\nnot-json\n".as_slice(),
            b"{\"id\":1}\n{\"id\":2}".as_slice(),
        ] {
            let temp = tempdir().unwrap();
            let path = temp.path().join("events.jsonl");
            fs::write(&path, source).unwrap();

            let error = with_jsonl_write_lock(&path, |guard| {
                rewrite_jsonl_raw_locked(
                    guard,
                    &path,
                    &|| false,
                    |_| Ok(RawJsonlRewrite::Keep),
                    |_| Ok(()),
                )
            })
            .unwrap_err();

            assert_eq!(fs::read(&path).unwrap(), source);
            assert!(
                error
                    .to_string()
                    .contains("Failed to parse source JSONL record")
                    || error.to_string().contains("not newline-terminated"),
                "{error:#}"
            );
        }
    }
}
