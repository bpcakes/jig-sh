use super::*;

pub(super) fn with_json_cache_lock<T, S>(
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
    action: impl FnOnce(&mut S) -> Result<T>,
) -> Result<T>
where
    S: Default + DeserializeOwned + Serialize,
{
    with_exclusive_file_lock(dir, lock_path, || {
        reclaim_orphaned_json_cache_temps(data_path)?;
        let mut store = read_json_or_default(data_path)?;
        let result = action(&mut store)?;
        write_json(data_path, &store)?;
        Ok(result)
    })
}

pub(super) fn validate_json_cache<T>(dir: &Path, lock_path: &Path, data_path: &Path) -> Result<()>
where
    T: Default + DeserializeOwned,
{
    with_exclusive_file_lock(dir, lock_path, || {
        reclaim_orphaned_json_cache_temps(data_path)?;
        read_json_or_default::<T>(data_path).map(|_| ())
    })
}

pub(super) fn recover_unparsable_json_cache<T>(
    dir: &Path,
    lock_path: &Path,
    data_path: &Path,
) -> Result<bool>
where
    T: Default + DeserializeOwned + Serialize,
{
    with_exclusive_file_lock(dir, lock_path, || {
        reclaim_orphaned_json_cache_temps(data_path)?;
        match read_json_or_default::<T>(data_path) {
            Ok(_) => Ok(false),
            Err(error) if error.downcast_ref::<serde_json::Error>().is_some() => {
                write_json(data_path, &T::default())?;
                Ok(true)
            }
            Err(error) => Err(error),
        }
    })
}

fn reclaim_orphaned_json_cache_temps(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Loop state path has no parent: {}", path.display()))?;
    let prefix_path = path.with_extension("tmp-");
    let prefix = prefix_path
        .file_name()
        .ok_or_else(|| anyhow!("Loop state path has no file name: {}", path.display()))?;
    for entry in fs::read_dir(parent).with_context(|| {
        format!(
            "Failed to inspect loop cache directory {}",
            parent.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "Failed to inspect loop cache directory {}",
                parent.display()
            )
        })?;
        if entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(prefix.as_encoded_bytes())
        {
            fs::remove_file(entry.path()).with_context(|| {
                format!(
                    "Failed to reclaim loop cache temporary file {}",
                    entry.path().display()
                )
            })?;
        }
    }
    Ok(())
}

pub(super) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("Loop state path has no parent: {}", path.display()));
    };
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", Ulid::new()));
    let result = fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).context("Failed to encode loop state JSON")?,
    )
    .with_context(|| format!("Failed to write {}", tmp.display()))
    .and_then(|()| {
        fs::rename(&tmp, path).with_context(|| {
            format!(
                "Failed to replace loop cache file {} with {}",
                path.display(),
                tmp.display()
            )
        })
    });
    match result {
        Ok(()) => Ok(()),
        Err(error) => match fs::remove_file(&tmp) {
            Ok(()) => Err(error),
            Err(cleanup_error) if cleanup_error.kind() == std::io::ErrorKind::NotFound => {
                Err(error)
            }
            Err(cleanup_error) => Err(error.context(format!(
                "Failed to remove temporary loop cache file {}: {cleanup_error}",
                tmp.display()
            ))),
        },
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
        let orphan = temp.path().join("attempts.tmp-ExampleOrphan");
        fs::write(&orphan, b"partial").unwrap();

        with_json_cache_lock::<_, BTreeMap<String, String>>(
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
    fn failed_cache_replacement_removes_its_temp_file() {
        let temp = tempdir().unwrap();
        let data_path = temp.path().join("attempts.json");
        fs::create_dir(&data_path).unwrap();

        let error = write_json(&data_path, &BTreeMap::<String, String>::new()).unwrap_err();

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
}
