use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result as AnyResult, bail};
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::error::{classified, classify_source};
use crate::store::VaultStore;
use crate::{VaultError, VaultErrorKind};

use super::payload::DecodedBackupArchive;
use super::{BackupRestoreResult, MAX_BACKUP_ARCHIVE_BYTES, RestoreTarget};

const VAULT_FILE: &str = "vault.json";
const AUDIT_FILE: &str = "audit.jsonl";
const LOCK_FILE: &str = "vault.lock";

pub(super) fn read_archive(path: &Path) -> AnyResult<Zeroizing<Vec<u8>>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("failed to open backup archive {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect backup archive {}", path.display()))?;
    if !metadata.is_file() {
        bail!("backup input must be a regular non-symlink file");
    }
    if metadata.len() > MAX_BACKUP_ARCHIVE_BYTES as u64 {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!(
                "backup archive is {} bytes, exceeding the {MAX_BACKUP_ARCHIVE_BYTES} byte read limit",
                metadata.len()
            ),
        ));
    }
    let capacity =
        usize::try_from(metadata.len()).context("backup archive length exceeds address space")?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    Read::by_ref(&mut file)
        .take(MAX_BACKUP_ARCHIVE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read backup archive {}", path.display()))?;
    if bytes.len() > MAX_BACKUP_ARCHIVE_BYTES {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("backup archive grew beyond the {MAX_BACKUP_ARCHIVE_BYTES} byte read limit"),
        ));
    }
    Ok(bytes)
}

pub(super) fn preflight_target(target_home: PathBuf) -> AnyResult<RestoreTarget> {
    let file_name = target_home
        .file_name()
        .context("restore target must name an absent vault home")?;
    if file_name == OsStr::new(".") || file_name == OsStr::new("..") {
        bail!("restore target must name an absent vault home");
    }
    let parent = match target_home.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    reject_symlinked_ancestors(&parent)?;
    let parent = fs::canonicalize(&parent).with_context(|| {
        format!(
            "restore target parent must already exist as a safe directory: {}",
            parent.display()
        )
    })?;
    reject_symlinked_ancestors(&parent)?;
    let metadata = validate_parent(&parent)?;
    let home = parent.join(file_name);
    require_absent(&home)?;
    Ok(RestoreTarget {
        home,
        parent,
        parent_device: metadata.dev(),
        parent_inode: metadata.ino(),
    })
}

pub(super) fn restore(
    passphrase: &SecretString,
    decoded: DecodedBackupArchive,
    target: RestoreTarget,
) -> AnyResult<BackupRestoreResult> {
    revalidate_target(&target)?;
    let mut staging = OwnedStaging::create(&target)?;
    let result = (|| -> AnyResult<BackupRestoreResult> {
        staging.write_file(VAULT_FILE, decoded.vault_bytes())?;
        staging.write_file(AUDIT_FILE, decoded.audit_bytes())?;
        staging.sync()?;

        let staged_store =
            VaultStore::open_existing(staging.path.clone()).map_err(vault_error_as_classified)?;
        staged_store
            .finalize_backup_restore(
                passphrase,
                &decoded.source_vault_id,
                decoded.source_format_version,
                decoded.backup_created_at_ms,
            )
            .map_err(vault_error_as_classified)?;
        staging.sync()?;
        revalidate_target(&target)?;
        staging.install(&target)?;
        Ok(BackupRestoreResult {
            root: target.home.clone(),
            vault_id: decoded.source_vault_id.clone(),
            format_version: decoded.source_format_version,
        })
    })();

    match result {
        Ok(result) => Ok(result),
        Err(error) => match staging.cleanup() {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(error.context(format!(
                "restore failed; additionally could not safely clean its owned staging directory: {cleanup_error}"
            ))),
        },
    }
}

struct OwnedStaging {
    path: PathBuf,
    parent: PathBuf,
    device: u64,
    inode: u64,
    active: bool,
}

