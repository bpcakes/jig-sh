use super::*;

/// Read a native policy through the same directory capability and budget as
/// source authority. Failed and raced reads still consume their observed bytes.
pub(crate) fn read_native_authority_bytes(
    root: &Path,
    path: &str,
    maximum: usize,
    budget: &mut CollectionBudget<'_>,
) -> CollectionResult<Option<Vec<u8>>> {
    let root = open_root(root)?;
    let mut directory = root.try_clone().map_err(|_| failed(path))?;
    let mut authority = DirectoryAuthority::default();
    let mut prefix = String::new();
    let mut components = path.split('/').peekable();
    while let Some(name) = components.next() {
        if name.is_empty() || name == "." || name == ".." {
            return Err(failed(path));
        }
        authority.observe(&root, if prefix.is_empty() { "." } else { &prefix }, budget)?;
        budget.entries(1)?;
        let metadata = match directory.symlink_metadata(name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                authority.revalidate(&root, budget)?;
                return Ok(None);
            }
            Err(_) => return Err(failed(path)),
        };
        let is_directory = components.peek().is_some();
        if metadata.file_type().is_symlink()
            || (is_directory && !metadata.is_dir())
            || (!is_directory && !metadata.is_file())
        {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::UnobservableInput,
                "native policy traverses a symlink or unexpected file type",
            )
            .at(path));
        }
        let mut file = directory
            .open_with(name, &read_options(is_directory))
            .map_err(|_| raced(path))?;
        if signature(&metadata) != signature(&file.metadata().map_err(|_| raced(path))?) {
            return Err(raced(path));
        }
        if is_directory {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(name);
            directory = Dir::from_std_file(file.into_std());
            continue;
        }
        if metadata.len() > maximum as u64 {
            return Err(CollectionFailure::new(
                FreshnessReasonCode::CollectionLimit,
                "native policy exceeds its bounded preparation limit",
            )
            .at(path));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        let mut buffer = [0; 8192];
        loop {
            budget.ensure_active()?;
            let remaining = budget
                .limits
                .bytes
                .saturating_sub(budget.stats.content_bytes_read)
                .min(maximum.saturating_sub(bytes.len()) as u64);
            let capacity = buffer.len().min(remaining.saturating_add(1) as usize);
            let count = file
                .read(&mut buffer[..capacity])
                .map_err(|_| failed(path))?;
            budget.bytes(count as u64)?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > maximum {
                return Err(raced(path));
            }
        }
        if bytes.len() as u64 != metadata.len()
            || signature(&metadata) != signature(&file.metadata().map_err(|_| raced(path))?)
            || signature(&metadata)
                != signature(&directory.symlink_metadata(name).map_err(|_| raced(path))?)
        {
            return Err(raced(path));
        }
        authority.revalidate(&root, budget)?;
        return Ok(Some(bytes));
    }
    Err(failed(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::freshness::CollectionLimits;
    use std::time::Duration;

    #[test]
    fn native_policy_reads_charge_partial_failure_and_preserve_absence() {
        let temp = tempfile::tempdir().unwrap();
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &|| false,
        );
        assert!(
            read_native_authority_bytes(temp.path(), ".jig/file-budget.toml", 100, &mut budget)
                .unwrap()
                .is_none()
        );
        std::fs::create_dir(temp.path().join(".jig")).unwrap();
        std::fs::write(temp.path().join(".jig/file-budget.toml"), [b'x'; 64]).unwrap();
        budget.limits.bytes = 10;
        let failure =
            read_native_authority_bytes(temp.path(), ".jig/file-budget.toml", 100, &mut budget)
                .unwrap_err();
        assert_eq!(failure.reason.code, FreshnessReasonCode::CollectionLimit);
        assert_eq!(budget.stats.content_bytes_read, 11);
    }

    #[cfg(unix)]
    #[test]
    fn native_policy_never_reads_through_a_symlinked_parent() {
        let temp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("file-budget.toml"), "Example policy").unwrap();
        std::os::unix::fs::symlink(other.path(), temp.path().join(".jig")).unwrap();
        let mut budget = CollectionBudget::new(
            CollectionLimits::with_timeout(Duration::from_secs(30)),
            &|| false,
        );
        let failure =
            read_native_authority_bytes(temp.path(), ".jig/file-budget.toml", 100, &mut budget)
                .unwrap_err();
        assert_eq!(failure.reason.code, FreshnessReasonCode::UnobservableInput);
        assert_eq!(budget.stats.content_bytes_read, 0);
    }
}
