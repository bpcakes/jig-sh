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
pub(crate) const DASHBOARD_JSONL_RECORD_BYTES: usize = 1024 * 1024;

#[cfg(test)]
thread_local! {
    static DASHBOARD_SCAN_COUNTS: std::cell::RefCell<std::collections::BTreeMap<PathBuf, usize>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

#[cfg(test)]
pub(crate) fn reset_dashboard_scan_counts() {
    DASHBOARD_SCAN_COUNTS.with(|counts| counts.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn dashboard_scan_count(path: &Path) -> usize {
    DASHBOARD_SCAN_COUNTS.with(|counts| counts.borrow().get(path).copied().unwrap_or(0))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RawJsonlRecord<'a> {
    pub(crate) line_number: u64,
    pub(crate) start_offset: u64,
    pub(crate) bytes: &'a [u8],
    pub(crate) terminated: bool,
}

#[derive(Debug)]
pub(crate) struct JsonlRecordTooLarge {
    path: PathBuf,
    start_offset: u64,
    limit: usize,
}

impl JsonlRecordTooLarge {
    #[cfg(test)]
    pub(crate) fn start_offset(&self) -> u64 {
        self.start_offset
    }

    #[cfg(test)]
    pub(crate) fn limit(&self) -> usize {
        self.limit
    }
}

impl std::fmt::Display for JsonlRecordTooLarge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let recovery = match self.path.file_name().and_then(|name| name.to_str()) {
            Some("sessions.jsonl") => {
                "run `scripts/jig state diagnose --deep`, then preview `scripts/jig state compact sessions --dry-run`"
            }
            Some("receipts.jsonl") => {
                "run `scripts/jig state diagnose --deep`, then preview `scripts/jig state archive --before <cutoff> --dry-run`"
            }
            _ => {
                "run `scripts/jig state diagnose --deep`; this stream has no automatic compaction command"
            }
        };
        write!(
            formatter,
            "JSONL record at byte {} in {} exceeds the {}-byte dashboard read limit; {recovery}",
            self.start_offset,
            self.path.display(),
            self.limit
        )
    }
}

impl std::error::Error for JsonlRecordTooLarge {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct JsonlScanStats {
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
    append_jsonl_with_end_offset(path, value).map(|_| ())
}

pub(super) fn append_jsonl_with_end_offset<T: Serialize>(path: &Path, value: &T) -> Result<u64> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    with_jsonl_write_lock(path, |guard| append_jsonl_locked(guard, path, value))
}

pub(super) fn append_jsonl_locked<T: Serialize>(
    _guard: &JsonlWriteGuard,
    path: &Path,
    value: &T,
) -> Result<u64> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    Ok(file.metadata()?.len())
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
    with_jsonl_read(
        path,
        cancelled,
        FileExt::try_lock_shared,
        ReadLockLabels::STATE,
        |access| match access {
            JsonlReadAccess::Missing => Ok(Vec::new()),
            JsonlReadAccess::Locked(file) => {
                parse_jsonl_file_with_cancellation(file, path, false, cancelled)
            }
            JsonlReadAccess::UnsupportedLock => {
                read_stable_unlocked_snapshot_with_cancellation(path, cancelled)
            }
        },
    )
}

pub(super) fn opened_file_is_current(file: &File, path: &Path) -> Result<bool> {
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

pub(super) fn lock_existing_cache_with_cancellation(
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
    visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_raw_with_limit(path, cancelled, None, visitor)
}

pub(crate) fn scan_dashboard_jsonl_raw(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    #[cfg(test)]
    DASHBOARD_SCAN_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let count = counts.entry(path.to_path_buf()).or_default();
        *count = count.saturating_add(1);
    });
    scan_jsonl_raw_with_limit(path, cancelled, Some(DASHBOARD_JSONL_RECORD_BYTES), visitor)
}

pub(crate) fn read_dashboard_jsonl<T: DeserializeOwned>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    scan_dashboard_jsonl_raw(path, cancelled, |record| {
        let value = serde_json::from_slice(record.bytes).with_context(|| {
            format!(
                "Failed to parse dashboard JSONL record {} at byte {} in {}",
                record.line_number,
                record.start_offset,
                path.display()
            )
        })?;
        items.push(value);
        Ok(())
    })?;
    Ok(items)
}

