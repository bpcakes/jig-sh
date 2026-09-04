use std::collections::VecDeque;

use super::*;

pub(in crate::state) fn read_receipt_window(
    path: &Path,
    limit: usize,
) -> Result<Vec<ReceiptRecord>> {
    read_receipts_reverse(path, limit, |_| true).map(|(receipts, _)| receipts)
}

#[cfg(test)]
pub(in crate::state) fn read_receipt_window_with_bytes(
    path: &Path,
    limit: usize,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    read_receipts_reverse(path, limit, |_| true)
}
#[cfg(test)]
pub(in crate::state) fn receipts_for_plan_with_lock(
    path: &Path,
    plan_id: &str,
    limit: usize,
    mut lock_data: impl FnMut(&File) -> io::Result<()>,
) -> Result<Vec<ReceiptRecord>> {
    read_receipts_reverse_with_lock(
        path,
        limit,
        |receipt| receipt.plan_id.as_deref() == Some(plan_id),
        &|| false,
        None,
        |file| lock_data(file).map(|()| true),
    )
    .map(|(receipts, _)| receipts)
}

pub(in crate::state) fn read_receipts_reverse(
    path: &Path,
    limit: usize,
    predicate: impl Fn(&ReceiptRecord) -> bool,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    read_receipts_reverse_with_lock(path, limit, predicate, &|| false, None, |file| {
        FileExt::lock_shared(file).map(|()| true)
    })
}

// Task B2 wires this into the worker-owned dashboard source. Keeping the
// primitive separate prevents today's uncancellable web path from polling.
#[allow(dead_code)]
pub(crate) fn read_receipts_reverse_with_cancellation(
    path: &Path,
    limit: usize,
    predicate: impl Fn(&ReceiptRecord) -> bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    read_receipts_reverse_with_lock(
        path,
        limit,
        predicate,
        cancelled,
        Some(DASHBOARD_JSONL_RECORD_BYTES),
        FileExt::try_lock_shared,
    )
}

