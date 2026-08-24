use super::*;

#[cfg(test)]
pub(in crate::state) fn read_jsonl_with_data_lock<T: DeserializeOwned>(
    path: &Path,
    lock_data: impl FnMut(&File) -> io::Result<()>,
) -> Result<Vec<T>> {
    read_jsonl_with_io(path, lock_data, |path| {
        fs::read(path)
            .with_context(|| format!("Failed to read unlocked snapshot {}", path.display()))
    })
}

#[cfg(test)]
pub(in crate::state) fn read_jsonl_with_io<T: DeserializeOwned>(
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

pub(super) fn read_stable_unlocked_snapshot<T: DeserializeOwned>(
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

pub(super) fn read_stable_unlocked_snapshot_with_cancellation<T: DeserializeOwned>(
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

pub(super) fn stable_unlocked_raw_snapshot(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<File> {
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

pub(super) fn same_file_snapshot(left: &fs::Metadata, right: &fs::Metadata) -> bool {
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

pub(super) fn read_snapshot_with_cancellation(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to read unlocked snapshot {}", path.display()))?;
    read_file_with_cancellation(&file, path, cancelled)
}

pub(super) fn read_file_with_cancellation(
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

pub(super) fn parse_jsonl_file_with_cancellation<T: DeserializeOwned>(
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

pub(super) fn scan_jsonl_file(
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

pub(super) fn scan_jsonl_reader(
    reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_all(reader, path, cancelled, &mut |record, blank| {
        if blank { Ok(()) } else { visitor(record) }
    })
}

pub(super) fn scan_jsonl_reader_all(
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

pub(super) fn finish_raw_line(
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

pub(super) enum ParsedSnapshot<T> {
    Complete(Vec<T>),
    InvalidFinal {
        prefix: Vec<T>,
        error: anyhow::Error,
    },
}

pub(super) fn parse_jsonl_snapshot<T: DeserializeOwned>(
    bytes: &[u8],
    path: &Path,
    allow_partial_final: bool,
) -> Result<ParsedSnapshot<T>> {
    parse_jsonl_snapshot_with_cancellation(bytes, path, allow_partial_final, &|| false)
}

pub(super) fn parse_jsonl_snapshot_with_cancellation<T: DeserializeOwned>(
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

pub(super) fn ensure_state_read_active(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure_status_collection_active(cancelled)
}
