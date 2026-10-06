use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result as AnyResult, bail};
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::acl;
use crate::error::{classified, classify_source};
use crate::store::VaultStore;
use crate::{VaultError, VaultErrorKind};

use super::payload::DecodedBackupArchive;
use super::{BackupRestoreResult, MAX_BACKUP_ARCHIVE_BYTES, RestoreTarget};

mod witnessed;

pub(super) use witnessed::restore;
pub(crate) use witnessed::{discard_orphan_restore, finish_pending_restore, read_candidate};

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
    let parent = prepare_target_parent(&parent)?;
    let metadata = validate_parent(&parent)?;
    let home = parent.join(file_name);
    if !witnessed::target_has_transaction(&home)? {
        require_absent(&home)?;
    }
    Ok(RestoreTarget {
        home,
        parent,
        parent_device: metadata.dev(),
        parent_inode: metadata.ino(),
    })
}

fn prepare_target_parent(parent: &Path) -> AnyResult<PathBuf> {
    let current_dir = std::env::current_dir()
        .context("failed to resolve current directory for restore target")?;
    prepare_target_parent_from(parent, &current_dir)
}

fn prepare_target_parent_from(parent: &Path, current_dir: &Path) -> AnyResult<PathBuf> {
    let parent = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        current_dir.join(parent)
    };
    // Resolve only fixed platform root aliases such as macOS /var and /tmp;
    // every user-controlled component below them must still be a real
    // directory.
    let parent = crate::path_security::physical_path(&parent, "restore target parent")?;
    let missing = validate_creation_ancestors(&parent)?;
    create_private_parent_chain(&missing)?;
    validate_trusted_ancestors(&parent)?;
    let parent = fs::canonicalize(&parent).with_context(|| {
        format!(
            "failed to resolve private restore target parent {}",
            parent.display()
        )
    })?;
    validate_trusted_ancestors(&parent)?;
    Ok(parent)
}

fn validate_creation_ancestors(path: &Path) -> AnyResult<Vec<PathBuf>> {
    let mut checked_creation_boundary = false;
    let mut missing = Vec::new();
    for ancestor in path.ancestors() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if matches!(
                    ancestor.components().next_back(),
                    Some(Component::ParentDir)
                ) {
                    bail!(
                        "restore target parent cannot traverse through a missing component before {}",
                        ancestor.display()
                    );
                }
                if !checked_creation_boundary {
                    missing.push(ancestor.to_path_buf());
                }
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect restore target creation ancestor {}",
                        ancestor.display()
                    )
                });
            }
        };
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing to create restore target parent through symlinked ancestor {}",
                ancestor.display()
            );
        }
        if !metadata.is_dir() {
            bail!(
                "restore target creation ancestor is not a directory: {}",
                ancestor.display()
            );
        }
        // Every existing ancestor, not only the creation boundary, must be
        // trusted before anything is created below it.
        validate_trusted_ancestor(ancestor, &metadata)?;
        checked_creation_boundary = true;
    }
    if checked_creation_boundary {
        Ok(missing)
    } else {
        bail!(
            "restore target has no existing directory ancestor: {}",
            path.display()
        )
    }
}

/// Another user who can rename any directory on the restore path can move
/// the restored subtree away and substitute their own, so each existing
/// ancestor must be on a volume that honors ownership, must be owned by the
/// current user or root, must not be writable by others unless sticky with a
/// trusted owner, and on macOS must not carry an ACL that lets others write
/// to, delete, or re-permission it.
fn validate_trusted_ancestor(path: &Path, metadata: &fs::Metadata) -> AnyResult<()> {
    // The owner checks below prove nothing on a volume that ignores
    // ownership, and nested mounts can place one anywhere above the target.
    reject_ownership_ignoring_volume(path)?;
    let mode = metadata.permissions().mode() & 0o7777;
    let owner = metadata.uid();
    let effective_user = unsafe { libc::geteuid() };
    if !ancestor_owner_is_trusted(owner, effective_user) {
        bail!(
            "refusing restore below an ancestor owned by another user: {}",
            path.display()
        );
    }
    if !creation_boundary_is_safe(mode, owner, effective_user) {
        bail!(
            "refusing restore below shared-writable ancestor {}",
            path.display()
        );
    }
    acl::reject_shared_write(path)
}

fn ancestor_owner_is_trusted(owner: u32, effective_user: u32) -> bool {
    owner == effective_user || owner == 0
}

fn creation_boundary_is_safe(mode: u32, owner: u32, effective_user: u32) -> bool {
    let shared_writable = mode & 0o022 != 0;
    let sticky = mode & 0o1000 != 0;
    let trusted_sticky_owner = owner == effective_user || owner == 0;
    !shared_writable || (sticky && trusted_sticky_owner)
}