fn read_receipts_reverse_with_lock(
    path: &Path,
    limit: usize,
    predicate: impl Fn(&ReceiptRecord) -> bool,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    mut lock_data: impl FnMut(&File) -> io::Result<bool>,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    ensure_state_read_active(cancelled)?;
    loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to open {}", path.display()));
            }
        };
        ensure_state_read_active(cancelled)?;
        loop {
            ensure_state_read_active(cancelled)?;
            match lock_data(&file) {
                Ok(true) => break,
                Ok(false) => thread::sleep(DATA_LOCK_RETRY_DELAY),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    let snapshot = stable_unlocked_raw_snapshot(path, cancelled)?;
                    return scan_jsonl_reverse(
                        &snapshot,
                        path,
                        limit,
                        &predicate,
                        cancelled,
                        max_record_bytes,
                        true,
                    );
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to shared-lock {}", path.display()));
                }
            }
        }
        let cache_lock = match lock_existing_cache_with_cancellation(path, cancelled) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = FileExt::unlock(&file);
                return Err(error);
            }
        };
        let is_current = opened_file_is_current(&file, path)?;
        let result = is_current.then(|| {
            scan_jsonl_reverse(
                &file,
                path,
                limit,
                &predicate,
                cancelled,
                max_record_bytes,
                false,
            )
        });
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
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    allow_unterminated_final: bool,
) -> Result<(Vec<ReceiptRecord>, u64)> {
    ensure_state_read_active(cancelled)?;
    if limit == 0 {
        return Ok((Vec::new(), 0));
    }
    let mut file = file;
    let mut cursor = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("Failed to seek {}", path.display()))?;
    let mut bytes_read = 0u64;
    if cursor > 0 {
        ensure_state_read_active(cancelled)?;
        file.seek(SeekFrom::Start(cursor - 1))
            .with_context(|| format!("Failed to seek {}", path.display()))?;
        let mut final_byte = [0u8; 1];
        file.read_exact(&mut final_byte)
            .with_context(|| format!("Failed to inspect receipt tail {}", path.display()))?;
        if final_byte[0] != b'\n' {
            if !allow_unterminated_final {
                bail!(
                    "Refusing to inspect {} because its final JSONL record is not newline-terminated",
                    path.display()
                );
            }
            let (stable_cursor, skipped) = skip_unterminated_tail(file, path, cursor, cancelled)?;
            cursor = stable_cursor;
            bytes_read = bytes_read.saturating_add(skipped);
        }
    }
    let mut pending = VecDeque::<Vec<u8>>::new();
    let mut pending_bytes = 0_usize;
    // The limit can originate at a transport boundary. Grow with observed
    // matches instead of trusting it as an allocation size.
    let mut selected = Vec::new();
    let mut oversized = false;
    while cursor > 0 && selected.len() < limit {
        ensure_state_read_active(cancelled)?;
        let read_len =
            usize::try_from(cursor.min(REVERSE_READ_CHUNK as u64)).unwrap_or(REVERSE_READ_CHUNK);
        cursor -= read_len as u64;
        file.seek(SeekFrom::Start(cursor))
            .with_context(|| format!("Failed to seek {}", path.display()))?;
        let mut chunk = vec![0u8; read_len];
        file.read_exact(&mut chunk)
            .with_context(|| format!("Failed to read receipt tail {}", path.display()))?;
        ensure_state_read_active(cancelled)?;
        bytes_read += read_len as u64;
        if oversized {
            if let Some(newline) = chunk.iter().rposition(|byte| *byte == b'\n') {
                return Err(JsonlRecordTooLarge {
                    path: path.to_path_buf(),
                    start_offset: cursor + newline as u64 + 1,
                    limit: max_record_bytes.expect("overflow requires a configured record limit"),
                }
                .into());
            }
            if cursor == 0 {
                return Err(JsonlRecordTooLarge {
                    path: path.to_path_buf(),
                    start_offset: 0,
                    limit: max_record_bytes.expect("overflow requires a configured record limit"),
                }
                .into());
            }
            continue;
        }
        let Some(last_newline) = chunk.iter().rposition(|byte| *byte == b'\n') else {
            if cursor == 0 {
                let record = assemble_record(&chunk, &pending, pending_bytes);
                select_receipt(
                    &record,
                    0,
                    path,
                    max_record_bytes,
                    &predicate,
                    &mut selected,
                    limit,
                    cancelled,
                )?;
                break;
            }
            pending_bytes = pending_bytes.saturating_add(chunk.len());
            if max_record_bytes.is_some_and(|limit| pending_bytes.saturating_sub(1) > limit) {
                oversized = true;
                pending.clear();
                pending_bytes = 0;
            } else {
                pending.push_front(chunk);
            }
            continue;
        };

        let suffix = &chunk[last_newline + 1..];
        let cross_record_bytes = suffix
            .len()
            .saturating_add(pending_bytes.saturating_sub(usize::from(pending_bytes > 0)));
        if max_record_bytes.is_some_and(|limit| cross_record_bytes > limit) {
            return Err(JsonlRecordTooLarge {
                path: path.to_path_buf(),
                start_offset: cursor + last_newline as u64 + 1,
                limit: max_record_bytes.expect("record overflow requires a limit"),
            }
            .into());
        }
        if cross_record_bytes > 0 {
            let record = assemble_record(suffix, &pending, pending_bytes);
            if select_receipt(
                &record,
                cursor + last_newline as u64 + 1,
                path,
                max_record_bytes,
                &predicate,
                &mut selected,
                limit,
                cancelled,
            )? {
                break;
            }
        }
        pending.clear();
        pending_bytes = 0;

        let mut record_end = last_newline;
        while let Some(previous_newline) =
            chunk[..record_end].iter().rposition(|byte| *byte == b'\n')
        {
            let record = &chunk[previous_newline + 1..record_end];
            if select_receipt(
                record,
                cursor + previous_newline as u64 + 1,
                path,
                max_record_bytes,
                &predicate,
                &mut selected,
                limit,
                cancelled,
            )? {
                break;
            }
            record_end = previous_newline;
        }
        if selected.len() == limit {
            break;
        }
        if cursor == 0 {
            select_receipt(
                &chunk[..record_end],
                0,
                path,
                max_record_bytes,
                &predicate,
                &mut selected,
                limit,
                cancelled,
            )?;
        } else if max_record_bytes.is_some_and(|limit| record_end > limit) {
            oversized = true;
        } else {
            let carry = chunk[..=record_end].to_vec();
            pending_bytes = carry.len();
            pending.push_back(carry);
        }
    }
    ensure_state_read_active(cancelled)?;
    Ok((selected, bytes_read))
}

fn assemble_record(prefix: &[u8], pending: &VecDeque<Vec<u8>>, pending_bytes: usize) -> Vec<u8> {
    let mut record = Vec::with_capacity(prefix.len().saturating_add(pending_bytes));
    record.extend_from_slice(prefix);
    for chunk in pending {
        record.extend_from_slice(chunk);
    }
    if record.last() == Some(&b'\n') {
        record.pop();
    }
    record
}

