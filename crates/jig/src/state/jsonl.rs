//! Append-only JSONL persistence, locking, snapshots, and bounded receipt reads.
//!
//! Durable schemas live in `records`; this module owns only their storage mechanics.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

use crate::cancellation::ensure_status_collection_active;

use super::records::ReceiptRecord;

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
    for value in values {
        serde_json::to_writer(&mut temp, value)?;
        temp.write_all(b"\n")?;
    }
    temp.as_file_mut().sync_data()?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

pub(super) fn with_jsonl_write_lock<T>(
    path: &Path,
    operation: impl FnOnce(&JsonlWriteGuard) -> Result<T>,
) -> Result<T> {
    let legacy_lock_file = legacy_lock_for_path(path)?;
    if let Some(file) = &legacy_lock_file {
        if let Err(error) = file.lock_exclusive() {
            return Err(error).context("Failed to lock legacy state file");
        }
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
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(path)
        .map(Some)
        .with_context(|| format!("Failed to open legacy lock {}", path.display()))
}

pub(super) fn state_lock_path(path: &Path) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path.with_extension("lock");
    };
    let Some(file_name) = path.file_name() else {
        return path.with_extension("lock");
    };
    if parent.file_name().and_then(|name| name.to_str()) == Some("state") {
        if let Some(agent_dir) = parent.parent() {
            if agent_dir.file_name().and_then(|name| name.to_str()) == Some(".agent") {
                let mut lock_name = file_name.to_os_string();
                lock_name.push(".lock");
                return agent_dir.join(".cache").join("state-locks").join(lock_name);
            }
        }
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
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to open {}", path.display()));
        }
    };

    loop {
        ensure_state_read_active(cancelled)?;
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
    }

    let result = (|| {
        ensure_state_read_active(cancelled)?;
        // Match the writer's data-file-then-cache-lock acquisition order. The
        // cache lock is deliberately opportunistic: reads never create state,
        // but wait cancellably when an existing writer owns it.
        let cache_lock = lock_existing_cache_with_cancellation(path, cancelled)?;
        let result = parse_jsonl_file_with_cancellation(&file, path, false, cancelled);
        let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
        match (result, cache_unlock) {
            (Ok(items), Ok(())) => Ok(items),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error).context("Failed to unlock state cache file"),
        }
    })();
    let data_unlock = FileExt::unlock(&file);
    match (result, data_unlock) {
        (Ok(items), Ok(())) => Ok(items),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("Failed to unlock state data file"),
    }
}

const DATA_LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);

fn lock_existing_cache_with_cancellation(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<File>> {
    let lock = match File::open(state_lock_path(path)) {
        Ok(lock) => lock,
        Err(_) => return Ok(None),
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

fn read_receipts_reverse(
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
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
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
                    .filter(predicate)
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
    let result = scan_jsonl_reverse(&file, path, limit, predicate);
    let unlock = FileExt::unlock(&file);
    match (result, unlock) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("Failed to unlock receipt state file"),
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

pub(super) fn read_jsonl_locked<T: DeserializeOwned>(
    _guard: &JsonlWriteGuard,
    path: &Path,
) -> Result<Vec<T>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open {} for locked read", path.display()))?;
    parse_jsonl_file(&file, path, false)
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
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
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
    let result = parse_jsonl_file(&file, path, false);
    let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
    let data_unlock = FileExt::unlock(&file);
    match (result, cache_unlock, data_unlock) {
        (Ok(items), Ok(()), Ok(())) => Ok(items),
        (Err(error), _, _) => Err(error),
        (Ok(_), Err(error), _) => Err(error).context("Failed to unlock state cache file"),
        (Ok(_), Ok(()), Err(error)) => Err(error).context("Failed to unlock state data file"),
    }
}

const UNLOCKED_SNAPSHOT_SAMPLES: usize = 3;

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

const JSONL_READ_CHUNK: usize = 16 * 1024;

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

fn parse_jsonl_file<T: DeserializeOwned>(
    file: &File,
    path: &Path,
    allow_partial_final: bool,
) -> Result<Vec<T>> {
    parse_jsonl_file_with_cancellation(file, path, allow_partial_final, &|| false)
}

fn parse_jsonl_file_with_cancellation<T: DeserializeOwned>(
    file: &File,
    path: &Path,
    allow_partial_final: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>> {
    let bytes = read_file_with_cancellation(file, path, cancelled)?;
    match parse_jsonl_snapshot_with_cancellation(&bytes, path, allow_partial_final, cancelled)? {
        ParsedSnapshot::Complete(items) => Ok(items),
        ParsedSnapshot::InvalidFinal { error, .. } => Err(error),
    }
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