impl OwnedStaging {
    fn create(target: &RestoreTarget) -> AnyResult<Self> {
        revalidate_target(target)?;
        let leaf = target
            .home
            .file_name()
            .expect("preflight restore target has a leaf");
        let mut name = OsString::from(".");
        name.push(leaf);
        name.push(format!(
            ".{}.{}.jig-vault-restore.tmp",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let path = target.parent.join(name);
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(&path).with_context(|| {
            format!(
                "failed to create private restore staging directory beside {}",
                target.home.display()
            )
        })?;
        // DirBuilder's 0700 mode can only be tightened by the umask. Stat
        // immediately and retain the exact identity before any cleanup is
        // permitted; an inspection failure deliberately leaves the
        // generated name for manual review instead of deleting an
        // unverified directory entry.
        let metadata = fs::symlink_metadata(&path).with_context(|| {
            format!(
                "restore staging directory was created at {}, but its identity could not be verified; inspect it manually",
                path.display()
            )
        })?;
        validate_owned_directory(&path, &metadata)?;
        let mut staging = Self {
            path,
            parent: target.parent.clone(),
            device: metadata.dev(),
            inode: metadata.ino(),
            active: true,
        };
        let setup = (|| -> AnyResult<()> {
            fs::set_permissions(&staging.path, fs::Permissions::from_mode(0o700)).with_context(
                || {
                    format!(
                        "failed to restrict restore staging directory {}",
                        staging.path.display()
                    )
                },
            )?;
            let metadata = fs::symlink_metadata(&staging.path).with_context(|| {
                format!(
                    "failed to inspect restore staging directory {}",
                    staging.path.display()
                )
            })?;
            staging.validate_metadata_identity(&metadata)?;
            sync_directory(&target.parent)?;
            Ok(())
        })();
        match setup {
            Ok(()) => Ok(staging),
            Err(error) => {
                match staging.cleanup() {
                    Ok(()) => Err(error),
                    Err(cleanup_error) => Err(error.context(format!(
                        "restore staging setup failed; additionally identity-checked cleanup failed: {cleanup_error}"
                    ))),
                }
            }
        }
    }

    fn write_file(&self, name: &str, bytes: &[u8]) -> AnyResult<()> {
        let path = self.path.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .with_context(|| format!("failed to create staged restore file {name}"))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write staged restore file {name}"))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to restrict staged restore file {name}"))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("failed to inspect staged restore file {name}"))?;
        validate_owned_file(&path, &metadata)?;
        file.sync_all()
            .with_context(|| format!("failed to sync staged restore file {name}"))?;
        Ok(())
    }

    fn sync(&self) -> AnyResult<()> {
        self.validate_identity()?;
        sync_directory(&self.path)
    }

    fn install(&mut self, target: &RestoreTarget) -> AnyResult<()> {
        self.validate_identity()?;
        revalidate_target(target)?;
        atomic_rename_noreplace(&self.path, &target.home)?;
        // The generated staging name no longer exists after this point;
        // cleanup must never target the user-selected installed home.
        self.active = false;
        sync_directory(&self.parent).with_context(|| {
            format!(
                "restored vault was installed at {}, but its parent directory could not be synced",
                target.home.display()
            )
        })?;
        Ok(())
    }

    fn cleanup(&mut self) -> AnyResult<()> {
        if !self.active {
            return Ok(());
        }
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.active = false;
                return Ok(());
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect restore staging directory {}",
                        self.path.display()
                    )
                });
            }
        };
        self.validate_metadata_identity(&metadata)?;
        for entry in fs::read_dir(&self.path).with_context(|| {
            format!(
                "failed to enumerate restore staging directory {}",
                self.path.display()
            )
        })? {
            let entry = entry.context("failed to inspect restore staging entry")?;
            let name = entry.file_name();
            if !matches!(name.to_str(), Some(VAULT_FILE | AUDIT_FILE | LOCK_FILE)) {
                bail!(
                    "refusing to clean restore staging directory containing unexpected entry {}",
                    entry.path().display()
                );
            }
            let metadata = fs::symlink_metadata(entry.path()).with_context(|| {
                format!(
                    "failed to inspect staged restore entry {}",
                    entry.path().display()
                )
            })?;
            validate_owned_file(&entry.path(), &metadata)?;
            fs::remove_file(entry.path()).with_context(|| {
                format!(
                    "failed to remove staged restore file {}",
                    entry.path().display()
                )
            })?;
        }
        fs::remove_dir(&self.path).with_context(|| {
            format!(
                "failed to remove restore staging directory {}",
                self.path.display()
            )
        })?;
        sync_directory(&self.parent)?;
        self.active = false;
        Ok(())
    }

    fn validate_identity(&self) -> AnyResult<()> {
        let metadata = fs::symlink_metadata(&self.path).with_context(|| {
            format!(
                "failed to inspect restore staging directory {}",
                self.path.display()
            )
        })?;
        self.validate_metadata_identity(&metadata)
    }

    fn validate_metadata_identity(&self, metadata: &fs::Metadata) -> AnyResult<()> {
        validate_owned_directory(&self.path, metadata)?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            bail!("restore staging directory identity changed; refusing cleanup or install");
        }
        Ok(())
    }
}

impl Drop for OwnedStaging {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn revalidate_target(target: &RestoreTarget) -> AnyResult<()> {
    reject_symlinked_ancestors(&target.parent)?;
    let metadata = validate_parent(&target.parent)?;
    if metadata.dev() != target.parent_device || metadata.ino() != target.parent_inode {
        bail!("restore target parent identity changed after preflight");
    }
    require_absent(&target.home)
}

fn require_absent(path: &Path) -> AnyResult<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(classified(
            VaultErrorKind::AlreadyExists,
            format!(
                "restore target already exists at {}; choose an entirely absent vault home",
                path.display()
            ),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(classify_source(
            VaultErrorKind::Io,
            "failed to inspect restore target",
            error.into(),
        )),
    }
}

fn validate_parent(path: &Path) -> AnyResult<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect restore target parent {}", path.display()))?;
    validate_owned_directory(path, &metadata)?;
    if metadata.permissions().mode() & 0o022 != 0 {
        bail!(
            "restore target parent must not be group- or other-writable: {}",
            path.display()
        );
    }
    Ok(metadata)
}

