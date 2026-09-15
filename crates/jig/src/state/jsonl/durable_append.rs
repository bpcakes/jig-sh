use super::*;

#[cfg(test)]
thread_local! {
    static DURABLE_APPEND_FAILURE: std::cell::Cell<Option<DurableAppendFailurePoint>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DurableAppendFailurePoint {
    BeforeFileSync,
    BeforeParentSync,
}

#[cfg(test)]
pub(crate) fn fail_next_durable_append_at(point: DurableAppendFailurePoint) {
    DURABLE_APPEND_FAILURE.with(|failure| failure.set(Some(point)));
}

#[cfg(test)]
fn take_durable_append_failure(point: DurableAppendFailurePoint) -> bool {
    DURABLE_APPEND_FAILURE.with(|failure| {
        if failure.get() == Some(point) {
            failure.set(None);
            true
        } else {
            false
        }
    })
}

pub(in crate::state) fn append_jsonl_locked<T: Serialize>(
    guard: &JsonlWriteGuard,
    path: &Path,
    value: &T,
) -> Result<u64> {
    append_jsonl_locked_with_durability(guard, path, value, false)
}

/// Append a record and durably publish the journal name before returning.
///
/// Tracker authority uses this stronger boundary before external side effects.
/// Other state streams retain their historical file-sync-only behavior.
pub(in crate::state) fn append_jsonl_durable_locked<T: Serialize>(
    guard: &JsonlWriteGuard,
    path: &Path,
    value: &T,
) -> Result<u64> {
    append_jsonl_locked_with_durability(guard, path, value, true)
}

fn append_jsonl_locked_with_durability<T: Serialize>(
    _guard: &JsonlWriteGuard,
    path: &Path,
    value: &T,
    sync_parent: bool,
) -> Result<u64> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    #[cfg(test)]
    if sync_parent && take_durable_append_failure(DurableAppendFailurePoint::BeforeFileSync) {
        bail!("injected durable append failure before file sync");
    }
    file.sync_data()?;
    if sync_parent {
        #[cfg(test)]
        if take_durable_append_failure(DurableAppendFailurePoint::BeforeParentSync) {
            bail!("injected durable append failure before parent sync");
        }
        sync_parent_directory(path.parent().unwrap_or_else(|| Path::new(".")))?;
    }
    Ok(file.metadata()?.len())
}

/// Re-confirm an already visible tracker record after an ambiguous prior append.
pub(in crate::state) fn confirm_jsonl_durable_locked(
    _guard: &JsonlWriteGuard,
    path: &Path,
) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| {
            format!(
                "Failed to reopen {} for durability confirmation",
                path.display()
            )
        })?;
    file.sync_data()
        .with_context(|| format!("Failed to sync existing {}", path.display()))?;
    sync_parent_directory(path.parent().unwrap_or_else(|| Path::new(".")))
}
