//! Out-of-home rollback witness.
//!
//! One per-user tree records, for every witnessed format 3 vault ID, the
//! committed state checkpoint and at most one durable pending transaction
//! marker. Target-keyed journals hold the exact candidate of a pending
//! transaction, and lock files serialize absent targets and same-ID copies.
//! The tree is protected internal storage: it is never a private-output
//! destination, and nothing in it is read through symlinks or unbounded.
//!
//! The witness protects previously witnessed history only while it
//! survives. Whole-profile rollback, deleting or replacing it, same-user or
//! root compromise, and forged events made with a compromised audit root are
//! outside its guarantee. It is not a remote authority.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result as AnyResult, bail};
use fs4::fs_std::FileExt;
use sha2::{Digest, Sha256};

mod journal;
mod record;

pub(crate) use journal::{
    AuditTransition, InPlacePayload, JOURNAL_SCHEMA, Journal, JournalPayload, JournalTarget,
    RestorePayload,
};
pub(crate) use record::{Checkpoint, PendingMarker, TransactionKind, WitnessRecord};

use super::{
    ensure_create_ancestor_is_not_shared_writable, ensure_create_base_is_not_symlink,
    ensure_private_dir_permissions, lock_file, path_is_symlink, private_open_options,
    sync_parent_dir, write_atomic_text,
};

/// Witness root below the user's home directory in production builds.
const PER_USER_RELATIVE_ROOT: &str = ".jig/vault-witness";
const IDS_DIR: &str = "ids";
const JOURNALS_DIR: &str = "journals";
const LOCKS_DIR: &str = "locks";
const RECORD_READ_LIMIT: u64 = 64 * 1024;
#[cfg(unix)]
const MAX_ALIAS_SCAN_ENTRIES: usize = 100_000;
/// Bounds an in-place journal: one envelope up to the persistent vault
/// limit, escaped as JSON, plus a single audit line.
pub(crate) const JOURNAL_READ_LIMIT: u64 = 48 * 1024 * 1024;

/// Test and `test-utils` builds read this variable before falling back to a
/// directory beside the vault home, so tests never touch the operator's
/// profile. Production builds ignore it.
#[cfg(any(test, feature = "test-utils"))]
pub(crate) const WITNESS_ROOT_ENV_FOR_TESTS: &str = "JIG_VAULT_WITNESS_ROOT";
#[cfg(any(test, feature = "test-utils"))]
const TEST_WITNESS_DIR_NAME: &str = ".jig-vault-witness";

/// Where the witness for one vault home lives. Resolving a location never
/// creates or reads anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WitnessLocation {
    root: PathBuf,
}

impl WitnessLocation {
    /// Production builds always use the per-user `~/.jig/vault-witness`,
    /// independent of repository, global, explicit-home, or
    /// `JIG_VAULT_HOME` selection.
    pub(crate) fn for_home(home: &Path) -> AnyResult<Self> {
        #[cfg(any(test, feature = "test-utils"))]
        {
            Self::isolated_for_tests(home)
        }
        #[cfg(not(any(test, feature = "test-utils")))]
        {
            let _ = home;
            Self::per_user()
        }
    }

    #[cfg_attr(any(test, feature = "test-utils"), allow(dead_code))]
    fn per_user() -> AnyResult<Self> {
        let home_dir = dirs::home_dir()
            .context("could not resolve the home directory for the vault witness")?;
        Ok(Self {
            root: per_user_root(&home_dir)?,
        })
    }

    #[cfg(any(test, feature = "test-utils"))]
    fn isolated_for_tests(home: &Path) -> AnyResult<Self> {
        let root = match std::env::var_os(WITNESS_ROOT_ENV_FOR_TESTS) {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => home
                .parent()
                .context("test vault home has no parent for its isolated witness")?
                .join(TEST_WITNESS_DIR_NAME),
        };
        Ok(Self {
            root: crate::path_security::physical_path(&root, "vault witness")?,
        })
    }

