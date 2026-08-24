use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

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
    let parent = prepare_target_parent(&parent)?;
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
    let missing = validate_creation_ancestors(&parent)?;
    create_private_parent_chain(&missing)?;
    reject_symlinked_ancestors(&parent)?;
    let parent = fs::canonicalize(&parent).with_context(|| {
        format!(
            "failed to resolve private restore target parent {}",
            parent.display()
        )
    })?;
    reject_symlinked_ancestors(&parent)?;
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
        if !checked_creation_boundary {
            let mode = metadata.permissions().mode() & 0o7777;
            let owner = metadata.uid();
            if !creation_boundary_is_safe(mode, owner, unsafe { libc::geteuid() }) {
                bail!(
                    "refusing to create restore target parent below shared-writable ancestor {}",
                    ancestor.display()
                );
            }
            checked_creation_boundary = true;
        }
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
    use std::os::unix::process::CommandExt;

    const UMASK_CHILD_ENV: &str = "JIG_VAULT_RESTORE_UMASK_CHILD";

    fn private_tempdir() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        temp
    }

    fn rerun_current_test_with_umask(mode: libc::mode_t) -> bool {
        if std::env::var_os(UMASK_CHILD_ENV).is_some() {
            return false;
        }
        let test_name = std::thread::current()
            .name()
            .expect("test harness thread has no name")
            .to_owned();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .arg(&test_name)
            .arg("--exact")
            .arg("--nocapture")
            .env(UMASK_CHILD_ENV, "1");
        unsafe {
            command.pre_exec(move || {
                libc::umask(mode);
                Ok(())
            });
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "test subprocess failed under umask {mode:03o}:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    #[test]
    fn preflight_creates_private_missing_parents_but_keeps_the_vault_home_absent() {
        if rerun_current_test_with_umask(0o777) {
            return;
        }
        let temp = private_tempdir();
        let parent = temp.path().join("vault-base/scopes");
        let home = parent.join("repo-scope");

        let target = preflight_target(home.clone()).unwrap();

        assert_eq!(target.parent, fs::canonicalize(&parent).unwrap());
        assert_eq!(
            target.home,
            fs::canonicalize(&parent).unwrap().join("repo-scope")
        );
        assert!(!home.exists());
        for created in [temp.path().join("vault-base"), parent] {
            assert_eq!(
                fs::metadata(created).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn preflight_refuses_to_create_parents_below_a_group_writable_ancestor() {
        let temp = private_tempdir();
        let shared = temp.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o770)).unwrap();
        let parent = shared.join("vault-base/scopes");

        let error = preflight_target(parent.join("repo-scope"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("shared-writable ancestor"), "{error}");
        assert!(!parent.exists());
    }

    #[test]
    fn preflight_allows_a_sticky_shared_writable_boundary_owned_by_the_current_user() {
        let temp = private_tempdir();
        let shared = temp.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o1770)).unwrap();
        let parent = shared.join("vault-base/scopes");

        let target = preflight_target(parent.join("repo-scope")).unwrap();

        assert_eq!(target.parent, fs::canonicalize(&parent).unwrap());
        assert!(!target.home.exists());
    }

    #[test]
    fn sticky_boundary_policy_rejects_an_untrusted_directory_owner() {
        let effective_user = unsafe { libc::geteuid() };
        let other_user = if effective_user == u32::MAX {
            1
        } else {
            effective_user + 1
        };

        assert!(creation_boundary_is_safe(
            0o1770,
            effective_user,
            effective_user
        ));
        assert!(creation_boundary_is_safe(0o1777, 0, effective_user));
        assert!(!creation_boundary_is_safe(
            0o1770,
            other_user,
            effective_user
        ));
        assert!(!creation_boundary_is_safe(
            0o0770,
            effective_user,
            effective_user
        ));
    }

    #[test]
    fn preflight_resolves_a_bare_relative_missing_parent_from_the_current_directory() {
        let temp = private_tempdir();
        let parent = Path::new("recovery/vault-base/scopes");

        let prepared = prepare_target_parent_from(parent, temp.path()).unwrap();

        let expected = temp.path().join(parent);
        assert_eq!(prepared, fs::canonicalize(&expected).unwrap());
        assert!(expected.is_dir());
    }

    #[test]
    fn preflight_rejects_parent_traversal_through_a_missing_component_without_mutation() {
        let temp = private_tempdir();
        let parent = Path::new("recovery/../vault-base/scopes");

        let error = prepare_target_parent_from(parent, temp.path())
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot traverse through a missing component"));
        assert!(!temp.path().join("recovery").exists());
        assert!(!temp.path().join("vault-base").exists());
    }

    #[test]
    fn preflight_preserves_leading_parent_traversal_when_the_prefix_exists() {
        let temp = private_tempdir();
        let invocation_dir = temp.path().join("invocation");
        fs::create_dir(&invocation_dir).unwrap();
        fs::set_permissions(&invocation_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let parent = Path::new("../recovery/vault-base/scopes");

        let prepared = prepare_target_parent_from(parent, &invocation_dir).unwrap();

        let expected = temp.path().join("recovery/vault-base/scopes");
        assert_eq!(prepared, fs::canonicalize(&expected).unwrap());
        assert!(expected.is_dir());
    }

    #[test]
    fn preflight_refuses_an_existing_symlink_above_the_creation_boundary() {
        if rerun_current_test_with_umask(0o000) {
            return;
        }
        let temp = private_tempdir();
        let real = temp.path().join("real");
        let existing = real.join("existing");
        fs::create_dir_all(&existing).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o700)).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let parent = link.join("existing/vault-base/scopes");

        let error = preflight_target(parent.join("repo-scope"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("symlinked ancestor"), "{error}");
        assert!(!existing.join("vault-base").exists());
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
