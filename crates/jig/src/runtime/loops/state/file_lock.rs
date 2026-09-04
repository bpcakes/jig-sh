#[cfg(test)]
use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(test)]
use anyhow::Result;

pub(in crate::runtime::loops) const LOOP_STATE_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

pub(in crate::runtime::loops) fn loop_state_lock_deadline() -> Instant {
    Instant::now() + LOOP_STATE_LOCK_TIMEOUT
}

#[cfg(test)]
pub(in crate::runtime::loops) fn with_exclusive_file_lock<T>(
    dir: &Path,
    lock_path: &Path,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    with_exclusive_file_lock_until(dir, dir, lock_path, loop_state_lock_deadline(), action)
}

#[cfg(test)]
pub(in crate::runtime::loops) fn with_exclusive_file_lock_until<T>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    deadline: Instant,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let directory = super::json_cache::StateDirectory::open(root, dir)?;
    let lock_name = super::json_cache::cache_file_name(dir, lock_path)?;
    directory.with_lock_until(&lock_name, lock_path, deadline, &|| false, action)
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

        let error = with_exclusive_file_lock_until(
            temp.path(),
            temp.path(),
            &lock_path,
            Instant::now(),
            || Ok(()),
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("Timed out waiting for loop state lock"),
            "{error}"
        );
    }
}
