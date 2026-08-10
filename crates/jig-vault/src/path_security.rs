use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

/// Returns whether a symlink is an OS-managed alias outside an unprivileged
/// caller's namespace control.
pub(crate) fn is_trusted_root_alias(path: &Path, metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        // macOS exposes system paths such as /var through root-owned aliases.
        // Restrict the exception to aliases directly beneath a root-owned,
        // non-writable `/`; user-controlled and nested symlinks remain denied.
        path.parent() == Some(Path::new("/"))
            && metadata.uid() == 0
            && fs::symlink_metadata("/").is_ok_and(|root| {
                root.is_dir() && root.uid() == 0 && root.permissions().mode() & 0o022 == 0
            })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, metadata);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::is_trusted_root_alias;

    #[cfg(unix)]
    #[test]
    fn rejects_user_controlled_aliases() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&target).unwrap();
        symlink(&target, &alias).unwrap();

        let metadata = std::fs::symlink_metadata(&alias).unwrap();
        assert!(!is_trusted_root_alias(&alias, &metadata));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_macos_var_alias() {
        let path = std::path::Path::new("/var");
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert!(metadata.file_type().is_symlink());
        assert!(is_trusted_root_alias(path, &metadata));
    }
}
