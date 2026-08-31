use std::fs::{self, OpenOptions};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;

const LOOP_STATE_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOOP_STATE_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(in crate::runtime::loops) fn with_exclusive_file_lock<T>(
    dir: &Path,
    lock_path: &Path,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    with_exclusive_file_lock_until(
        dir,
        lock_path,
        Instant::now() + LOOP_STATE_LOCK_TIMEOUT,
        action,
    )
}

fn with_exclusive_file_lock_until<T>(
    dir: &Path,
    lock_path: &Path,
    deadline: Instant,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .with_context(|| format!("Failed to open loop state lock {}", lock_path.display()))?;
    loop {
        match lock.try_lock_exclusive() {
            Ok(true) => break,
            Ok(false) if Instant::now() >= deadline => bail!(
                "Timed out waiting for loop state lock {} after {LOOP_STATE_LOCK_TIMEOUT:?}",
                lock_path.display()
            ),
            Ok(false) => thread::sleep(LOOP_STATE_LOCK_POLL_INTERVAL),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to lock {}", lock_path.display()));
            }
        }
    }

    let result = action();
    drop(lock);
    result
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use fs4::fs_std::FileExt;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn an_owned_loop_state_lock_times_out_instead_of_blocking() {
        let temp = tempdir().unwrap();
        let lock_path = temp.path().join("schedule.lock");
        let owner = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        assert!(owner.try_lock_exclusive().unwrap());

        let error =
            with_exclusive_file_lock_until(temp.path(), &lock_path, Instant::now(), || Ok(()))
                .unwrap_err()
                .to_string();

        assert!(
            error.contains("Timed out waiting for loop state lock"),
            "{error}"
        );
    }
}