    #[cfg(test)]
    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Refuses a vault home and witness that contain one another, so neither
    /// can replace, lock, or publish the other's files.
    pub(crate) fn ensure_disjoint(&self, home: &Path) -> AnyResult<()> {
        let witness = resolve_existing_prefix(&self.root)?;
        let home = resolve_existing_prefix(home)?;
        if witness.starts_with(&home) || home.starts_with(&witness) {
            bail!("the vault home and the rollback witness must not overlap");
        }
        Ok(())
    }

    /// Whether `path` names the witness tree or anything inside it.
    pub(crate) fn contains(&self, path: &Path) -> AnyResult<bool> {
        let witness = resolve_existing_prefix(&self.root)?;
        let path = resolve_existing_prefix(path)?;
        Ok(path.starts_with(witness))
    }

    /// Whether `output` is a hard link to a record, journal, or lock in the
    /// witness. Never creates or follows anything.
    #[cfg(unix)]
    pub(crate) fn aliases_protected_file(&self, output: &fs::Metadata) -> AnyResult<bool> {
        let Some(witness) = self.open_existing()? else {
            return Ok(false);
        };
        for child in [IDS_DIR, JOURNALS_DIR, LOCKS_DIR] {
            let directory = witness.root.join(child);
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect vault witness {}", directory.display())
                    });
                }
            };
            for (index, entry) in entries.enumerate() {
                if index >= MAX_ALIAS_SCAN_ENTRIES {
                    bail!("vault witness has too many entries to check output aliases safely");
                }
                let metadata = fs::symlink_metadata(entry?.path())?;
                if metadata.dev() == output.dev() && metadata.ino() == output.ino() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Opens an existing witness without creating, chmodding, or locking
    /// anything. `None` means no witness tree exists yet.
    pub(crate) fn open_existing(&self) -> AnyResult<Option<WitnessStore>> {
        match fs::symlink_metadata(&self.root) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect vault witness {}", self.root.display())
                });
            }
        }
        let root = validate_existing_private_dir(&self.root)?;
        Ok(Some(WitnessStore { root }))
    }

    /// Opens the witness, creating its private tree when needed.
    pub(crate) fn open_or_create(&self) -> AnyResult<WitnessStore> {
        if path_is_symlink(&self.root)? {
            bail!(
                "vault witness {} must not be a symlink",
                self.root.display()
            );
        }
        if !self.root.exists() {
            ensure_create_base_is_not_symlink(&self.root)?;
            ensure_create_ancestor_is_not_shared_writable(&self.root)?;
            fs::create_dir_all(&self.root).with_context(|| {
                format!("failed to create vault witness {}", self.root.display())
            })?;
        }
        let root = prepare_private_dir(&self.root)?;
        for child in [IDS_DIR, JOURNALS_DIR, LOCKS_DIR] {
            let path = root.join(child);
            match fs::symlink_metadata(&path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&path).with_context(|| {
                        format!(
                            "failed to create vault witness directory {}",
                            path.display()
                        )
                    })?;
                    sync_parent_dir(&root)?;
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to inspect {}", path.display()));
                }
            }
            prepare_private_dir(&path)?;
        }
        Ok(WitnessStore { root })
    }
}

fn per_user_root(home_dir: &Path) -> AnyResult<PathBuf> {
    crate::path_security::physical_path(&home_dir.join(PER_USER_RELATIVE_ROOT), "vault witness")
}

/// An opened, validated witness tree.
#[derive(Debug)]
pub(crate) struct WitnessStore {
    root: PathBuf,
}

pub(crate) fn record_vault_id_is_valid(vault_id: &str) -> bool {
    record::validate_vault_id(vault_id).is_ok()
}

/// The SHA-256 of `bytes` as lowercase hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Fixed-size, path-safe key for one vault ID. Header text is never used as
/// a path.
pub(crate) fn id_key(vault_id: &str) -> String {
    domain_key(b"jig-vault-witness/id/v1", vault_id.as_bytes())
}

/// Fixed-size key for one validated physical final path.
pub(crate) fn target_key(path: &Path) -> String {
    domain_key(b"jig-vault-witness/target/v1", path_bytes(path).as_ref())
}

impl WitnessStore {
    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    fn record_path(&self, vault_id: &str) -> PathBuf {
        self.root
            .join(IDS_DIR)
            .join(format!("{}.json", id_key(vault_id)))
    }

