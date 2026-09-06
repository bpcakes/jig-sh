use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Write};
use std::path::Component;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::bail;
#[cfg(unix)]
use cap_fs_ext::OpenOptionsMaybeDirExt;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use fs4::fs_std::FileExt;

use super::bounded_json::{encode_bounded_json, read_bounded_json};
use super::*;

#[cfg(test)]
#[path = "json_cache/limit_tests.rs"]
mod limit_tests;
#[cfg(test)]
#[path = "json_cache/lock_creation_tests.rs"]
mod lock_creation_tests;

const CACHE_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

include!("json_cache/temp_names.rs");
include!("json_cache/durable.rs");
include!("json_cache/lock_file.rs");

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
    let location = JsonLocation {
        root: root.to_path_buf(),
        dir: dir.to_path_buf(),
        path: data_path.to_path_buf(),
        lock_path: lock_path.to_path_buf(),
        write_mode: JsonWriteMode::Cache,
    };
    with_json_cache_lock_until(&location, loop_state_lock_deadline(), &|| false, action)
}

pub(super) fn with_json_cache_lock_until<T, S>(
    location: &JsonLocation,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    action: impl FnOnce(&mut S) -> Result<T>,
) -> Result<T>
where
    S: Default + DeserializeOwned + Serialize,
{
    let cache = StateDirectory::open(&location.root, &location.dir)?;
    let lock_name = cache_file_name(&location.dir, &location.lock_path)?;
    let data_name = cache_file_name(&location.dir, &location.path)?;
    cache.with_lock_until(&lock_name, &location.lock_path, deadline, cancelled, || {
        cache.reclaim_orphaned_temps(&data_name, &location.path)?;
        let mut store: S = cache.read_json_or_default(&data_name, &location.path, cancelled)?;
        let result = action(&mut store)?;
        cache.write_json_with_mode(&data_name, &location.path, &store, location.write_mode)?;
        Ok(result)
    })
}

