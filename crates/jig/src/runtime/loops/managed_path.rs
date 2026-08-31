use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};

pub(super) fn ensure_managed_directory(
    root: &Path,
    directory: &Path,
    description: &str,
) -> Result<()> {
    walk_managed_directory(root, directory, description, true).map(|_| ())
}

pub(super) fn inspect_managed_directory(
    root: &Path,
    directory: &Path,
    description: &str,
) -> Result<bool> {
    walk_managed_directory(root, directory, description, false)
}

pub(super) fn inspect_managed_file(root: &Path, path: &Path, description: &str) -> Result<bool> {
    let parent = path
        .parent()
        .with_context(|| format!("{description} has no parent: {}", path.display()))?;
    if !inspect_managed_directory(root, parent, description)? {
        return Ok(false);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("Managed {description} is a symlink: {}", path.display())
        }
        Ok(_) => bail!(
            "Managed {description} is not a regular file: {}",
            path.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to inspect managed {description} {}", path.display())),
    }
}

fn walk_managed_directory(
    root: &Path,
    directory: &Path,
    description: &str,
    create: bool,
) -> Result<bool> {
    let relative = directory.strip_prefix(root).with_context(|| {
        format!(
            "Managed {description} path {} is outside trusted root {}",
            directory.display(),
            root.display()
        )
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!(
            "Managed {description} path is not a contained path below {}: {}",
            root.display(),
            directory.display()
        );
    }
    require_real_directory(root, description)?;

    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "Managed {description} directory component is a symlink: {}",
                current.display()
            ),
            Ok(_) => bail!(
                "Managed {description} directory component is not a directory: {}",
                current.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound && !create => return Ok(false),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let parent = current.parent().with_context(|| {
                    format!(
                        "Managed {description} directory has no parent: {}",
                        current.display()
                    )
                })?;
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!(
                                "Failed to create managed {description} {}",
                                current.display()
                            )
                        });
                    }
                }
                require_real_directory(&current, description)?;
                sync_directory(parent, description)?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to inspect managed {description} {}",
                        current.display()
                    )
                });
            }
        }
    }
    Ok(true)
}

fn require_real_directory(path: &Path, description: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect managed {description} {}", path.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!(
            "Managed {description} root is not a real directory: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path, description: &str) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .with_context(|| {
            format!(
                "Failed to sync managed {description} directory {}",
                path.display()
            )
        })
}

#[cfg(not(unix))]
fn sync_directory(_: &Path, _: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_missing_managed_directory_chain() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join(".agent/runtime/loop");

        ensure_managed_directory(temp.path(), &nested, "loop runtime").unwrap();

        assert!(nested.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlinked_existing_ancestor() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let redirected = tempfile::tempdir().unwrap();
        fs::create_dir_all(redirected.path().join("runtime/loop")).unwrap();
        symlink(redirected.path(), temp.path().join(".agent")).unwrap();

        let error = inspect_managed_directory(
            temp.path(),
            &temp.path().join(".agent/runtime/loop"),
            "loop runtime",
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("component is a symlink"),
            "{error:#}"
        );
    }
}
