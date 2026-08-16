use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use anyhow::bail;
use anyhow::{Context, Result};

/// Returns an absolute path with only known platform root aliases resolved.
///
/// This is deliberately narrower than canonicalizing the complete path: the
/// caller still needs to reject symlinks in every user-controlled component.
pub(crate) fn physical_path(path: &Path, purpose: &str) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .with_context(|| format!("failed to resolve current directory for {purpose}"))?
            .join(path)
    };
    resolve_platform_root_alias(absolute, purpose)
}

#[cfg(target_os = "macos")]
fn resolve_platform_root_alias(path: PathBuf, purpose: &str) -> Result<PathBuf> {
    for (alias, expected) in [
        (Path::new("/var"), Path::new("/private/var")),
        (Path::new("/tmp"), Path::new("/private/tmp")),
        (Path::new("/etc"), Path::new("/private/etc")),
    ] {
        let Ok(suffix) = path.strip_prefix(alias) else {
            continue;
        };
        let physical = std::fs::canonicalize(alias).with_context(|| {
            format!(
                "failed to resolve macOS system path alias {} for {purpose}",
                alias.display()
            )
        })?;
        return resolve_verified_alias(&path, suffix, alias, expected, &physical, purpose);
    }
    Ok(path)
}

#[cfg(target_os = "macos")]
fn resolve_verified_alias(
    path: &Path,
    suffix: &Path,
    alias: &Path,
    expected: &Path,
    physical: &Path,
    purpose: &str,
) -> Result<PathBuf> {
    if physical == alias {
        return Ok(path.to_path_buf());
    }
    if physical != expected {
        bail!(
            "refusing unexpected macOS system path alias {} -> {} for {purpose}",
            alias.display(),
            physical.display()
        );
    }
    Ok(physical.join(suffix))
}

#[cfg(not(target_os = "macos"))]
fn resolve_platform_root_alias(path: PathBuf, _purpose: &str) -> Result<PathBuf> {
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_relative_paths_absolute() {
        let resolved = physical_path(Path::new("relative/vault"), "test").unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("relative/vault"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolves_only_verified_macos_system_root_aliases() {
        assert_eq!(
            physical_path(Path::new("/var/folders/example/output"), "test").unwrap(),
            Path::new("/private/var/folders/example/output")
        );
        assert_eq!(
            physical_path(Path::new("/tmp/output"), "test").unwrap(),
            Path::new("/private/tmp/output")
        );
        assert_eq!(
            physical_path(Path::new("/etc/hosts"), "test").unwrap(),
            Path::new("/private/etc/hosts")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_real_system_root_and_rejects_unexpected_redirect() {
        let path = Path::new("/tmp/vault");
        let alias = Path::new("/tmp");
        let expected = Path::new("/private/tmp");
        let suffix = Path::new("vault");

        assert_eq!(
            resolve_verified_alias(path, suffix, alias, expected, alias, "test").unwrap(),
            path
        );

        let unexpected = Path::new("/unexpected/tmp");
        let error =
            resolve_verified_alias(path, suffix, alias, expected, unexpected, "test").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unexpected macOS system path alias")
        );
    }
}
