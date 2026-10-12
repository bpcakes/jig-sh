//! Restore staging ownership.
//!
//! Only staging the current operation created exclusively is ever cleaned
//! up, and only until it is installed or its ownership is handed off
//! immediately before a journal can publish it. Staging that a pending
//! transaction names is reached through a recovery reference: it may be
//! inspected, authenticated, and installed without replacement, but never
//! deleted, and nothing turns a reference back into an owner. An installed
//! home is never staging. A journal's description of a directory (its
//! name, inode, or plausible contents) never confers ownership.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result as AnyResult, bail};

use crate::acl;

use super::{
    AUDIT_FILE, LOCK_FILE, RestoreTarget, VAULT_FILE, atomic_rename_noreplace, revalidate_target,
    sync_directory, validate_owned_directory,
};

/// The verified identity of one staging directory.
struct Identity {
    path: PathBuf,
    parent: PathBuf,
    device: u64,
    inode: u64,
}

impl Identity {
    fn validate(&self) -> AnyResult<()> {
        let metadata = fs::symlink_metadata(&self.path).with_context(|| {
            format!(
                "failed to inspect restore staging directory {}",
                self.path.display()
            )
        })?;
        self.validate_metadata(&metadata)
    }

    fn validate_metadata(&self, metadata: &fs::Metadata) -> AnyResult<()> {
        validate_owned_directory(&self.path, metadata)?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            bail!("restore staging directory identity changed; refusing cleanup or install");
        }
        Ok(())
    }
}

/// Staging created exclusively by the current operation. Every failure
/// cleans it up until [`FreshStaging::install`] publishes it or
/// [`FreshStaging::hand_off`] ends this operation's ownership.
pub(super) struct FreshStaging {
    owned: Option<Identity>,
}

impl FreshStaging {
    /// Legacy staging named beside the target for direct installation.
    pub(super) fn create(target: &RestoreTarget) -> AnyResult<Self> {
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

    pub(super) fn create_named(target: &RestoreTarget, leaf: &str) -> AnyResult<Self> {
        Self::create_at(target, target.parent.join(leaf))
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
        let staging = Self {
            owned: Some(Identity {
                path,
                parent: target.parent.clone(),
                device: metadata.dev(),
                inode: metadata.ino(),
            }),
        };
        let setup = (|| -> AnyResult<()> {
            let path = staging.path();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
                format!(
                    "failed to restrict restore staging directory {}",
                    path.display()
                )
            })?;
            // Staged files must not inherit the parent's ACL entries.
            acl::clear_directory(path)?;
            let metadata = fs::symlink_metadata(path).with_context(|| {
                format!(
                    "failed to inspect restore staging directory {}",
                    path.display()
                )
            })?;
            staging.identity().validate_metadata(&metadata)?;
            sync_directory(&target.parent)
        })();
        match setup {
            Ok(()) => Ok(staging),
            Err(error) => Err(staging.abandon(error.context("restore staging setup failed"))),
        }
    }

    fn identity(&self) -> &Identity {
        self.owned
            .as_ref()
            .expect("fresh staging is owned until consumed")
    }

    pub(super) fn path(&self) -> &Path {
        &self.identity().path
    }

    pub(super) fn device(&self) -> u64 {
        self.identity().device
    }

    pub(super) fn inode(&self) -> u64 {
        self.identity().inode
    }

    pub(super) fn write_file(&self, name: &str, bytes: &[u8]) -> AnyResult<()> {
        let path = self.path().join(name);
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
        crate::store::durable::sync_file(&file, &path)
    }

    pub(super) fn sync(&self) -> AnyResult<()> {
        self.identity().validate()?;
        sync_directory(self.path())
    }

    /// Publishes the staging as `target`'s home without replacement. Until
    /// the rename it is still cleaned up on failure; afterwards the
    /// generated name no longer exists, and the installed home is never
    /// cleaned up, even if syncing its entry fails.
    pub(super) fn install(mut self, target: &RestoreTarget) -> AnyResult<()> {
        let checked = (|| -> AnyResult<()> {
            self.identity().validate()?;
            require_private_contents(self.path())?;
            revalidate_target(target)?;
            atomic_rename_noreplace(self.path(), &target.home)
        })();
        if let Err(error) = checked {
            return Err(self.abandon(error));
        }
        let identity = self.owned.take().expect("fresh staging is owned");
        sync_directory(&identity.parent).with_context(|| {
            format!(
                "restored vault was installed at {}, but its parent directory could not be synced",
                target.home.display()
            )
        })
    }

    /// Ends this operation's ownership immediately before a journal can
    /// publish the staging as recovery data. From here every failure,
    /// including a published rename whose sync failed, preserves it.
    pub(super) fn hand_off(mut self) {
        self.owned = None;
    }

    /// Cleans up after `error`, reporting any cleanup failure with it.
    pub(super) fn abandon(mut self, error: anyhow::Error) -> anyhow::Error {
        match self.cleanup() {
            Ok(()) => error,
            Err(cleanup_error) => error.context(format!(
                "additionally could not safely clean the restore's owned staging directory: {cleanup_error}"
            )),
        }
    }

    fn cleanup(&mut self) -> AnyResult<()> {
        let Some(identity) = self.owned.take() else {
            return Ok(());
        };
        let metadata = match fs::symlink_metadata(&identity.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect restore staging directory {}",
                        identity.path.display()
                    )
                });
            }
        };
        identity.validate_metadata(&metadata)?;
        for entry in fs::read_dir(&identity.path).with_context(|| {
            format!(
                "failed to enumerate restore staging directory {}",
                identity.path.display()
            )
        })? {
            let entry = entry.context("failed to inspect restore staging entry")?;
            if !matches!(
                entry.file_name().to_str(),
                Some(VAULT_FILE | AUDIT_FILE | LOCK_FILE)
            ) {
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
        fs::remove_dir(&identity.path).with_context(|| {
            format!(
                "failed to remove restore staging directory {}",
                identity.path.display()
            )
        })?;
        sync_directory(&identity.parent)
    }
}