fn validate_owned_directory(path: &Path, metadata: &fs::Metadata) -> AnyResult<()> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "protected restore path must be a real directory: {}",
            path.display()
        );
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        bail!(
            "protected restore directory is not owned by the current user: {}",
            path.display()
        );
    }
    if metadata.permissions().mode() & 0o077 != 0
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("jig-vault-restore.tmp"))
    {
        bail!("restore staging directory is not owner-only");
    }
    Ok(())
}

fn validate_owned_file(path: &Path, metadata: &fs::Metadata) -> AnyResult<()> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "staged restore path is not a regular file: {}",
            path.display()
        );
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        bail!(
            "staged restore file is not owned by the current user: {}",
            path.display()
        );
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        bail!("staged restore file is not owner-only: {}", path.display());
    }
    Ok(())
}

fn reject_symlinked_ancestors(path: &Path) -> AnyResult<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for restore target")?
            .join(path)
    };
    let mut ancestors = absolute.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for ancestor in ancestors {
        let metadata = fs::symlink_metadata(ancestor).with_context(|| {
            format!(
                "failed to inspect restore path ancestor {}",
                ancestor.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing restore through symlinked parent {}",
                ancestor.display()
            );
        }
    }
    Ok(())
}

fn atomic_rename_noreplace(source: &Path, destination: &Path) -> AnyResult<()> {
    let source =
        CString::new(source.as_os_str().as_bytes()).context("restore staging path contains NUL")?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .context("restore target path contains NUL")?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EEXIST | libc::ENOTEMPTY) => Err(classify_source(
            VaultErrorKind::AlreadyExists,
            "restore target appeared before atomic installation; nothing was overwritten",
            error.into(),
        )),
        Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP) => Err(classify_source(
            VaultErrorKind::InvalidInput,
            "the restore target filesystem does not support atomic absent-target directory installation",
            error.into(),
        )),
        Some(libc::EXDEV) => Err(classify_source(
            VaultErrorKind::InvalidInput,
            "restore staging unexpectedly crossed filesystems; nothing was installed",
            error.into(),
        )),
        _ => Err(classify_source(
            VaultErrorKind::Io,
            "failed to atomically install restored vault without replacement",
            error.into(),
        )),
    }
}

fn sync_directory(path: &Path) -> AnyResult<()> {
    File::open(path)
        .with_context(|| format!("failed to open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync directory {}", path.display()))
}

fn vault_error_as_classified(error: VaultError) -> anyhow::Error {
    classified(error.kind(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_tempdir() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        temp
    }

    #[test]
    fn owned_staging_cleanup_removes_only_validated_generated_entries() {
        let temp = private_tempdir();
        let target = preflight_target(temp.path().join("restored-home")).unwrap();
        let mut staging = OwnedStaging::create(&target).unwrap();
        let staging_path = staging.path.clone();
        staging.write_file(VAULT_FILE, b"vault").unwrap();
        staging.write_file(AUDIT_FILE, b"audit").unwrap();
        staging.cleanup().unwrap();
        assert!(!staging_path.exists());
        assert!(!target.home.exists());
    }

    #[test]
    fn staging_cleanup_refuses_a_replaced_directory_identity() {
        let temp = private_tempdir();
        let target = preflight_target(temp.path().join("restored-home")).unwrap();
        let mut staging = OwnedStaging::create(&target).unwrap();
        let original = staging.path.with_extension("original-stage");
        fs::rename(&staging.path, &original).unwrap();
        fs::create_dir(&staging.path).unwrap();
        fs::set_permissions(&staging.path, fs::Permissions::from_mode(0o700)).unwrap();

        let error = staging.cleanup().unwrap_err();
        assert!(error.to_string().contains("identity changed"));
        assert!(staging.path.exists());
        assert!(original.exists());

        // Test-only explicit cleanup of the two exact paths. Disable the
        // guard first so Drop cannot act on the replacement.
        staging.active = false;
        fs::remove_dir(&staging.path).unwrap();
        fs::remove_dir(&original).unwrap();
    }

    #[test]
    fn atomic_install_never_replaces_a_raced_target() {
        let temp = private_tempdir();
        let target = preflight_target(temp.path().join("restored-home")).unwrap();
        let mut staging = OwnedStaging::create(&target).unwrap();
        let staging_path = staging.path.clone();
        staging.write_file(VAULT_FILE, b"vault").unwrap();
        staging.write_file(AUDIT_FILE, b"audit").unwrap();
        fs::create_dir(&target.home).unwrap();
        fs::write(target.home.join("marker"), b"unchanged").unwrap();

        // Invoke the final primitive directly to model the target
        // appearing after the last ordinary preflight check.
        let error = atomic_rename_noreplace(&staging.path, &target.home).unwrap_err();
        assert_eq!(
            crate::error::classified_kind(&error),
            Some(VaultErrorKind::AlreadyExists)
        );
        assert_eq!(fs::read(target.home.join("marker")).unwrap(), b"unchanged");
        staging.cleanup().unwrap();
        assert!(!staging_path.exists());
    }
}