fn create_private_parent_chain(missing: &[PathBuf]) -> AnyResult<()> {
    for path in missing.iter().rev() {
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path).with_context(|| {
            format!(
                "failed to create private restore target parent {}",
                path.display()
            )
        })?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!(
                "failed to restrict restore target parent {}",
                path.display()
            )
        })?;
        let metadata = fs::symlink_metadata(path).with_context(|| {
            format!(
                "failed to inspect created restore target parent {}",
                path.display()
            )
        })?;
        validate_owned_directory(path, &metadata)?;
        if metadata.permissions().mode() & 0o777 != 0o700 {
            bail!(
                "created restore target parent is not owner-only: {}",
                path.display()
            );
        }
        // Clear inherited entries before the next component is created so
        // nothing below this directory can inherit them either.
        acl::clear_directory(path)?;
        sync_directory(path).with_context(|| {
            format!(
                "failed to sync created restore target parent {}",
                path.display()
            )
        })?;
        let containing_parent = path
            .parent()
            .context("created restore target parent must have an existing containing directory")?;
        sync_directory(containing_parent).with_context(|| {
            format!(
                "failed to sync directory entry for created restore target parent {}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Restores an unwitnessed format 2 archive exactly as archived.
fn restore_legacy(
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
            source_format_version: decoded.source_format_version,
            generation: None,
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
        Self::create_at(target, target.parent.join(name))
    }

    fn create_at(target: &RestoreTarget, path: PathBuf) -> AnyResult<Self> {
        revalidate_target(target)?;
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
            // Staged files must not inherit the parent's ACL entries.
            acl::clear_directory(&staging.path)?;
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
        acl::clear_file(&file, &path)?;
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
        self.require_private_contents()?;
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

    /// Rechecks everything that will be published, including files the
    /// vault store wrote while finalizing the staged restore.
    fn require_private_contents(&self) -> AnyResult<()> {
        acl::require_none(&self.path)?;
        for entry in fs::read_dir(&self.path).with_context(|| {
            format!(
                "failed to enumerate restore staging directory {}",
                self.path.display()
            )
        })? {
            let entry = entry.context("failed to inspect restore staging entry")?;
            if !matches!(
                entry.file_name().to_str(),
                Some(VAULT_FILE | AUDIT_FILE | LOCK_FILE)
            ) {
                bail!(
                    "refusing to install restore staging directory containing unexpected entry {}",
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
            acl::require_none(&entry.path())?;
        }
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
    validate_trusted_ancestors(&target.parent)?;
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

/// Refuses volumes whose ownership checks prove nothing.
///
/// macOS can mount a volume with "ignore ownership", which reports every
/// entry as owned by the accessing user. The owner and owner-only checks in
/// this module would then pass for any local user, so restore fails closed.
#[cfg(target_os = "macos")]
fn reject_ownership_ignoring_volume(path: &Path) -> AnyResult<()> {
    let c_path =
        CString::new(path.as_os_str().as_bytes()).context("restore target path contains NUL")?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c_path` is NUL-terminated and `stats` is valid for writes of
    // one `statfs`; it is read only after the call reports success.
    if unsafe { libc::statfs(c_path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!("failed to inspect the volume containing {}", path.display())
        });
    }
    // SAFETY: statfs succeeded and initialized the structure.
    let stats = unsafe { stats.assume_init() };
    if stats.f_flags & libc::MNT_IGNORE_OWNERSHIP as u32 != 0 {
        bail!(
            "refusing restore through a volume that ignores file ownership: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn reject_ownership_ignoring_volume(_path: &Path) -> AnyResult<()> {
    Ok(())
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

/// Rechecks every directory from the filesystem root down to `path`.
fn validate_trusted_ancestors(path: &Path) -> AnyResult<()> {
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
        if !metadata.is_dir() {
            bail!(
                "restore path ancestor is not a directory: {}",
                ancestor.display()
            );
        }
        validate_trusted_ancestor(ancestor, &metadata)?;
    }
    Ok(())
}

fn atomic_rename_noreplace(source: &Path, destination: &Path) -> AnyResult<()> {
    let source =
        CString::new(source.as_os_str().as_bytes()).context("restore staging path contains NUL")?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .context("restore target path contains NUL")?;
    let Err(error) = rename_noreplace(&source, &destination) else {
        return Ok(());
    };
    match error.raw_os_error() {
        Some(libc::EEXIST | libc::ENOTEMPTY) => Err(classify_source(
            VaultErrorKind::AlreadyExists,
            "restore target appeared before atomic installation; nothing was overwritten",
            error.into(),
        )),
        Some(errno) if noreplace_is_unsupported(errno) => Err(classify_source(
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

#[cfg(target_os = "linux")]
fn rename_noreplace(source: &CStr, destination: &CStr) -> std::io::Result<()> {
    // SAFETY: both strings are NUL-terminated and live for the duration of the call.
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
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn rename_noreplace(source: &CStr, destination: &CStr) -> std::io::Result<()> {
    // SAFETY: both strings are NUL-terminated and live for the duration of the call.
    if unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
const fn noreplace_is_unsupported(errno: i32) -> bool {
    matches!(errno, libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
}

/// Darwin reports a rename flag the filesystem cannot honor as `ENOTSUP`,
/// which, unlike on Linux, is a different value from `EOPNOTSUPP`.
#[cfg(target_os = "macos")]
const fn noreplace_is_unsupported(errno: i32) -> bool {
    matches!(errno, libc::EINVAL | libc::ENOTSUP | libc::EOPNOTSUPP)
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
mod tests;
