use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Error;

use crate::{Result, SecretBytes, VaultError, VaultErrorKind};

/// A fully written and synced private sibling file awaiting atomic install.
///
/// Preparation does not change the destination. Dropping an uninstalled value
/// removes only the generated same-directory temporary file.
pub struct PreparedPrivateFile {
    inner: PreparedPrivateOutput,
    destination: PathBuf,
    overwrite: bool,
    byte_len: usize,
}

impl PreparedPrivateFile {
    /// Validates a destination and overwrite policy without creating a file.
    ///
    /// This is suitable for dry-run reporting. It checks the same supported
    /// platform, parent, ancestor, and current leaf conditions as preparation,
    /// but installation still rechecks them to close ordinary races.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be hardened or the current
    /// leaf conflicts with `overwrite`.
    pub fn preflight(destination: &Path, overwrite: bool) -> Result<()> {
        preflight_private_destination(destination, overwrite).map_err(output_failure_to_vault_error)
    }

    /// Writes and syncs an owner-only temporary file beside `destination`.
    ///
    /// # Errors
    ///
    /// Returns an error when path hardening, overwrite preflight, private
    /// creation, writing, or syncing fails. Platforms without the required
    /// guarantees reject preparation.
    pub fn prepare(destination: &Path, contents: SecretBytes, overwrite: bool) -> Result<Self> {
        let byte_len = contents.len();
        let inner = prepare_private_bytes(destination, contents.as_slice(), overwrite)
            .map_err(output_failure_to_vault_error)?;
        Ok(Self {
            inner,
            destination: destination.to_path_buf(),
            overwrite,
            byte_len,
        })
    }

    /// Atomically installs the prepared file according to its overwrite policy.
    ///
    /// # Errors
    ///
    /// Returns an error when close-to-install path checks or the atomic
    /// namespace operation fail.
    pub fn install(self) -> Result<()> {
        install_prepared_private(self.inner).map_err(output_failure_to_vault_error)
    }
}

impl fmt::Debug for PreparedPrivateFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedPrivateFile")
            .field("destination", &self.destination)
            .field("overwrite", &self.overwrite)
            .field("byte_len", &self.byte_len)
            .field("contents", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFailureStage {
    Preflight,
    #[cfg(unix)]
    Sink,
}

impl OutputFailureStage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "sink_preflight",
            #[cfg(unix)]
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

fn output_failure_to_vault_error(failure: OutputInstallFailure) -> VaultError {
    VaultError::from_anyhow(failure.kind, failure.error)
}

#[cfg(unix)]
pub(crate) fn install_private_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    let prepared = prepare_private_bytes(path, bytes, overwrite)?;
    install_prepared_private(prepared)
}

#[cfg(not(unix))]
pub(crate) fn install_private_bytes(
    path: &Path,
    _bytes: &[u8],
    _overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(unix)]
type PreparedPrivateOutput = unix::PreparedPrivateOutput;

#[cfg(unix)]
fn prepare_private_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> std::result::Result<PreparedPrivateOutput, OutputInstallFailure> {
    unix::prepare_private_bytes(path, bytes, overwrite)
}

#[cfg(unix)]
fn install_prepared_private(
    prepared: PreparedPrivateOutput,
) -> std::result::Result<(), OutputInstallFailure> {
    prepared.install()
}

#[cfg(unix)]
fn preflight_private_destination(
    path: &Path,
    overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    unix::preflight_private_destination(path, overwrite)
}

#[cfg(not(unix))]
struct PreparedPrivateOutput;

#[cfg(not(unix))]
fn prepare_private_bytes(
    path: &Path,
    _bytes: &[u8],
    _overwrite: bool,
) -> std::result::Result<PreparedPrivateOutput, OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(not(unix))]
fn install_prepared_private(
    _prepared: PreparedPrivateOutput,
) -> std::result::Result<(), OutputInstallFailure> {
    unreachable!("unsupported platforms cannot construct prepared private output")
}

#[cfg(not(unix))]
fn preflight_private_destination(
    path: &Path,
    _overwrite: bool,
) -> std::result::Result<(), OutputInstallFailure> {
    Err(unsupported_private_output(path))
}

#[cfg(not(unix))]
fn unsupported_private_output(path: &Path) -> OutputInstallFailure {
    OutputInstallFailure {
        stage: OutputFailureStage::Preflight,
        kind: VaultErrorKind::InvalidInput,
        error: anyhow::anyhow!(
            "private vault output to {} is unsupported on this platform because Jig cannot guarantee owner-only ACLs, reparse-point refusal, and atomic no-clobber installation",
            path.display()
        ),
    }
}

#[cfg(unix)]
mod unix {
    use std::ffi::OsString;
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use anyhow::{Context, anyhow, bail};

    use super::{OutputFailureStage, OutputInstallFailure};
    use crate::VaultErrorKind;

    pub(super) fn preflight_private_destination(
        path: &Path,
        overwrite: bool,
    ) -> Result<(), OutputInstallFailure> {
        preflight(path, overwrite)
            .map(|_| ())
            .map_err(|error| OutputInstallFailure {
                stage: OutputFailureStage::Preflight,
                kind: preflight_error_kind(&error),
                error,
            })
    }

