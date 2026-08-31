use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Component;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::bail;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use fs4::fs_std::FileExt;

use super::*;

const CACHE_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(test)]
pub(super) fn with_json_cache_lock<T, S>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    action: impl FnOnce(&mut S) -> Result<T>,
) -> Result<T>
where
    S: Default + DeserializeOwned + Serialize,
{
    with_json_cache_lock_until(
        root,
        dir,
        lock_path,
        data_path,
        loop_state_lock_deadline(),
        action,
    )
}

pub(super) fn with_json_cache_lock_until<T, S>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    deadline: Instant,
    action: impl FnOnce(&mut S) -> Result<T>,
) -> Result<T>
where
    S: Default + DeserializeOwned + Serialize,
{
    let cache = StateDirectory::open(root, dir)?;
    let lock_name = cache_file_name(dir, lock_path)?;
    let data_name = cache_file_name(dir, data_path)?;
    cache.with_lock_until(&lock_name, lock_path, deadline, || {
        cache.reclaim_orphaned_temps(&data_name, data_path)?;
        let mut store: S = cache.read_json_or_default(&data_name, data_path, &|| false)?;
        let result = action(&mut store)?;
        cache.write_json(&data_name, data_path, &store)?;
        Ok(result)
    })
}

pub(super) fn with_json_cache_lock_compensating_until<T, U, S>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    deadline: Instant,
    action: impl FnOnce(&mut S) -> Result<T>,
    after_commit: impl FnOnce(&T) -> Result<U>,
) -> Result<(T, U)>
where
    S: Clone + Default + DeserializeOwned + Serialize,
{
    let cache = StateDirectory::open(root, dir)?;
    let lock_name = cache_file_name(dir, lock_path)?;
    let data_name = cache_file_name(dir, data_path)?;
    cache.with_lock_until(&lock_name, lock_path, deadline, || {
        cache.reclaim_orphaned_temps(&data_name, data_path)?;
        let mut store: S = cache.read_json_or_default(&data_name, data_path, &|| false)?;
        let rollback = store.clone();
        let result = action(&mut store)?;
        cache.write_json(&data_name, data_path, &store)?;
        match after_commit(&result) {
            Ok(effect) => Ok((result, effect)),
            Err(error) if crate::state::receipt_append_may_have_landed(&error) => Err(error
                .context(
                    "Committed loop state was retained because its receipt append may have landed",
                )),
            Err(error) => match cache.write_json(&data_name, data_path, &rollback) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error.context(format!(
                    "Failed to roll back committed loop state: {rollback_error:#}"
                ))),
            },
        }
    })
}

pub(super) fn read_json_cache_or_default_with_cancellation<T>(
    root: &Path,
    dir: &Path,
    data_path: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    let Some(cache) = StateDirectory::open_existing(root, dir)? else {
        return Ok(T::default());
    };
    let data_name = cache_file_name(dir, data_path)?;
    cache.read_json_or_default(&data_name, data_path, cancelled)
}

pub(super) fn read_json_cache_locked_until<T>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    deadline: Instant,
) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    let cache = StateDirectory::open(root, dir)?;
    let lock_name = cache_file_name(dir, lock_path)?;
    let data_name = cache_file_name(dir, data_path)?;
    cache.with_lock_until(&lock_name, lock_path, deadline, || {
        cache.read_json_or_default(&data_name, data_path, &|| false)
    })
}

pub(super) fn recover_unparsable_json_cache<T>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
) -> Result<bool>
where
    T: Default + DeserializeOwned + Serialize,
{
    replace_unparsable_json_cache(root, dir, lock_path, data_path, T::default())
}

pub(super) fn replace_unparsable_json_cache<T>(
    root: &Path,
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    replacement: T,
) -> Result<bool>
where
    T: Default + DeserializeOwned + Serialize,
{
    let cache = StateDirectory::open(root, dir)?;
    let lock_name = cache_file_name(dir, lock_path)?;
    let data_name = cache_file_name(dir, data_path)?;
    cache.with_lock_until(&lock_name, lock_path, loop_state_lock_deadline(), || {
        cache.reclaim_orphaned_temps(&data_name, data_path)?;
        match cache.read_json_or_default::<T>(&data_name, data_path, &|| false) {
            Ok(_) => Ok(false),
            Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => {
                cache.write_json(&data_name, data_path, &replacement)?;
                Ok(true)
            }
            Err(error) => Err(error),
        }
    })
}

