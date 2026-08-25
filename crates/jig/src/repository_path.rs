use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

pub(crate) fn normalize_repo_relative_path(path: &Path, label: &str) -> Result<PathBuf> {
    if path.is_absolute() {
        bail!("{label} must be repository-relative: {}", path.display());
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!(
                    "{label} must stay inside the repository: {}",
                    path.display()
                )
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        bail!("{label} must not be empty");
    }

    Ok(normalized)
}

pub(crate) fn normalize_portable_repo_path(value: &str, label: &str) -> Result<String> {
    let bytes = value.as_bytes();
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    if value.starts_with('/')
        || value.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    {
        bail!("{label} must be a portable repository-relative path: {value}");
    }
    if value.contains('\\') {
        bail!("{label} must use portable '/' separators and stay repository-relative: {value}");
    }
    if value.chars().any(char::is_control) {
        bail!("{label} must not contain control characters: {value:?}");
    }

    let mut normalized = Vec::new();
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                bail!("{label} must not contain '..' and must stay inside the repository: {value}")
            }
            component => normalized.push(component),
        }
    }
    if normalized.is_empty() {
        Ok(".".into())
    } else {
        Ok(normalized.join("/"))
    }
}

pub(crate) fn normalize_portable_repository_directory(value: &str, label: &str) -> Result<String> {
    let normalized = normalize_portable_repo_path(value, label)?;
    if normalized == "." {
        bail!("{label} must identify a directory below the repository root: {value}");
    }
    Ok(normalized)
}

pub(crate) fn resolve_repository_working_directory(
    root: &Path,
    configured: Option<&str>,
) -> Result<PathBuf> {
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve repository root {}", root.display()))?;
    let Some(configured) = configured else {
        return Ok(canonical_root);
    };
    if configured == "." {
        return Ok(canonical_root);
    }
    let configured_path = normalize_repo_relative_path(Path::new(configured), "working_directory")
        .context("working_directory must be a non-empty relative repository path")?;
    let candidate = root
        .join(&configured_path)
        .canonicalize()
        .with_context(|| {
            format!(
                "failed to resolve configured path {}",
                root.join(&configured_path).display()
            )
        })?;
    if !candidate.starts_with(&canonical_root) {
        bail!("working_directory resolves outside the repository");
    }
    if !candidate.is_dir() {
        bail!("working_directory is not a directory");
    }
    Ok(candidate)
}

pub(crate) fn validate_runner_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) {
            bail!("environment variable name {name:?} is invalid");
        }
        if value.contains('\0') {
            bail!("environment variable {name:?} contains a NUL byte");
        }
    }
    Ok(())
}

/// Validates a repository-relative directory immediately before a caller may
/// create entries below it. Existing components must be real directories so a
/// configured path cannot redirect writes through a symlink.
pub(crate) fn validate_repository_directory_path(root: &Path, relative: &Path) -> Result<()> {
    let relative = normalize_repo_relative_path(relative, "repository directory")?;
    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("Failed to stat repository root {}", root.display()))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        bail!(
            "Repository root must be a real directory, not a symlink or non-directory: {}",
            root.display()
        );
    }

    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            unreachable!("repository-relative path was normalized above");
        };
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to stat {}", current.display()));
            }
        };
        if metadata.file_type().is_symlink() {
            bail!(
                "Unsafe repository directory {}: component {} is a symlink",
                relative.display(),
                current.display()
            );
        }
        if !metadata.is_dir() {
            bail!(
                "Unsafe repository directory {}: component {} is not a directory",
                relative.display(),
                current.display()
            );
        }
    }
    Ok(())
}