pub(super) fn with_json_cache_lock_compensating_until<T, U, S>(
    location: &JsonLocation,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
    action: impl FnOnce(&mut S) -> Result<T>,
    after_commit: impl FnOnce(&T, Instant) -> Result<U>,
) -> Result<(T, U)>
where
    S: Clone + Default + DeserializeOwned + Serialize,
{
    let cache = StateDirectory::open(&location.root, &location.dir)?;
    let lock_name = cache_file_name(&location.dir, &location.lock_path)?;
    let data_name = cache_file_name(&location.dir, &location.path)?;
    cache.with_lock_until(&lock_name, &location.lock_path, deadline, cancelled, || {
        cache.reclaim_orphaned_temps(&data_name, &location.path)?;
        let mut store: S = cache.read_json_or_default(&data_name, &location.path, cancelled)?;
        let rollback = store.clone();
        let result = action(&mut store)?;
        cache.write_json_with_mode(&data_name, &location.path, &store, location.write_mode)?;
        match after_commit(&result, loop_state_lock_deadline()) {
            Ok(effect) => Ok((result, effect)),
            Err(error) if crate::state::receipt_append_may_have_landed(&error) => Err(error
                .context(
                    "Committed loop state was retained because its receipt append may have landed",
                )),
            Err(error) => {
                match cache.write_json_with_mode(
                    &data_name,
                    &location.path,
                    &rollback,
                    location.write_mode,
                ) {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(error.context(format!(
                        "Failed to roll back committed loop state: {rollback_error:#}"
                    ))),
                }
            }
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
    cancelled: &dyn Fn() -> bool,
) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    let cache = StateDirectory::open(root, dir)?;
    let lock_name = cache_file_name(dir, lock_path)?;
    let data_name = cache_file_name(dir, data_path)?;
    cache.with_lock_until(&lock_name, lock_path, deadline, cancelled, || {
        cache.read_json_or_default(&data_name, data_path, cancelled)
    })
}

pub(super) fn recover_unparsable_json_cache<T>(
    location: &JsonLocation,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool>
where
    T: Default + DeserializeOwned + Serialize,
{
    replace_unparsable_json_cache(location, T::default(), cancelled)
}

pub(super) fn replace_unparsable_json_cache<T>(
    location: &JsonLocation,
    replacement: T,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool>
where
    T: Default + DeserializeOwned + Serialize,
{
    let cache = StateDirectory::open(&location.root, &location.dir)?;
    let lock_name = cache_file_name(&location.dir, &location.lock_path)?;
    let data_name = cache_file_name(&location.dir, &location.path)?;
    cache.with_lock_until(
        &lock_name,
        &location.lock_path,
        loop_state_lock_deadline(),
        cancelled,
        || {
            cache.reclaim_orphaned_temps(&data_name, &location.path)?;
            match cache.read_json_or_default::<T>(&data_name, &location.path, cancelled) {
                Ok(_) => Ok(false),
                Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => {
                    cache.write_json_with_mode(
                        &data_name,
                        &location.path,
                        &replacement,
                        location.write_mode,
                    )?;
                    Ok(true)
                }
                Err(error) => Err(error),
            }
        },
    )
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
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let lock = open_or_create_lock_file(&self.directory, lock_name, lock_path)?;
        loop {
            if cancelled() {
                bail!(
                    "Execution was cancelled while waiting for loop state lock {}",
                    lock_path.display()
                );
            }
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
        read_bounded_json(&mut file, data_path, cancelled)
    }

    pub(in crate::runtime::loops) fn reclaim_orphaned_temps(
        &self,
        data_name: &OsStr,
        data_path: &Path,
    ) -> Result<()> {
        let prefix = temporary_file_prefix(data_name);
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
        let tmp_name = temporary_file_name(data_name);
        let tmp_path = data_path.parent().unwrap_or(data_path).join(&tmp_name);
        let result = (|| {
            let encoded = encode_bounded_json(value, data_path)?;
            let mut tmp = open_regular_file(
                &self.directory,
                tmp_name.as_os_str(),
                true,
                true,
                true,
                &tmp_path,
            )?;
            tmp.write_all(&encoded)
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

    fn write_json_with_mode<T: Serialize>(
        &self,
        data_name: &OsStr,
        data_path: &Path,
        value: &T,
        write_mode: JsonWriteMode,
    ) -> Result<()> {
        match write_mode {
            JsonWriteMode::Cache => self.write_json(data_name, data_path, value),
            JsonWriteMode::Durable => self.write_json_durable(data_name, data_path, value),
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
        read_bounded_json(&mut file, data_path, cancelled).map(Some)
    }

    pub(in crate::runtime::loops) fn write_json_durable<T: Serialize>(
        &self,
        data_name: &OsStr,
        data_path: &Path,
        value: &T,
    ) -> Result<()> {
        let _ = self.exists(data_name, data_path)?;
        let tmp_name = temporary_file_name(data_name);
        let tmp_path = data_path.parent().unwrap_or(data_path).join(&tmp_name);
        let encoded = encode_bounded_json(value, data_path)?;
        let result = publish_durable_json(
            data_path,
            || {
                let mut tmp = open_regular_file(
                    &self.directory,
                    tmp_name.as_os_str(),
                    true,
                    true,
                    true,
                    &tmp_path,
                )?;
                tmp.write_all(&encoded)
                    .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
                tmp.sync_all()
                    .with_context(|| format!("Failed to sync {}", tmp_path.display()))?;
                drop(tmp);
                Ok(())
            },
            || {
                self.directory
                    .rename(&tmp_name, &self.directory, data_name)
                    .with_context(|| {
                        format!(
                            "Failed to replace loop state file {} with {}",
                            data_path.display(),
                            tmp_path.display()
                        )
                    })
            },
            || self.sync_durable_json_publication(data_name, data_path),
        );
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
        let orphan = temp.path().join("attempts.json.tmp-ExampleOrphan");
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
        let unrelated_directory = temp.path().join("attempts.json.tmp-ExampleDirectory");
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
    fn locked_cache_access_does_not_reclaim_a_sibling_files_temps() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("schedule.json");
        let lock_path = temp.path().join("schedule.lock");
        let own_orphan = temp.path().join("schedule.json.tmp-ExampleOrphan");
        let sibling_orphan = temp.path().join("schedule.initialized.tmp-ExampleOrphan");
        fs::write(&own_orphan, b"partial").unwrap();
        fs::write(&sibling_orphan, b"sibling").unwrap();
        with_json_cache_lock::<_, BTreeMap<String, String>>(
            temp.path(),
            temp.path(),
            &lock_path,
            &data_path,
            |_| Ok(()),
        )
        .unwrap();
        assert!(!own_orphan.exists());
        assert_eq!(fs::read(sibling_orphan).unwrap(), b"sibling");
    }

    #[cfg(unix)]
    #[test]
    fn locked_cache_refuses_a_symlinked_lock_file() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let target = outside.path().join("outside.lock");
        fs::write(&target, b"outside").unwrap();
        let lock_path = temp.path().join("attempts.lock");
        let data_path = temp.path().join("attempts.json");
        symlink(&target, &lock_path).unwrap();

        let error = with_json_cache_lock::<_, BTreeMap<String, String>>(
            temp.path(),
            temp.path(),
            &lock_path,
            &data_path,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("without following links"));
        assert_eq!(fs::read(&target).unwrap(), b"outside");
        assert!(!data_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn locked_cache_refuses_a_symlinked_directory_component() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), temp.path().join("loop")).unwrap();
        let directory = temp.path().join("loop");
        let lock_path = directory.join("attempts.lock");
        let data_path = directory.join("attempts.json");

        let error = with_json_cache_lock::<_, BTreeMap<String, String>>(
            temp.path(),
            &directory,
            &lock_path,
            &data_path,
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("without following links"));
        assert!(!outside.path().join("attempts.lock").exists());
        assert!(!outside.path().join("attempts.json").exists());
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
                .starts_with("attempts.json.tmp-")
        }));
    }

    #[test]
    fn compensating_cache_retains_commit_when_receipt_may_have_landed() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        let location = JsonLocation::new(
            temp.path().to_path_buf(),
            temp.path().to_path_buf(),
            "attempts",
            JsonWriteMode::Cache,
        );

        let error = with_json_cache_lock_compensating_until(
            &location,
            loop_state_lock_deadline(),
            &|| false,
            |state: &mut BTreeMap<String, String>| {
                state.insert("ExampleProject".into(), "cleared".into());
                Ok(())
            },
            |_, _| -> Result<()> { Err(crate::state::receipt_append_may_have_landed_for_test()) },
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

    include!("json_cache/durable_tests.rs");
}