pub(in crate::runtime::loops) struct StateDirectory {
    directory: Dir,
}

impl StateDirectory {
    pub(in crate::runtime::loops) fn open(root: &Path, cache_dir: &Path) -> Result<Self> {
        Self::open_with_creation(root, cache_dir, true)?.ok_or_else(|| {
            anyhow!(
                "Failed to create loop cache directory {}",
                cache_dir.display()
            )
        })
    }

    pub(in crate::runtime::loops) fn open_existing(
        root: &Path,
        cache_dir: &Path,
    ) -> Result<Option<Self>> {
        Self::open_with_creation(root, cache_dir, false)
    }

    fn open_with_creation(root: &Path, cache_dir: &Path, create: bool) -> Result<Option<Self>> {
        let relative = cache_dir.strip_prefix(root).with_context(|| {
            format!(
                "Loop cache directory {} is outside repository root {}",
                cache_dir.display(),
                root.display()
            )
        })?;
        let mut directory = Dir::open_ambient_dir(root, ambient_authority())
            .with_context(|| format!("Failed to open repository root {}", root.display()))?;
        let mut opened = root.to_path_buf();
        for component in relative.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(name) => {
                    opened.push(name);
                    directory = match open_directory(&directory, name, &opened, create)? {
                        Some(directory) => directory,
                        None => return Ok(None),
                    };
                }
                _ => bail!(
                    "Loop cache directory must be repository-relative: {}",
                    cache_dir.display()
                ),
            }
        }
        Ok(Some(Self { directory }))
    }

    pub(in crate::runtime::loops) fn with_lock_until<T>(
        &self,
        lock_name: &OsStr,
        lock_path: &Path,
        deadline: Instant,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let lock = open_regular_file(&self.directory, lock_name, true, true, false, lock_path)?;
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
                    thread::sleep(CACHE_LOCK_POLL_INTERVAL.min(remaining));
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

    pub(in crate::runtime::loops) fn read_json_or_default<T>(
        &self,
        data_name: &OsStr,
        data_path: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<T>
    where
        T: Default + DeserializeOwned,
    {
        ensure_status_active(cancelled)?;
        let Some(mut file) = open_optional_regular_file(&self.directory, data_name, data_path)?
        else {
            return Ok(T::default());
        };
        let mut bytes = Vec::new();
        let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            ensure_status_active(cancelled)?;
            let read = file
                .read(&mut chunk)
                .with_context(|| format!("Failed to read {}", data_path.display()))?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        ensure_status_active(cancelled)?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse {}", data_path.display()))
    }

    pub(in crate::runtime::loops) fn reclaim_orphaned_temps(
        &self,
        data_name: &OsStr,
        data_path: &Path,
    ) -> Result<()> {
        let prefix_path = Path::new(data_name).with_extension("tmp-");
        let prefix = prefix_path
            .file_name()
            .ok_or_else(|| anyhow!("Loop state path has no file name: {}", data_path.display()))?;
        for entry in self.directory.entries().with_context(|| {
            format!(
                "Failed to inspect loop cache directory {}",
                data_path.parent().unwrap_or(data_path).display()
            )
        })? {
            let entry = entry.with_context(|| {
                format!(
                    "Failed to inspect loop cache directory {}",
                    data_path.parent().unwrap_or(data_path).display()
                )
            })?;
            let name = entry.file_name();
            if !name
                .as_encoded_bytes()
                .starts_with(prefix.as_encoded_bytes())
            {
                continue;
            }
            let file_type = entry.file_type().with_context(|| {
                format!(
                    "Failed to inspect loop cache temporary entry {}",
                    data_path
                        .parent()
                        .unwrap_or(data_path)
                        .join(&name)
                        .display()
                )
            })?;
            if file_type.is_file() || file_type.is_symlink() {
                self.directory.remove_file(&name).with_context(|| {
                    format!(
                        "Failed to reclaim loop cache temporary file {}",
                        data_path
                            .parent()
                            .unwrap_or(data_path)
                            .join(&name)
                            .display()
                    )
                })?;
            }
        }
        Ok(())
    }

    fn write_json<T: Serialize>(
        &self,
        data_name: &OsStr,
        data_path: &Path,
        value: &T,
    ) -> Result<()> {
        let tmp_name = Path::new(data_name).with_extension(format!("tmp-{}", Ulid::new()));
        let tmp_path = data_path.parent().unwrap_or(data_path).join(&tmp_name);
        let result = (|| {
            let mut tmp = open_regular_file(
                &self.directory,
                tmp_name.as_os_str(),
                true,
                true,
                true,
                &tmp_path,
            )?;
            tmp.write_all(
                &serde_json::to_vec_pretty(value).context("Failed to encode loop state JSON")?,
            )
            .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
            drop(tmp);
            self.directory
                .rename(&tmp_name, &self.directory, data_name)
                .with_context(|| {
                    format!(
                        "Failed to replace loop cache file {} with {}",
                        data_path.display(),
                        tmp_path.display()
                    )
                })
        })();
        match result {
            Ok(()) => Ok(()),
            Err(error) => match self.directory.remove_file(&tmp_name) {
                Ok(()) => Err(error),
                Err(cleanup_error) if cleanup_error.kind() == io::ErrorKind::NotFound => Err(error),
                Err(cleanup_error) => Err(error.context(format!(
                    "Failed to remove temporary loop cache file {}: {cleanup_error}",
                    tmp_path.display()
                ))),
            },
        }
    }

    pub(in crate::runtime::loops) fn exists(
        &self,
        data_name: &OsStr,
        data_path: &Path,
    ) -> Result<bool> {
        open_optional_regular_file(&self.directory, data_name, data_path).map(|file| file.is_some())
    }

    pub(in crate::runtime::loops) fn read_json<T>(
        &self,
        data_name: &OsStr,
        data_path: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        ensure_status_active(cancelled)?;
        let Some(mut file) = open_optional_regular_file(&self.directory, data_name, data_path)?
        else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            ensure_status_active(cancelled)?;
            let read = file
                .read(&mut chunk)
                .with_context(|| format!("Failed to read {}", data_path.display()))?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        ensure_status_active(cancelled)?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse {}", data_path.display()))
            .map(Some)
    }

    pub(in crate::runtime::loops) fn write_json_durable<T: Serialize>(
        &self,
        data_name: &OsStr,
        data_path: &Path,
        value: &T,
    ) -> Result<()> {
        let _ = self.exists(data_name, data_path)?;
        let tmp_name = Path::new(data_name).with_extension(format!("tmp-{}", Ulid::new()));
        let tmp_path = data_path.parent().unwrap_or(data_path).join(&tmp_name);
        let result = (|| {
            let mut tmp = open_regular_file(
                &self.directory,
                tmp_name.as_os_str(),
                true,
                true,
                true,
                &tmp_path,
            )?;
            tmp.write_all(
                &serde_json::to_vec_pretty(value).context("Failed to encode loop state JSON")?,
            )
            .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
            tmp.sync_all()
                .with_context(|| format!("Failed to sync {}", tmp_path.display()))?;
            drop(tmp);
            self.directory
                .rename(&tmp_name, &self.directory, data_name)
                .with_context(|| {
                    format!(
                        "Failed to replace loop state file {} with {}",
                        data_path.display(),
                        tmp_path.display()
                    )
                })?;
            self.directory
                .open(".")
                .and_then(|directory| directory.sync_all())
                .with_context(|| {
                    format!(
                        "Failed to sync loop state directory {}",
                        data_path.parent().unwrap_or(data_path).display()
                    )
                })
        })();
        match result {
            Ok(()) => Ok(()),
            Err(error) => match self.directory.remove_file(&tmp_name) {
                Ok(()) => Err(error),
                Err(cleanup_error) if cleanup_error.kind() == io::ErrorKind::NotFound => Err(error),
                Err(cleanup_error) => Err(error.context(format!(
                    "Failed to remove temporary loop state file {}: {cleanup_error}",
                    tmp_path.display()
                ))),
            },
        }
    }
}

pub(in crate::runtime::loops) fn cache_file_name(
    cache_dir: &Path,
    path: &Path,
) -> Result<OsString> {
    if path.parent() != Some(cache_dir) {
        bail!(
            "Loop cache file {} is not directly inside {}",
            path.display(),
            cache_dir.display()
        );
    }
    path.file_name()
        .map(OsStr::to_os_string)
        .ok_or_else(|| anyhow!("Loop cache path has no file name: {}", path.display()))
}

fn open_directory(parent: &Dir, name: &OsStr, path: &Path, create: bool) -> Result<Option<Dir>> {
    match parent.open_dir_nofollow(name) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to create loop cache directory {}", path.display())
                    });
                }
            }
            parent.open_dir_nofollow(name).map(Some).with_context(|| {
                format!(
                    "Failed to open loop cache directory {} without following links",
                    path.display()
                )
            })
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to open loop cache directory {} without following links",
                path.display()
            )
        }),
    }
}

