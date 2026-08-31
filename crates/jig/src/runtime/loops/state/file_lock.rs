use std::ffi::OsStr;
use std::path::{Component, Path};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use fs4::fs_std::FileExt;

pub(in crate::runtime::loops) const LOOP_STATE_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOOP_STATE_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

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

pub(in crate::runtime::loops) fn with_exclusive_file_lock_until<T>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    deadline: Instant,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let directory = open_lock_directory(root, dir)?;
    let lock_name = direct_child_name(dir, lock_path)?;
    let lock = open_lock_file(&directory, lock_name, lock_path)?;
    loop {
        match lock.try_lock_exclusive() {
            Ok(true) => break,
            Ok(false) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    bail!(
                        "Timed out waiting for loop state lock {} before its operation deadline",
                        lock_path.display()
                    );
                }
                thread::sleep(LOOP_STATE_LOCK_POLL_INTERVAL.min(remaining));
            }
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

fn open_lock_directory(root: &Path, dir: &Path) -> Result<Dir> {
    let relative = dir.strip_prefix(root).with_context(|| {
        format!(
            "Loop state lock directory {} is outside trusted root {}",
            dir.display(),
            root.display()
        )
    })?;
    let mut directory = Dir::open_ambient_dir(root, ambient_authority())
        .with_context(|| format!("Failed to open loop state lock root {}", root.display()))?;
    let mut opened = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                opened.push(name);
                directory = directory.open_dir_nofollow(name).with_context(|| {
                    format!(
                        "Failed to open loop state lock directory {} without following links",
                        opened.display()
                    )
                })?;
            }
            _ => bail!(
                "Loop state lock directory must be below its trusted root: {}",
                dir.display()
            ),
        }
    }
    Ok(directory)
}

fn direct_child_name<'a>(dir: &Path, path: &'a Path) -> Result<&'a OsStr> {
    if path.parent() != Some(dir) {
        bail!(
            "Loop state lock {} is not directly inside {}",
            path.display(),
            dir.display()
        );
    }
    path.file_name()
        .ok_or_else(|| anyhow!("Loop state lock path has no file name: {}", path.display()))
}

fn open_lock_file(directory: &Dir, name: &OsStr, path: &Path) -> Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .read(true)
        .write(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = directory
        .open_with(name, &options)
        .map(cap_std::fs::File::into_std)
        .with_context(|| {
            format!(
                "Failed to open loop state lock {} without following links",
                path.display()
            )
        })?;
    if !file.metadata()?.is_file() {
        bail!("Loop state lock is not a regular file: {}", path.display());
    }
    Ok(file)
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

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_lock_file_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target = target_dir.path().join("outside.lock");
        std::fs::write(&target, b"outside").unwrap();
        let lock_path = temp.path().join("schedule.lock");
        symlink(&target, &lock_path).unwrap();

        let error = with_exclusive_file_lock_until(
            temp.path(),
            temp.path(),
            &lock_path,
            loop_state_lock_deadline(),
            || Ok(()),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("without following links"));
        assert_eq!(std::fs::read(&target).unwrap(), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_lock_directory_component() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        symlink(target_dir.path(), temp.path().join("loop")).unwrap();
        let dir = temp.path().join("loop");
        let lock_path = dir.join("schedule.lock");

        let error = with_exclusive_file_lock_until(
            temp.path(),
            &dir,
            &lock_path,
            loop_state_lock_deadline(),
            || Ok(()),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("without following links"));
        assert!(!target_dir.path().join("schedule.lock").exists());
    }
}
