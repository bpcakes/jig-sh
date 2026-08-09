use std::path::Path;

use anyhow::Error;

use crate::VaultErrorKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFailureStage {
    Preflight,
    Sink,
}

impl OutputFailureStage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "sink_preflight",
            Self::Sink => "sink",
        }
    }
}

#[derive(Debug)]
pub(crate) struct OutputInstallFailure {
    pub(crate) stage: OutputFailureStage,
    pub(crate) kind: VaultErrorKind,
    pub(crate) error: Error,
}

#[cfg(unix)]
pub(crate) fn install_private_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<(), OutputInstallFailure> {
    unix::install_private_bytes(path, bytes, overwrite)
}

#[cfg(not(unix))]
pub(crate) fn install_private_bytes(
    path: &Path,
    _bytes: &[u8],
    _overwrite: bool,
) -> Result<(), OutputInstallFailure> {
    Err(OutputInstallFailure {
        stage: OutputFailureStage::Preflight,
        kind: VaultErrorKind::InvalidInput,
        error: anyhow::anyhow!(
            "private vault output to {} is unsupported on this platform because Jig cannot guarantee owner-only ACLs, reparse-point refusal, and atomic no-clobber installation",
            path.display()
        ),
    })
}

#[cfg(unix)]
mod unix {
    use std::ffi::OsString;
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use anyhow::{Context, anyhow, bail};

    use super::{OutputFailureStage, OutputInstallFailure};
    use crate::VaultErrorKind;

    pub(super) fn install_private_bytes(
        path: &Path,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<(), OutputInstallFailure> {
        let prepared = preflight(path, overwrite).map_err(|error| OutputInstallFailure {
            stage: OutputFailureStage::Preflight,
            kind: preflight_error_kind(&error),
            error,
        })?;
        install(prepared, bytes, overwrite).map_err(|error| OutputInstallFailure {
            stage: OutputFailureStage::Sink,
            kind: install_error_kind(&error),
            error,
        })
    }

    struct PreparedPath {
        destination: PathBuf,
        parent: PathBuf,
        temporary: PathBuf,
    }

    fn preflight(path: &Path, overwrite: bool) -> anyhow::Result<PreparedPath> {
        let file_name = path
            .file_name()
            .ok_or_else(|| anyhow!("vault output path must name a file: {}", path.display()))?;
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        reject_symlinked_ancestors(&parent)?;
        let parent_metadata = fs::metadata(&parent)
            .with_context(|| format!("failed to inspect output parent {}", parent.display()))?;
        if !parent_metadata.is_dir() {
            bail!(
                "vault output parent is not a directory: {}",
                parent.display()
            );
        }
        inspect_destination(path, overwrite)?;

        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(
            ".{}.{}.jig-vault-output.tmp",
            std::process::id(),
            ulid::Ulid::new()
        ));
        Ok(PreparedPath {
            destination: path.to_path_buf(),
            temporary: parent.join(temporary_name),
            parent,
        })
    }

    fn install(prepared: PreparedPath, bytes: &[u8], overwrite: bool) -> anyhow::Result<()> {
        let PreparedPath {
            destination,
            parent,
            temporary,
        } = prepared;
        let result = (|| -> anyhow::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)
                .with_context(|| {
                    format!(
                        "failed to create private output temporary file beside {}",
                        destination.display()
                    )
                })?;
            file.write_all(bytes).with_context(|| {
                format!(
                    "failed to write private vault output {}",
                    destination.display()
                )
            })?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .with_context(|| {
                    format!(
                        "failed to restrict private vault output permissions for {}",
                        destination.display()
                    )
                })?;
            let metadata = file.metadata().with_context(|| {
                format!(
                    "failed to inspect private vault output temporary file for {}",
                    destination.display()
                )
            })?;
            if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
                bail!(
                    "private vault output temporary file is not an owner-only regular file for {}",
                    destination.display()
                );
            }
            file.sync_all().with_context(|| {
                format!(
                    "failed to sync private vault output {}",
                    destination.display()
                )
            })?;
            drop(file);

