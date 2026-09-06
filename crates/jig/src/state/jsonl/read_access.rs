use super::*;

pub(super) enum JsonlReadAccess<'a> {
    Missing,
    Locked(&'a File),
    UnsupportedLock,
}

#[derive(Clone, Copy)]
pub(super) struct ReadLockLabels {
    cache: &'static str,
    data: &'static str,
}

impl ReadLockLabels {
    pub(super) const STATE: Self = Self {
        cache: "state cache file",
        data: "state data file",
    };
    pub(super) const RECEIPT: Self = Self {
        cache: "receipt cache lock",
        data: "receipt state file",
    };
}

/// Invoke the reader only after selecting a current locked file, a missing
/// path, or an unsupported-lock fallback. Replaced files are retried before
/// invoking the reader, so a streaming visitor's effects are never replayed.
pub(super) fn with_jsonl_read<T>(
    path: &Path,
    cancelled: &dyn Fn() -> bool,
    mut lock_data: impl FnMut(&File) -> io::Result<bool>,
    labels: ReadLockLabels,
    mut read: impl FnMut(JsonlReadAccess<'_>) -> Result<T>,
) -> Result<T> {
    ensure_state_read_active(cancelled)?;
    loop {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return read(JsonlReadAccess::Missing);
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to open {}", path.display()));
            }
        };
        loop {
            ensure_state_read_active(cancelled)?;
            match lock_data(&file) {
                Ok(true) => break,
                Ok(false) => thread::sleep(DATA_LOCK_RETRY_DELAY),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                    return read(JsonlReadAccess::UnsupportedLock);
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("Failed to shared-lock {}", path.display()));
                }
            }
        }

        // Match the writer's data-file-then-cache-lock order. Reads never
        // create state, but wait cancellably for an existing cache lock.
        let cache_lock = match lock_existing_cache_with_cancellation(path, cancelled) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = FileExt::unlock(&file);
                return Err(error);
            }
        };
        let is_current = opened_file_is_current(&file, path)?;
        let result = is_current.then(|| read(JsonlReadAccess::Locked(&file)));
        // Always attempt both unlocks, even after a read or cache-unlock error.
        let cache_unlock = cache_lock.as_ref().map(FileExt::unlock).unwrap_or(Ok(()));
        let data_unlock = FileExt::unlock(&file);
        if let Some(value) = finish_read(result, cache_unlock, data_unlock, labels)? {
            return Ok(value);
        }
    }
}

fn finish_read<T>(
    result: Option<Result<T>>,
    cache_unlock: io::Result<()>,
    data_unlock: io::Result<()>,
    labels: ReadLockLabels,
) -> Result<Option<T>> {
    let stale = if result.is_none() { "stale " } else { "" };
    let value = result.transpose()?;
    cache_unlock.with_context(|| format!("Failed to unlock {stale}{}", labels.cache))?;
    data_unlock.with_context(|| format!("Failed to unlock {stale}{}", labels.data))?;
    Ok(value)
}

#[cfg(test)]
mod tests;