    pub(super) fn prepare_private_bytes(
        path: &Path,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<PreparedPrivateOutput, OutputInstallFailure> {
        let mut path = preflight(path, overwrite).map_err(|error| OutputInstallFailure {
            stage: OutputFailureStage::Preflight,
            kind: preflight_error_kind(&error),
            error,
        })?;
        path.temporary_identity = match write_temporary(&path, bytes) {
            Ok(identity) => Some(identity),
            Err(error) => {
                // The create-new file is still owned by this operation here;
                // no path-based handoff has occurred yet.
                let _ = fs::remove_file(&path.temporary);
                return Err(OutputInstallFailure {
                    stage: OutputFailureStage::Sink,
                    kind: install_error_kind(&error),
                    error,
                });
            }
        };
        Ok(PreparedPrivateOutput {
            path: Some(path),
            overwrite,
        })
    }

    pub(super) struct PreparedPrivateOutput {
        path: Option<PreparedPath>,
        overwrite: bool,
    }

    impl PreparedPrivateOutput {
        pub(super) fn install(mut self) -> Result<(), OutputInstallFailure> {
            let path = self
                .path
                .take()
                .expect("prepared private output installs at most once");
            install(path, self.overwrite).map_err(|error| OutputInstallFailure {
                stage: OutputFailureStage::Sink,
                kind: install_error_kind(&error),
                error,
            })
        }
    }

    impl Drop for PreparedPrivateOutput {
        fn drop(&mut self) {
            if let Some(path) = self.path.take() {
                let _ = remove_temporary_if_identity_matches(&path);
            }
        }
    }

    struct PreparedPath {
        destination: PathBuf,
        parent: PathBuf,
        temporary: PathBuf,
        temporary_identity: Option<FileIdentity>,
    }

    #[derive(Clone, Copy)]
    struct FileIdentity {
        device: u64,
        inode: u64,
        owner: u32,
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
        let parent_mode = parent_metadata.permissions().mode();
        if parent_mode & 0o022 != 0 && parent_mode & 0o1000 == 0 {
            bail!(
                "refusing private vault output in shared-writable non-sticky parent {}",
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
            temporary_identity: None,
        })
    }

    fn write_temporary(prepared: &PreparedPath, bytes: &[u8]) -> anyhow::Result<FileIdentity> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&prepared.temporary)
            .with_context(|| {
                format!(
                    "failed to create private output temporary file beside {}",
                    prepared.destination.display()
                )
            })?;
        file.write_all(bytes).with_context(|| {
            format!(
                "failed to write private vault output {}",
                prepared.destination.display()
            )
        })?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| {
                format!(
                    "failed to restrict private vault output permissions for {}",
                    prepared.destination.display()
                )
            })?;
        let metadata = file.metadata().with_context(|| {
            format!(
                "failed to inspect private vault output temporary file for {}",
                prepared.destination.display()
            )
        })?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
        {
            bail!(
                "private vault output temporary file is not an owner-only regular file for {}",
                prepared.destination.display()
            );
        }
        file.sync_all().with_context(|| {
            format!(
                "failed to sync private vault output {}",
                prepared.destination.display()
            )
        })?;
        Ok(FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner: metadata.uid(),
        })
    }

    fn install(prepared: PreparedPath, overwrite: bool) -> anyhow::Result<()> {
        let PreparedPath {
            destination,
            parent,
            temporary,
            temporary_identity,
        } = prepared;
        let temporary_identity = temporary_identity
            .expect("prepared private output records its temporary file identity");
        let result = (|| -> anyhow::Result<()> {
            // Recheck immediately before the atomic namespace operation. This
            // narrows same-user directory-entry races without claiming an OS
            // isolation boundary stronger than the containing directory.
            reject_symlinked_ancestors(&parent)?;
            inspect_destination(&destination, overwrite)?;
            validate_temporary_identity(&temporary, temporary_identity)?;
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
            // Remove only the exact inode this operation created. A replaced
            // directory entry is never deleted during error cleanup.
            let cleanup_path = PreparedPath {
                destination,
                parent,
                temporary,
                temporary_identity: Some(temporary_identity),
            };
            let _ = remove_temporary_if_identity_matches(&cleanup_path);
        }
        result
    }

    fn validate_temporary_identity(path: &Path, expected: FileIdentity) -> anyhow::Result<()> {
        let metadata = fs::symlink_metadata(path).with_context(|| {
            format!(
                "failed to revalidate private output temporary file {}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.dev() != expected.device
            || metadata.ino() != expected.inode
            || metadata.uid() != expected.owner
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!(
                "private output temporary file identity changed before installation for {}",
                path.display()
            );
        }
        Ok(())
    }

    fn remove_temporary_if_identity_matches(prepared: &PreparedPath) -> anyhow::Result<()> {
        let Some(identity) = prepared.temporary_identity else {
            return Ok(());
        };
        match fs::symlink_metadata(&prepared.temporary) {
            Ok(_) => validate_temporary_identity(&prepared.temporary, identity)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect private output temporary file {} during cleanup",
                        prepared.temporary.display()
                    )
                });
            }
        }
        fs::remove_file(&prepared.temporary).with_context(|| {
            format!(
                "failed to remove private output temporary file {}",
                prepared.temporary.display()
            )
        })
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
        if error.to_string().contains("already exists")
            || error
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

        fn private_tempdir() -> tempfile::TempDir {
            let temp = tempfile::tempdir().unwrap();
            fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
            temp
        }

        fn install_private_bytes(
            path: &Path,
            bytes: &[u8],
            overwrite: bool,
        ) -> std::result::Result<(), OutputInstallFailure> {
            prepare_private_bytes(path, bytes, overwrite)?.install()
        }

        #[test]
        fn installs_owner_only_bytes_without_clobbering() {
            let temp = private_tempdir();
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
        fn public_prepared_file_is_private_redacted_and_installs_only_when_consumed() {
            let temp = private_tempdir();
            let output = temp.path().join("import.env");
            let contents = b"TOKEN=jig://Production/TOKEN\n";
            let prepared = crate::PreparedPrivateFile::prepare(
                &output,
                crate::SecretBytes::new(contents.to_vec()),
                false,
            )
            .unwrap();
            assert!(!output.exists());
            let debug = format!("{prepared:?}");
            assert!(debug.contains("[REDACTED]"));
            assert!(!debug.contains("jig://Production/TOKEN"));

            let temporary = fs::read_dir(temp.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .next()
                .unwrap();
            assert_eq!(fs::read(&temporary).unwrap(), contents);
            assert_eq!(
                fs::metadata(&temporary).unwrap().permissions().mode() & 0o777,
                0o600
            );

            prepared.install().unwrap();
            assert_eq!(fs::read(&output).unwrap(), contents);
            assert_eq!(
                fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        }

        #[test]
        fn public_preflight_is_non_writing_and_applies_overwrite_policy() {
            let temp = private_tempdir();
            let output = temp.path().join("import.env");
            crate::PreparedPrivateFile::preflight(&output, false).unwrap();
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);

            fs::write(&output, b"existing").unwrap();
            let collision = crate::PreparedPrivateFile::preflight(&output, false).unwrap_err();
            assert_eq!(collision.kind(), VaultErrorKind::AlreadyExists);
            crate::PreparedPrivateFile::preflight(&output, true).unwrap();
            assert_eq!(fs::read(&output).unwrap(), b"existing");
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        }

        #[test]
        fn prepared_no_clobber_rechecks_destination_and_cleans_its_temporary() {
            let temp = private_tempdir();
            let output = temp.path().join("import.env");
            let prepared = crate::PreparedPrivateFile::prepare(
                &output,
                crate::SecretBytes::new(b"prepared".to_vec()),
                false,
            )
            .unwrap();
            fs::write(&output, b"raced-existing").unwrap();

            let error = prepared.install().unwrap_err();
            assert_eq!(error.kind(), VaultErrorKind::AlreadyExists);
            assert_eq!(fs::read(&output).unwrap(), b"raced-existing");
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        }

        #[test]
        fn prepared_install_refuses_replaced_temporary_identity_for_both_policies() {
            for overwrite in [false, true] {
                let temp = private_tempdir();
                let output = temp.path().join("result.bin");
                if overwrite {
                    fs::write(&output, b"original").unwrap();
                }
                let attacker = temp.path().join("attacker");
                fs::write(&attacker, b"attacker-bytes").unwrap();
                let prepared = prepare_private_bytes(&output, b"protected", overwrite).unwrap();
                let temporary = prepared.path.as_ref().unwrap().temporary.clone();
                fs::remove_file(&temporary).unwrap();
                symlink(&attacker, &temporary).unwrap();

                let error = prepared.install().unwrap_err();
                assert!(error.error.to_string().contains("identity changed"));
                assert_eq!(fs::read(&attacker).unwrap(), b"attacker-bytes");
                if overwrite {
                    assert_eq!(fs::read(&output).unwrap(), b"original");
                } else {
                    assert!(!output.exists());
                }
                // Identity-safe cleanup deliberately leaves the replacement.
                assert!(
                    fs::symlink_metadata(&temporary)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
                fs::remove_file(temporary).unwrap();
            }
        }

        #[test]
        fn rejects_shared_writable_non_sticky_output_parent() {
            let temp = private_tempdir();
            fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o777)).unwrap();
            let output = temp.path().join("result.bin");

            let error = preflight_private_destination(&output, false).unwrap_err();
            assert!(
                error
                    .error
                    .to_string()
                    .contains("shared-writable non-sticky")
            );
            assert!(!output.exists());
            assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
        }

        #[test]
        fn refuses_symlinked_parent_and_leaf() {
            let temp = private_tempdir();
            let real = temp.path().join("real");
            fs::create_dir(&real).unwrap();
            fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
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
            let temp = private_tempdir();
            let output = temp.path().join("directory");
            fs::create_dir(&output).unwrap();
            let error = install_private_bytes(&output, b"secret", true).unwrap_err();
            assert!(error.error.to_string().contains("non-regular"));
            assert!(output.is_dir());
        }
    }
}
