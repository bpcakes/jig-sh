use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;

use crate::cancellation::ensure_status_collection_active;
use crate::context::RepoContext;

use super::jsonl::state_lock_path;

const LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(super) fn read(ctx: &RepoContext) -> Result<Option<String>> {
    read_with_cancellation(ctx, &|| false)
}

pub(super) fn read_with_cancellation(
    ctx: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    let pointer_path = ctx.current_session_path();
    let lock_path = state_lock_path(&pointer_path);
    ensure_status_collection_active(cancelled)?;

    if let Some(lock_file) = open_read_lock(&lock_path)? {
        return read_locked(ctx, lock_file, &lock_path, cancelled);
    }

    // Legacy repositories can have a pointer without the sibling lock. Read
    // that snapshot without creating cache state, then check the lock again.
    // Every current writer creates the persistent lock before changing the
    // pointer, so a lock that appeared during the read sends us through the
    // synchronized path; otherwise the pre-transition snapshot is coherent.
    let current = read_unlocked(ctx)?;
    ensure_status_collection_active(cancelled)?;
    if let Some(lock_file) = open_read_lock(&lock_path)? {
        return read_locked(ctx, lock_file, &lock_path, cancelled);
    }
    Ok(current)
}

pub(super) fn with_write_lock<T>(
    ctx: &RepoContext,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let pointer_path = ctx.current_session_path();
    let lock_path = state_lock_path(&pointer_path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| {
            format!(
                "Failed to open current-session lock {}",
                lock_path.display()
            )
        })?;
    FileExt::lock_exclusive(&lock_file).with_context(|| {
        format!(
            "Failed to lock current-session state {}",
            lock_path.display()
        )
    })?;
    finish_locked(operation(), &lock_file, &lock_path)
}

pub(super) fn read_unlocked(ctx: &RepoContext) -> Result<Option<String>> {
    let path = ctx.current_session_path();
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub(super) fn write_locked(ctx: &RepoContext, session_id: Option<&str>) -> Result<()> {
    let path = ctx.current_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match session_id {
        Some(value) => fs::write(path, format!("{value}\n"))?,
        None => {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn open_read_lock(path: &Path) -> Result<Option<File>> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to open current-session lock {}", path.display())),
    }
}

fn read_locked(
    ctx: &RepoContext,
    lock_file: File,
    lock_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<String>> {
    loop {
        ensure_status_collection_active(cancelled)?;
        match FileExt::try_lock_shared(&lock_file) {
            Ok(true) => break,
            Ok(false) => thread::sleep(LOCK_RETRY_DELAY),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to lock current-session state {}",
                        lock_path.display()
                    )
                });
            }
        }
    }
    ensure_status_collection_active(cancelled)?;
    finish_locked(read_unlocked(ctx), &lock_file, lock_path)
}

fn finish_locked<T>(result: Result<T>, lock_file: &File, lock_path: &Path) -> Result<T> {
    let unlock = FileExt::unlock(lock_file).with_context(|| {
        format!(
            "Failed to unlock current-session state {}",
            lock_path.display()
        )
    });
    match (result, unlock) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn reader_opens_the_sidecar_without_write_access() {
        let temp = tempfile::tempdir().unwrap();
        let lock_path = temp.path().join("current-session.lock");
        fs::write(&lock_path, b"").unwrap();

        let mut lock = open_read_lock(&lock_path).unwrap().unwrap();

        assert!(lock.write_all(b"unexpected write").is_err());
    }
}