    fn journal_path(&self, target_key: &str) -> PathBuf {
        self.root
            .join(JOURNALS_DIR)
            .join(format!("{target_key}.json"))
    }

    /// Reads and validates the record for `vault_id`. Unreadable, malformed,
    /// or symlinked records fail closed and are never treated as absent.
    pub(crate) fn read_record(&self, vault_id: &str) -> AnyResult<Option<WitnessRecord>> {
        record::validate_vault_id(vault_id)?;
        let Some(bytes) = read_protected(&self.record_path(vault_id), RECORD_READ_LIMIT)? else {
            return Ok(None);
        };
        let record: WitnessRecord = serde_json::from_slice(&bytes)
            .context("vault witness record is malformed; refusing to treat it as absent")?;
        record.validate(vault_id)?;
        Ok(Some(record))
    }

    pub(crate) fn write_record(&self, record: &WitnessRecord) -> AnyResult<()> {
        record.validate(&record.vault_id)?;
        let text = serde_json::to_string_pretty(record)?;
        write_atomic_text(&self.record_path(&record.vault_id), &text)
            .context("failed to persist the vault witness record")
    }

    /// Reads the journal for one target, returning it with the SHA-256 of
    /// its exact bytes.
    pub(crate) fn read_journal(&self, target_key: &str) -> AnyResult<Option<(Journal, String)>> {
        let Some(bytes) = read_protected(&self.journal_path(target_key), JOURNAL_READ_LIMIT)?
        else {
            return Ok(None);
        };
        let digest = sha256_hex(&bytes);
        let journal: Journal =
            serde_json::from_slice(&bytes).context("vault transaction journal is malformed")?;
        journal.validate()?;
        if journal.target.target_key != target_key {
            bail!("vault transaction journal does not belong to this target");
        }
        Ok(Some((journal, digest)))
    }

    /// Atomically persists a journal and returns the digest of its bytes.
    pub(crate) fn write_journal(&self, journal: &Journal) -> AnyResult<String> {
        journal.validate()?;
        let text = serde_json::to_string(journal)?;
        if text.len() as u64 > JOURNAL_READ_LIMIT {
            bail!("vault transaction journal would exceed its size limit");
        }
        write_atomic_text(&self.journal_path(&journal.target.target_key), &text)
            .context("failed to persist the vault transaction journal")?;
        Ok(sha256_hex(text.as_bytes()))
    }

    /// Whether a journal entry exists for one target, without reading it.
    pub(crate) fn journal_exists(&self, target_key: &str) -> bool {
        fs::symlink_metadata(self.journal_path(target_key)).is_ok()
    }

    pub(crate) fn remove_journal(&self, target_key: &str) -> AnyResult<()> {
        let path = self.journal_path(target_key);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).context("failed to remove the vault transaction journal");
            }
        }
        sync_parent_dir(&self.root.join(JOURNALS_DIR))
    }

    /// Locks one final target path. Taken before any home or ID lock.
    pub(crate) fn lock_target(&self, target_key: &str) -> AnyResult<HeldLock> {
        HeldLock::acquire(
            self.root
                .join(LOCKS_DIR)
                .join(format!("target-{target_key}.lock")),
        )
    }

    /// Locks one vault ID, shared by every same-ID copy. Taken after the
    /// target and home locks.
    pub(crate) fn lock_id(&self, vault_id: &str) -> AnyResult<HeldLock> {
        record::validate_vault_id(vault_id)?;
        HeldLock::acquire(
            self.root
                .join(LOCKS_DIR)
                .join(format!("id-{}.lock", id_key(vault_id))),
        )
    }
}

/// An exclusive lock held by this thread. Re-acquiring a lock this thread
/// already holds only adds a reference, so nested operations never deadlock
/// on their own descriptor.
pub(crate) struct HeldLock {
    path: PathBuf,
}

thread_local! {
    static HELD_LOCKS: RefCell<BTreeMap<PathBuf, (usize, Rc<File>)>> =
        const { RefCell::new(BTreeMap::new()) };
}

