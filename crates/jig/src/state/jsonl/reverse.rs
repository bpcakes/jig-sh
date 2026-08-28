use super::*;

pub(in crate::state) fn read_receipt_window(
    path: &Path,
    limit: usize,
) -> Result<Vec<ReceiptRecord>> {
    read_receipts_reverse(path, limit, |_| true).map(|(receipts, _)| receipts)
}

pub(in crate::state) fn receipts_for_plan(
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

pub(in crate::state) fn read_receipts_reverse(
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
    if cursor > 0 {
        file.seek(SeekFrom::Start(cursor - 1))
            .with_context(|| format!("Failed to seek {}", path.display()))?;
        let mut final_byte = [0u8; 1];
        file.read_exact(&mut final_byte)
            .with_context(|| format!("Failed to inspect receipt tail {}", path.display()))?;
        if final_byte[0] != b'\n' {
            bail!(
                "Refusing to inspect {} because its final JSONL record is not newline-terminated",
                path.display()
            );
        }
    }
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