fn open_optional_regular_file(directory: &Dir, name: &OsStr, path: &Path) -> Result<Option<File>> {
    match open_regular_file(directory, name, false, false, false, path) {
        Ok(file) => Ok(Some(file)),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn open_regular_file(
    directory: &Dir,
    name: &OsStr,
    writable: bool,
    create: bool,
    create_new: bool,
    path: &Path,
) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(!create_new)
        .write(writable)
        .create(create)
        .create_new(create_new)
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
                "Failed to open loop cache file {} without following links",
                path.display()
            )
        })?;
    if !file.metadata()?.is_file() {
        bail!("Loop cache path is not a regular file: {}", path.display());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn locked_cache_access_reclaims_orphaned_temp_files() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        let lock_path = temp.path().join("attempts.lock");
        let orphan = temp.path().join("attempts.tmp-ExampleOrphan");
        fs::write(&orphan, b"partial").unwrap();

        with_json_cache_lock::<_, BTreeMap<String, String>>(
            temp.path(),
            temp.path(),
            &lock_path,
            &data_path,
            |_| Ok(()),
        )
        .unwrap();

        assert!(!orphan.exists());
        assert!(data_path.exists());
    }

    #[test]
    fn locked_cache_access_ignores_non_file_temp_entries() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        let lock_path = temp.path().join("attempts.lock");
        let unrelated_directory = temp.path().join("attempts.tmp-ExampleDirectory");
        fs::create_dir(&unrelated_directory).unwrap();

        with_json_cache_lock::<_, BTreeMap<String, String>>(
            temp.path(),
            temp.path(),
            &lock_path,
            &data_path,
            |_| Ok(()),
        )
        .unwrap();

        assert!(unrelated_directory.is_dir());
        assert!(data_path.is_file());
    }

    #[test]
    fn read_only_cache_access_does_not_rewrite_the_cache() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        let original = b"{\n  \"example\": \"value\"\n}\n";
        fs::write(&data_path, original).unwrap();

        let value = read_json_cache_or_default_with_cancellation::<BTreeMap<String, String>>(
            temp.path(),
            temp.path(),
            &data_path,
            &|| false,
        )
        .unwrap();

        assert_eq!(value.get("example").map(String::as_str), Some("value"));
        assert_eq!(fs::read(&data_path).unwrap(), original);
    }

    #[test]
    fn failed_cache_replacement_removes_its_temp_file() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        fs::create_dir(&data_path).unwrap();
        let cache = StateDirectory::open(temp.path(), temp.path()).unwrap();

        let error = cache
            .write_json(
                OsStr::new("attempts.json"),
                &data_path,
                &BTreeMap::<String, String>::new(),
            )
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to replace loop cache file")
        );
        assert!(fs::read_dir(temp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("attempts.tmp-")
        }));
    }

    #[test]
    fn compensating_cache_retains_commit_when_receipt_may_have_landed() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        let lock_path = temp.path().join("attempts.lock");

        let error = with_json_cache_lock_compensating_until(
            temp.path(),
            temp.path(),
            &lock_path,
            &data_path,
            loop_state_lock_deadline(),
            |state: &mut BTreeMap<String, String>| {
                state.insert("ExampleProject".into(), "cleared".into());
                Ok(())
            },
            |_| -> Result<()> { Err(crate::state::receipt_append_may_have_landed_for_test()) },
        )
        .unwrap_err();

        let state = read_json_cache_or_default_with_cancellation::<BTreeMap<String, String>>(
            temp.path(),
            temp.path(),
            &data_path,
            &|| false,
        )
        .unwrap();
        assert_eq!(
            state.get("ExampleProject").map(String::as_str),
            Some("cleared")
        );
        assert!(
            format!("{error:#}").contains("receipt append may have landed"),
            "{error:#}"
        );
    }
}