impl HeldLock {
    fn acquire(path: PathBuf) -> AnyResult<Self> {
        let reentered = HELD_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            held.get_mut(&path).map(|(count, _)| *count += 1).is_some()
        });
        if reentered {
            return Ok(Self { path });
        }
        let file = private_open_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .context("failed to open a vault witness lock")?;
        crate::acl::clear_file(&file, &path)?;
        lock_file(&file)?;
        HELD_LOCKS.with(|held| held.borrow_mut().insert(path.clone(), (1, Rc::new(file))));
        Ok(Self { path })
    }
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        HELD_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            if let Some((count, _)) = held.get_mut(&self.path) {
                *count -= 1;
                if *count == 0
                    && let Some((_, file)) = held.remove(&self.path)
                {
                    let _ = FileExt::unlock(file.as_ref());
                }
            }
        });
    }
}

/// Reads a witness file without following symlinks, requiring a regular,
/// owner-only file owned by the current user within `max_len` bytes.
fn read_protected(path: &Path, max_len: u64) -> AnyResult<Option<Vec<u8>>> {
    let mut file = match private_open_options().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).context("failed to open a protected vault witness file");
        }
    };
    let metadata = file
        .metadata()
        .context("failed to inspect a protected vault witness file")?;
    if !metadata.is_file() {
        bail!("protected vault witness entry is not a regular file");
    }
    #[cfg(unix)]
    {
        if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
            bail!("protected vault witness file is not private to the current user");
        }
    }
    if metadata.len() > max_len {
        bail!("protected vault witness file exceeds its size limit");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(max_len + 1)
        .read_to_end(&mut bytes)
        .context("failed to read a protected vault witness file")?;
    if bytes.len() as u64 > max_len {
        bail!("protected vault witness file grew beyond its size limit");
    }
    Ok(Some(bytes))
}

/// Creates or tightens a private witness directory and clears inherited ACL
/// entries before anything is written in it.
fn prepare_private_dir(path: &Path) -> AnyResult<PathBuf> {
    if path_is_symlink(path)? {
        bail!(
            "vault witness directory {} must not be a symlink",
            path.display()
        );
    }
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve vault witness {}", path.display()))?;
    ensure_owned(&canonical)?;
    ensure_private_dir_permissions(&canonical)?;
    crate::acl::clear_directory(&canonical)?;
    if path_is_symlink(&canonical)? {
        bail!(
            "vault witness directory {} became a symlink",
            canonical.display()
        );
    }
    Ok(canonical)
}

/// Validates an existing witness root without changing it.
fn validate_existing_private_dir(path: &Path) -> AnyResult<PathBuf> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect vault witness {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("vault witness {} must be a real directory", path.display());
    }
    #[cfg(unix)]
    if metadata.mode() & 0o077 != 0 {
        bail!(
            "vault witness {} must be private to the current user",
            path.display()
        );
    }
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve vault witness {}", path.display()))?;
    ensure_owned(&canonical)?;
    Ok(canonical)
}

fn ensure_owned(path: &Path) -> AnyResult<()> {
    #[cfg(unix)]
    {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if metadata.uid() != unsafe { libc::geteuid() } {
            bail!(
                "vault witness {} is not owned by the current user",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Canonicalizes the longest existing ancestor and appends the rest, so
/// containment checks compare physical paths even before creation.
fn resolve_existing_prefix(path: &Path) -> AnyResult<PathBuf> {
    let physical = crate::path_security::physical_path(path, "vault witness")?;
    let mut suffix = Vec::new();
    let mut current = physical.as_path();
    loop {
        match fs::canonicalize(current) {
            Ok(canonical) => {
                let mut resolved = canonical;
                for component in suffix.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = current.file_name() else {
                    return Ok(physical);
                };
                suffix.push(name.to_owned());
                let Some(parent) = current.parent() else {
                    return Ok(physical);
                };
                current = parent;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to resolve {}", current.display()));
            }
        }
    }
}

fn domain_key(domain: &[u8], value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update([0]);
    hasher.update(value);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.to_string_lossy().into_owned().into_bytes())
}

/// Device and inode of a directory, used to bind journals to identities.
pub(crate) fn directory_identity(path: &Path) -> AnyResult<(u64, u64)> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{} must be a real directory", path.display());
    }
    #[cfg(unix)]
    {
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        Ok((0, 0))
    }
}

#[cfg(test)]
mod tests;
