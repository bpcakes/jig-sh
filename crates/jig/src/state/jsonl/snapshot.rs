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
    with_jsonl_read(
        path,
        &|| false,
        |file| lock_data(file).map(|()| true),
        ReadLockLabels::STATE,
        |access| match access {
            JsonlReadAccess::Missing => Ok(Vec::new()),
            JsonlReadAccess::Locked(file) => {
                parse_jsonl_file_with_cancellation(file, path, false, &|| false)
            }
            JsonlReadAccess::UnsupportedLock => {
                read_stable_unlocked_snapshot(path, &mut read_snapshot)
            }
        },
    )
}

const UNLOCKED_SNAPSHOT_SAMPLES: usize = 3;
const UNLOCKED_RAW_SNAPSHOT_ATTEMPTS: usize = 3;

#[cfg(test)]
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
        ensure_state_read_active(cancelled)?;
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
            ensure_state_read_active(cancelled)?;
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
        ensure_state_read_active(cancelled)?;
        let Ok(path_after) = fs::metadata(path) else {
            continue;
        };
        ensure_state_read_active(cancelled)?;
        let source_unchanged = same_file_snapshot(&before, &after);
        ensure_state_read_active(cancelled)?;
        let path_still_current = same_file_snapshot(&after, &path_after);
        if copied == before.len() && source_unchanged && path_still_current {
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
    scan_jsonl_file_with_limit(file, path, cancelled, None, visitor)
}

pub(super) fn scan_jsonl_file_with_limit(
    file: &File,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_with_limit(
        BufReader::with_capacity(JSONL_READ_CHUNK, file),
        path,
        cancelled,
        max_record_bytes,
        visitor,
    )
}

pub(super) fn scan_jsonl_reader(
    reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_with_limit(reader, path, cancelled, None, visitor)
}

pub(super) fn scan_jsonl_reader_with_limit(
    reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_with_limit_allow_unterminated(
        reader,
        path,
        cancelled,
        max_record_bytes,
        false,
        visitor,
    )
}

pub(super) fn scan_jsonl_reader_with_limit_allow_unterminated(
    reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    allow_unterminated_final: bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_all_with_limit(
        reader,
        path,
        cancelled,
        max_record_bytes,
        allow_unterminated_final,
        &mut |record, blank| {
            if blank { Ok(()) } else { visitor(record) }
        },
    )
}

pub(super) fn scan_jsonl_reader_all(
    reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>, bool) -> Result<()>,
) -> Result<JsonlScanStats> {
    scan_jsonl_reader_all_with_limit(reader, path, cancelled, None, false, visitor)
}

fn scan_jsonl_reader_all_with_limit(
    mut reader: impl BufRead,
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    max_record_bytes: Option<usize>,
    allow_unterminated_final: bool,
    visitor: &mut impl FnMut(RawJsonlRecord<'_>, bool) -> Result<()>,
) -> Result<JsonlScanStats> {
    let mut stats = JsonlScanStats::default();
    let mut line = Vec::new();
    let mut oversized = false;
    let mut line_start_offset = 0_u64;

    loop {
        ensure_state_read_active(cancelled)?;
        let available = reader
            .fill_buf()
            .with_context(|| format!("Failed to read state stream {}", path.display()))?;
        if available.is_empty() {
            if !line.is_empty() || oversized {
                reject_oversized_record(path, line_start_offset, max_record_bytes, oversized)?;
                if !allow_unterminated_final {
                    finish_raw_line(&mut stats, line_start_offset, &line, false, visitor)?;
                }
            }
            break;
        }

        let window_len = available.len().min(JSONL_READ_CHUNK);
        let window = &available[..window_len];
        if let Some(newline) = window.iter().position(|byte| *byte == b'\n') {
            extend_bounded_line(
                &mut line,
                &window[..newline],
                max_record_bytes,
                &mut oversized,
            );
            let consumed = newline + 1;
            reader.consume(consumed);
            stats.file_bytes += consumed as u64;
            reject_oversized_record(path, line_start_offset, max_record_bytes, oversized)?;
            finish_raw_line(&mut stats, line_start_offset, &line, true, visitor)?;
            line.clear();
            oversized = false;
            line_start_offset = stats.file_bytes;
        } else {
            let consumed = window.len();
            extend_bounded_line(&mut line, window, max_record_bytes, &mut oversized);
            reader.consume(consumed);
            stats.file_bytes += consumed as u64;
        }
        ensure_state_read_active(cancelled)?;
    }

    ensure_state_read_active(cancelled)?;
    Ok(stats)
}

fn extend_bounded_line(
    line: &mut Vec<u8>,
    bytes: &[u8],
    max_record_bytes: Option<usize>,
    oversized: &mut bool,
) {
    if *oversized {
        return;
    }
    if max_record_bytes.is_some_and(|limit| line.len().saturating_add(bytes.len()) > limit) {
        *oversized = true;
        return;
    }
    line.extend_from_slice(bytes);
}

fn reject_oversized_record(
    path: &Path,
    start_offset: u64,
    max_record_bytes: Option<usize>,
    oversized: bool,
) -> Result<()> {
    if oversized {
        return Err(JsonlRecordTooLarge {
            path: path.to_path_buf(),
            start_offset,
            limit: max_record_bytes.expect("overflow requires a configured record limit"),
        }
        .into());
    }
    Ok(())
}

pub(super) fn finish_raw_line(
    stats: &mut JsonlScanStats,
    start_offset: u64,
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
            start_offset,
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

#[cfg(test)]
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