impl Drop for FreshStaging {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Staging that a validated pending transaction names. It may be inspected
/// and installed without replacement; it is never deleted.
pub(super) struct StagingRef {
    identity: Identity,
}

impl StagingRef {
    pub(super) fn adopt(
        path: PathBuf,
        parent: PathBuf,
        device: u64,
        inode: u64,
    ) -> AnyResult<Self> {
        let identity = Identity {
            path,
            parent,
            device,
            inode,
        };
        identity.validate()?;
        Ok(Self { identity })
    }

    pub(super) fn path(&self) -> &Path {
        &self.identity.path
    }

    /// Requires what installation would publish to still be private.
    pub(super) fn require_installable(&self) -> AnyResult<()> {
        require_owner_only(self.path())?;
        require_private_contents(self.path())
    }

    /// Installs the staging at `home` without replacement; the caller syncs
    /// the parent's entry.
    pub(super) fn install(self, home: &Path) -> AnyResult<()> {
        self.identity.validate()?;
        atomic_rename_noreplace(self.path(), home)
    }
}

/// Requires `home` to be the recorded staging directory itself, installed
/// and still private: its identity, an owner-only mode whatever its name,
/// and owned private contents without ACL entries.
pub(super) fn require_recorded_private_home(home: &Path, device: u64, inode: u64) -> AnyResult<()> {
    let identity = Identity {
        path: home.to_path_buf(),
        parent: home.parent().map(Path::to_path_buf).unwrap_or_default(),
        device,
        inode,
    };
    identity.validate()?;
    require_owner_only(home)?;
    require_private_contents(home)
}

fn require_owner_only(path: &Path) -> AnyResult<()> {
    let mode = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?
        .mode();
    if mode & 0o077 != 0 {
        bail!(
            "restore directory {} is no longer private to the current user",
            path.display()
        );
    }
    Ok(())
}

/// Rechecks everything that will be published, including files the vault
/// store wrote while finalizing the staged restore.
fn require_private_contents(path: &Path) -> AnyResult<()> {
    acl::require_none(path)?;
    for entry in fs::read_dir(path).with_context(|| {
        format!(
            "failed to enumerate restore staging directory {}",
            path.display()
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