            // Recheck immediately before the atomic namespace operation. This
            // narrows same-user directory-entry races without claiming an OS
            // isolation boundary stronger than the containing directory.
            reject_symlinked_ancestors(&parent)?;
            inspect_destination(&destination, overwrite)?;
            if overwrite {
                fs::rename(&temporary, &destination).with_context(|| {
                    format!(
                        "failed to atomically replace private vault output {}",
                        destination.display()
                    )
                })?;
            } else {
                // A same-directory hard link is an atomic no-replace install:
                // it fails if any directory entry already occupies the leaf.
                fs::hard_link(&temporary, &destination).with_context(|| {
                    format!(
                        "failed to atomically install private vault output without replacing {}",
                        destination.display()
                    )
                })?;
                fs::remove_file(&temporary).with_context(|| {
                    format!(
                        "private vault output was installed at {}, but its temporary link could not be removed",
                        destination.display()
                    )
                })?;
            }
            sync_parent(&parent)?;
            Ok(())
        })();

        if result.is_err() {
            // The path is an exact same-directory name generated above. This
            // cleanup never follows links and never targets user-selected
            // directories. Preserve the primary failure if cleanup also fails.
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn reject_symlinked_ancestors(path: &Path) -> anyhow::Result<()> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .context("failed to resolve current directory for vault output")?
                .join(path)
        };
        let mut ancestors = absolute.ancestors().collect::<Vec<_>>();
        ancestors.reverse();
        for ancestor in ancestors {
            let metadata = fs::symlink_metadata(ancestor).with_context(|| {
                format!(
                    "failed to inspect output path ancestor {}",
                    ancestor.display()
                )
            })?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "refusing private vault output through symlinked parent {}",
                    ancestor.display()
                );
            }
        }
        Ok(())
    }

    fn inspect_destination(path: &Path, overwrite: bool) -> anyhow::Result<()> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "refusing to write private vault output through symlink {}",
                path.display()
            ),
            Ok(_metadata) if !overwrite => Err(anyhow!(
                "private vault output already exists at {}; pass --overwrite to replace it",
                path.display()
            )),
            Ok(metadata) if !metadata.is_file() => bail!(
                "refusing to replace non-regular private vault output {}",
                path.display()
            ),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("failed to inspect vault output {}", path.display())),
        }
    }

    fn sync_parent(parent: &Path) -> anyhow::Result<()> {
        let directory = File::open(parent)
            .with_context(|| format!("failed to open output parent {}", parent.display()))?;
        directory
            .sync_all()
            .with_context(|| format!("failed to sync output parent {}", parent.display()))
    }

    fn preflight_error_kind(error: &anyhow::Error) -> VaultErrorKind {
        if error.to_string().contains("already exists") {
            VaultErrorKind::AlreadyExists
        } else if error.chain().any(|source| source.is::<std::io::Error>()) {
            VaultErrorKind::Io
        } else {
            VaultErrorKind::InvalidInput
        }
    }

    fn install_error_kind(error: &anyhow::Error) -> VaultErrorKind {
        if error
            .chain()
            .filter_map(|source| source.downcast_ref::<std::io::Error>())
            .any(|error| error.kind() == std::io::ErrorKind::AlreadyExists)
        {
            VaultErrorKind::AlreadyExists
        } else {
            VaultErrorKind::Io
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};

        #[test]
        fn installs_owner_only_bytes_without_clobbering() {
            let temp = tempfile::tempdir().unwrap();
            let output = temp.path().join("result.bin");
            install_private_bytes(&output, b"first\0bytes", false).unwrap();
            assert_eq!(fs::read(&output).unwrap(), b"first\0bytes");
            assert_eq!(
                fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o600
            );

            let error = install_private_bytes(&output, b"second", false).unwrap_err();
            assert!(error.error.to_string().contains("already exists"));
            assert_eq!(fs::read(&output).unwrap(), b"first\0bytes");

            install_private_bytes(&output, b"second", true).unwrap();
            assert_eq!(fs::read(&output).unwrap(), b"second");
            assert_eq!(
                fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        #[test]
        fn refuses_symlinked_parent_and_leaf() {
            let temp = tempfile::tempdir().unwrap();
            let real = temp.path().join("real");
            fs::create_dir(&real).unwrap();
            let linked_parent = temp.path().join("linked");
            symlink(&real, &linked_parent).unwrap();
            let error =
                install_private_bytes(&linked_parent.join("value"), b"secret", false).unwrap_err();
            assert!(error.error.to_string().contains("symlinked parent"));
            assert!(!real.join("value").exists());

            let target = real.join("target");
            fs::write(&target, b"unchanged").unwrap();
            let leaf = real.join("leaf");
            symlink(&target, &leaf).unwrap();
            let error = install_private_bytes(&leaf, b"secret", true).unwrap_err();
            assert!(error.error.to_string().contains("symlink"));
            assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        }

        #[test]
        fn overwrite_refuses_non_regular_leaf() {
            let temp = tempfile::tempdir().unwrap();
            let output = temp.path().join("directory");
            fs::create_dir(&output).unwrap();
            let error = install_private_bytes(&output, b"secret", true).unwrap_err();
            assert!(error.error.to_string().contains("non-regular"));
            assert!(output.is_dir());
        }
    }
}