fn scan_jsonl_raw_with_limit(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_raw_with_limit_and_lock(
        path,
        cancelled,
        max_record_bytes,
        visitor,
        FileExt::try_lock_shared,
    )
}

#[cfg(test)]
fn scan_dashboard_jsonl_raw_with_lock(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
    lock_data: impl FnMut(&File) -> io::Result<bool>,
) -> Result<JsonlScanStats> {
    scan_jsonl_raw_with_limit_and_lock(
        path,
        cancelled,
        Some(DASHBOARD_JSONL_RECORD_BYTES),
        visitor,
        lock_data,
    )
}

fn scan_jsonl_raw_with_limit_and_lock(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    mut visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
    lock_data: impl FnMut(&File) -> io::Result<bool>,
) -> Result<JsonlScanStats> {
    with_jsonl_read(
        path,
        cancelled,
        lock_data,
        ReadLockLabels::STATE,
        |access| match access {
            JsonlReadAccess::Missing => Ok(JsonlScanStats::default()),
            JsonlReadAccess::Locked(file) => {
                scan_jsonl_file_with_limit(file, path, cancelled, max_record_bytes, &mut visitor)
            }
            JsonlReadAccess::UnsupportedLock => {
                let snapshot = stable_unlocked_raw_snapshot(path, cancelled)?;
                scan_jsonl_reader_with_limit_allow_unterminated(
                    BufReader::with_capacity(JSONL_READ_CHUNK, snapshot),
                    path,
                    cancelled,
                    max_record_bytes,
                    true,
                    &mut visitor,
                )
            }
        },
    )
}

#[cfg(test)]
pub(super) fn jsonl_end_offset(path: &Path) -> Result<u64> {
    match File::open(path) {
        Ok(file) => {
            FileExt::lock_shared(&file)
                .with_context(|| format!("Failed to shared-lock {}", path.display()))?;
            let offset = file
                .metadata()
                .map(|metadata| metadata.len())
                .with_context(|| format!("Failed to inspect {}", path.display()));
            let unlock = FileExt::unlock(&file)
                .with_context(|| format!("Failed to unlock {}", path.display()));
            match (offset, unlock) {
                (Ok(offset), Ok(())) => Ok(offset),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error).with_context(|| format!("Failed to open {}", path.display())),
    }
}

pub(super) fn scan_jsonl_raw_from(
    path: &Path,
    offset: u64,
    cancelled: &dyn Fn() -> bool,
    mut visitor: impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<(u64, JsonlScanStats)> {
    ensure_state_read_active(cancelled)?;
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((0, JsonlScanStats::default()));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to open {}", path.display()));
        }
    };
    loop {
        match FileExt::try_lock_shared(&file) {
            Ok(true) => break,
            Ok(false) => thread::sleep(DATA_LOCK_RETRY_DELAY),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to shared-lock {}", path.display()));
            }
        }
        ensure_state_read_active(cancelled)?;
    }
    ensure_state_read_active(cancelled)?;
    let file_len = file.metadata()?.len();
    let start = if offset <= file_len { offset } else { 0 };
    file.seek(SeekFrom::Start(start))?;
    let result = scan_jsonl_reader(
        BufReader::with_capacity(JSONL_READ_CHUNK, &file),
        path,
        cancelled,
        &mut visitor,
    );
    let unlock = FileExt::unlock(&file);
    let stats = result?;
    unlock.with_context(|| format!("Failed to unlock {}", path.display()))?;
    if stats.unterminated_final_record {
        bail!(
            "Refusing to advance a JSONL cursor past an unterminated record in {}",
            path.display()
        );
    }
    Ok((start + stats.file_bytes, stats))
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

mod read_access;
use read_access::{JsonlReadAccess, ReadLockLabels, with_jsonl_read};

mod reverse;
pub(super) use reverse::read_receipts_reverse;
pub(crate) use reverse::read_receipts_reverse_with_cancellation;
#[cfg(test)]
pub(super) use reverse::{
    read_receipt_window, read_receipt_window_with_bytes, receipts_for_plan_with_lock,
};

mod snapshot;
use snapshot::*;
#[cfg(test)]
pub(super) use snapshot::{read_jsonl_with_data_lock, read_jsonl_with_io};

#[cfg(test)]
mod streaming_tests;