#[allow(clippy::too_many_arguments)]
fn select_receipt(
    record: &[u8],
    start_offset: u64,
    path: &Path,
    max_record_bytes: Option<usize>,
    predicate: &impl Fn(&ReceiptRecord) -> bool,
    selected: &mut Vec<ReceiptRecord>,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool> {
    ensure_state_read_active(cancelled)?;
    if record.iter().all(u8::is_ascii_whitespace) {
        return Ok(false);
    }
    if max_record_bytes.is_some_and(|limit| record.len() > limit) {
        return Err(JsonlRecordTooLarge {
            path: path.to_path_buf(),
            start_offset,
            limit: max_record_bytes.expect("record overflow requires a limit"),
        }
        .into());
    }
    let receipt: ReceiptRecord = serde_json::from_slice(record)
        .with_context(|| format!("Failed to parse receipt tail in {}", path.display()))?;
    if predicate(&receipt) {
        selected.push(receipt);
    }
    Ok(selected.len() == limit)
}

fn skip_unterminated_tail(
    mut file: &File,
    path: &Path,
    mut cursor: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<(u64, u64)> {
    let mut bytes_read = 0_u64;
    while cursor > 0 {
        ensure_state_read_active(cancelled)?;
        let read_len =
            usize::try_from(cursor.min(REVERSE_READ_CHUNK as u64)).unwrap_or(REVERSE_READ_CHUNK);
        cursor -= read_len as u64;
        file.seek(SeekFrom::Start(cursor))
            .with_context(|| format!("Failed to seek {}", path.display()))?;
        let mut chunk = vec![0_u8; read_len];
        file.read_exact(&mut chunk).with_context(|| {
            format!("Failed to skip incomplete receipt tail {}", path.display())
        })?;
        ensure_state_read_active(cancelled)?;
        bytes_read = bytes_read.saturating_add(read_len as u64);
        if let Some(newline) = chunk.iter().rposition(|byte| *byte == b'\n') {
            return Ok((cursor + newline as u64 + 1, bytes_read));
        }
    }
    Ok((0, bytes_read))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn data_lock_wait_is_cancellable() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        fs::write(&path, b"{}\n").unwrap();
        let checks = Cell::new(0_usize);

        let error = read_receipts_reverse_with_lock(
            &path,
            1,
            |_| true,
            &|| {
                checks.set(checks.get() + 1);
                checks.get() > 3
            },
            Some(DASHBOARD_JSONL_RECORD_BYTES),
            |_| Ok(false),
        )
        .unwrap_err();

        assert!(crate::cancellation::is_status_collection_cancellation(
            &error
        ));
        assert!(checks.get() > 3);
    }

    #[test]
    fn cache_lock_wait_is_cancellable() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        fs::write(&path, b"{}\n").unwrap();
        let lock_path = state_lock_path(&path);
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .unwrap();
        lock.lock_exclusive().unwrap();
        let checks = Cell::new(0_usize);

        let error = read_receipts_reverse_with_cancellation(&path, 1, |_| true, &|| {
            checks.set(checks.get() + 1);
            checks.get() > 3
        })
        .unwrap_err();
        FileExt::unlock(&lock).unwrap();

        assert!(crate::cancellation::is_status_collection_cancellation(
            &error
        ));
        assert!(checks.get() > 3);
    }

    #[test]
    fn reverse_scan_polls_cancellation_between_chunks() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        let mut bytes = vec![b'x'; REVERSE_READ_CHUNK * 2];
        bytes.push(b'\n');
        fs::write(&path, bytes).unwrap();
        let checks = Cell::new(0_usize);

        let error = read_receipts_reverse_with_cancellation(&path, 1, |_| true, &|| {
            let current = checks.get();
            checks.set(current + 1);
            current >= 6
        })
        .unwrap_err();

        assert!(crate::cancellation::is_status_collection_cancellation(
            &error
        ));
        assert!(checks.get() > 6);
    }

    #[test]
    fn reverse_scan_rejects_an_oversized_record_at_its_exact_offset() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        let prefix = b"{}\n";
        let mut bytes = prefix.to_vec();
        bytes.extend(std::iter::repeat_n(b'x', DASHBOARD_JSONL_RECORD_BYTES + 1));
        bytes.push(b'\n');
        fs::write(&path, bytes).unwrap();

        let error =
            read_receipts_reverse_with_cancellation(&path, 1, |_| true, &|| false).unwrap_err();
        let oversized = error.downcast_ref::<JsonlRecordTooLarge>().unwrap();

        assert_eq!(oversized.start_offset(), prefix.len() as u64);
        assert_eq!(oversized.limit(), DASHBOARD_JSONL_RECORD_BYTES);
        assert!(error.to_string().contains("state archive"));
    }

    #[test]
    fn stable_fallback_copy_is_cancellable_between_chunks() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        let mut bytes = vec![b'x'; REVERSE_READ_CHUNK * 8];
        bytes.push(b'\n');
        fs::write(&path, bytes).unwrap();
        let checks = Cell::new(0_usize);

        let error = read_receipts_reverse_with_lock(
            &path,
            1,
            |_| true,
            &|| {
                checks.set(checks.get() + 1);
                checks.get() > 8
            },
            Some(DASHBOARD_JSONL_RECORD_BYTES),
            |_| Err(io::Error::from(io::ErrorKind::Unsupported)),
        )
        .unwrap_err();

        assert!(crate::cancellation::is_status_collection_cancellation(
            &error
        ));
        assert!(checks.get() > 8);
    }

    #[test]
    fn stable_fallback_comparison_is_cancellable() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("receipts.jsonl");
        fs::write(&path, b"{}\n").unwrap();
        let checks = Cell::new(0_usize);

        let error = read_receipts_reverse_with_lock(
            &path,
            1,
            |_| true,
            &|| {
                checks.set(checks.get() + 1);
                checks.get() > 10
            },
            Some(DASHBOARD_JSONL_RECORD_BYTES),
            |_| Err(io::Error::from(io::ErrorKind::Unsupported)),
        )
        .unwrap_err();

        assert!(crate::cancellation::is_status_collection_cancellation(
            &error
        ));
        assert!(checks.get() > 10);
    }
}
