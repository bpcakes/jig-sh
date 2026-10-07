use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use fs4::fs_std::FileExt;
use zeroize::Zeroizing;

use crate::crypto::KdfParams;
use crate::{Result, VaultError, VaultErrorKind, VaultHomeState};

pub(crate) mod durable;
mod existing;
mod header;
mod locking;
mod pending;
pub(crate) mod witness;

pub(crate) use durable::ensure_entry_chain_durable;
use durable::{create_dir_all_durable, sync_dir as sync_parent_dir, sync_file};
pub(crate) use pending::{pending_transaction_marked, pending_transaction_recorded};
use witness::WitnessLocation;

const VAULT_HOME_ENV: &str = "JIG_VAULT_HOME";
const VAULT_FILE: &str = "vault.json";
const LOCK_FILE: &str = "vault.lock";
const AUDIT_FILE: &str = "audit.jsonl";
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(100);
pub(crate) const VAULT_TEXT_READ_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const AUDIT_TEXT_READ_LIMIT: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct VaultStore {
    root: PathBuf,
    initialization_kdf: KdfParams,
    witness: WitnessLocation,
    #[cfg(test)]
    fail_next_vault_write: Arc<AtomicBool>,
    #[cfg(test)]
    audit_text_read_limit: u64,
}

#[cfg(any(test, feature = "test-utils"))]
thread_local! {
    /// Armed per test thread so stores created internally, such as restore
    /// staging, share the injected crash point.
    static ARMED_FAULT: std::cell::Cell<Option<FaultPoint>> = const { std::cell::Cell::new(None) };
}

/// Simulates a crash at `point` when a test armed it on this thread.
/// Production builds compile this to a no-op.
pub(crate) fn fault(point: FaultPoint) -> AnyResult<()> {
    #[cfg(any(test, feature = "test-utils"))]
    if ARMED_FAULT.with(|armed| armed.get()) == Some(point) {
        ARMED_FAULT.with(|armed| armed.set(None));
        bail!("injected crash at {point:?}");
    }
    #[cfg(not(any(test, feature = "test-utils")))]
    let _ = point;
    Ok(())
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) fn arm_fault_for_test(point: FaultPoint) {
    ARMED_FAULT.with(|armed| armed.set(Some(point)));
}

/// Deterministic crash points of the transaction protocol, armed by tests.
#[cfg_attr(not(any(test, feature = "test-utils")), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultPoint {
    BeforeJournal,
    AfterJournal,
    AfterPending,
    PartialAudit,
    AfterAudit,
    AfterEnvelope,
    AfterPromotion,
}

impl VaultStore {
    pub(crate) fn resolve(explicit_home: Option<PathBuf>) -> Result<Self> {
        Self::resolve_inner(explicit_home, KdfParams::production())
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn resolve_for_test(explicit_home: Option<PathBuf>) -> Result<Self> {
        Self::resolve_inner(explicit_home, KdfParams::for_tests())
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))
    }

    fn resolve_inner(
        explicit_home: Option<PathBuf>,
        initialization_kdf: KdfParams,
    ) -> AnyResult<Self> {
        let root = resolve_root(explicit_home)?;
        if let Some(store) = pending::pending_absent_target(&root, &initialization_kdf)? {
            return Ok(store);
        }
        prepare_private_dir(root, initialization_kdf)
    }

    pub(crate) fn inspect(explicit_home: Option<PathBuf>) -> Result<(PathBuf, VaultHomeState)> {
        let root = resolve_root(explicit_home)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))?;
        let home_state = inspect_home_state(&root)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))?;
        Ok((root, home_state))
    }

    /// Unauthenticated format version from an initialized home's public
    /// header; see [`header::public_format_version`].
    pub(crate) fn public_format_version(root: &Path) -> Option<u32> {
        header::public_format_version(root)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Builds a store for a validated physical home whose witness location
    /// has already been checked to be disjoint from it.
    fn at(root: PathBuf, initialization_kdf: KdfParams, witness: WitnessLocation) -> Self {
        Self {
            root,
            initialization_kdf,
            witness,
            #[cfg(test)]
            fail_next_vault_write: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            audit_text_read_limit: AUDIT_TEXT_READ_LIMIT,
        }
    }

    pub(crate) fn witness(&self) -> &WitnessLocation {
        &self.witness
    }

    pub(crate) fn audit_text_read_limit(&self) -> u64 {
        #[cfg(test)]
        return self.audit_text_read_limit;
        #[cfg(not(test))]
        AUDIT_TEXT_READ_LIMIT
    }

    #[cfg(test)]
    pub(crate) fn set_audit_text_read_limit_for_test(&mut self, limit: u64) {
        self.audit_text_read_limit = limit;
    }

    /// Key of this home as a final target in the witness.
    pub(crate) fn target_key(&self) -> String {
        witness::target_key(&self.root)
    }

    /// Unauthenticated vault ID from the public header, used only to pick
    /// which ID lock to take. Authentication happens after locking.
    pub(crate) fn header_vault_id_for_lock(&self) -> Option<String> {
        header::public_vault_id(&self.root)
    }

    #[cfg(test)]
    pub(crate) fn arm_fault_for_test(&self, point: FaultPoint) {
        arm_fault_for_test(point);
    }

    pub(crate) fn fault(&self, point: FaultPoint) -> AnyResult<()> {
        fault(point)
    }

    pub(crate) fn initialization_kdf(&self) -> &KdfParams {
        &self.initialization_kdf
    }

    /// Used under the target lock after recovery has ruled out a pending
    /// restore. Reapply normal home privacy and durability before init.
    pub(crate) fn prepare_init_home(&self) -> AnyResult<Self> {
        prepare_private_dir(self.root.clone(), self.initialization_kdf.clone())
    }

    pub(crate) fn vault_path(&self) -> PathBuf {
        self.root.join(VAULT_FILE)
    }

    pub(crate) fn audit_path(&self) -> PathBuf {
        self.root.join(AUDIT_FILE)
    }

    pub(crate) fn validate_external_output(
        &self,
        output: &Path,
        operation_label: &str,
    ) -> AnyResult<()> {
        let file_name = output
            .file_name()
            .with_context(|| format!("{operation_label} output path must name a file"))?;
        let parent = match output.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let canonical_parent = fs::canonicalize(parent).with_context(|| {
            format!(
                "failed to canonicalize {operation_label} output parent {}",
                parent.display()
            )
        })?;
        let normalized_output = canonical_parent.join(file_name);
        if normalized_output.starts_with(self.root()) {
            bail!("{operation_label} output must be outside the source vault home");
        }
        // The witness holds the checkpoints and recovery journals an output
        // must never replace, whether by path or through a hard link.
        if self.witness.contains(&normalized_output)? {
            bail!("{operation_label} output must be outside the vault rollback witness");
        }

        #[cfg(unix)]
        if let Ok(output_metadata) = fs::metadata(&normalized_output) {
            for source in [self.vault_path(), self.audit_path()] {
                // A state file a pending transaction has not installed yet
                // cannot be aliased.
                let source_metadata = match fs::metadata(&source) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    result => result.with_context(|| {
                        format!(
                            "failed to inspect {operation_label} source {}",
                            source.display()
                        )
                    })?,
                };
                if output_metadata.dev() == source_metadata.dev()
                    && output_metadata.ino() == source_metadata.ino()
                {
                    bail!("{operation_label} output must not alias a source vault file");
                }
            }
            if self.witness.aliases_protected_file(&output_metadata)? {
                bail!("{operation_label} output must not alias a vault rollback witness file");
            }
        }
        Ok(())
    }

    pub(crate) fn exists(&self) -> Result<bool> {
        text_file_exists_no_follow(&self.vault_path())
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))
    }

    pub(crate) fn audit_exists(&self) -> Result<bool> {
        text_file_exists_no_follow(&self.audit_path())
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))
    }

    pub(crate) fn read_vault_text(&self) -> AnyResult<Option<String>> {
        read_text_no_follow(&self.vault_path(), VAULT_TEXT_READ_LIMIT)
    }

    pub(crate) fn read_vault_bytes(&self) -> AnyResult<Option<Zeroizing<Vec<u8>>>> {
        read_bytes_no_follow(&self.vault_path(), VAULT_TEXT_READ_LIMIT, "vault state")
    }

    #[cfg(test)]
    pub(crate) fn write_vault_text(&self, contents: &str) -> AnyResult<()> {
        self.with_lock(|| self.write_vault_text_unlocked(contents))
    }

    pub(crate) fn write_vault_text_unlocked(&self, contents: &str) -> AnyResult<()> {
        self.validate_vault_text_len(contents)?;
        #[cfg(test)]
        if self.fail_next_vault_write.swap(false, Ordering::SeqCst) {
            bail!("injected vault state write failure");
        }
        write_atomic_text(&self.vault_path(), contents)
    }

    pub(crate) fn validate_vault_text_len(&self, contents: &str) -> AnyResult<()> {
        if contents.len() > VAULT_TEXT_READ_LIMIT as usize {
            bail!(
                "vault state is {} bytes, exceeding the {VAULT_TEXT_READ_LIMIT} byte persistent vault limit",
                contents.len()
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fail_next_vault_write_for_test(&self) {
        self.fail_next_vault_write.store(true, Ordering::SeqCst);
    }

    pub(crate) fn append_audit_line_unlocked(&self, line: &str) -> AnyResult<()> {
        let path = self.audit_path();
        let mut file = private_open_options()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)
            .with_context(|| format!("failed to open vault audit log {}", path.display()))?;
        crate::acl::clear_file(&file, &path)?;
        if file
            .metadata()
            .with_context(|| format!("failed to stat vault audit log {}", path.display()))?
            .len()
            > 0
        {
            file.seek(SeekFrom::End(-1)).with_context(|| {
                format!(
                    "failed to seek to end of vault audit log {}",
                    path.display()
                )
            })?;
            let mut last = [0_u8; 1];
            file.read_exact(&mut last).with_context(|| {
                format!(
                    "failed to read final byte of vault audit log {}",
                    path.display()
                )
            })?;
            if last[0] != b'\n' {
                // Torn tails are truncated by the verifier before append; this
                // preserves a complete final event that only lacks a line
                // terminator.
                file.write_all(b"\n").with_context(|| {
                    format!(
                        "failed to terminate final vault audit log line {}",
                        path.display()
                    )
                })?;
            }
        }
        file.write_all(line.as_bytes())
            .with_context(|| format!("failed to write vault audit event to {}", path.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to finish vault audit event in {}", path.display()))?;
        sync_file(&file, &path)?;
        if let Some(parent) = path.parent() {
            sync_parent_dir(parent)?;
        }
        Ok(())
    }

    /// Appends exact bytes to the audit log, creating a private log when it
    /// is absent, and syncs the file and its directory.
    pub(crate) fn append_audit_bytes_unlocked(&self, bytes: &[u8]) -> AnyResult<()> {
        let path = self.audit_path();
        let mut file = private_open_options()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open vault audit log {}", path.display()))?;
        crate::acl::clear_file(&file, &path)?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write vault audit log {}", path.display()))?;
        sync_file(&file, &path)?;
        sync_parent_dir(&self.root)
    }

    /// Re-establishes the durability barrier of a state file whose bytes a
    /// recovery step found already in place: a crash may have preceded the
    /// original sync.
    pub(crate) fn sync_state_file_unlocked(&self, path: &Path) -> AnyResult<()> {
        let file = private_open_options()
            .read(true)
            .open(path)
            .with_context(|| format!("failed to open {} for sync", path.display()))?;
        sync_file(&file, path)?;
        sync_parent_dir(&self.root)
    }

    /// Makes durable the state a transaction is about to bind: the audit log
    /// (its verified prefix and any torn suffix) and the predecessor
    /// envelope. An interrupted earlier write may have left them visible but
    /// unsynced.
    pub(crate) fn sync_existing_state_unlocked(&self) -> AnyResult<()> {
        for path in [self.audit_path(), self.vault_path()] {
            match private_open_options().read(true).open(&path) {
                Ok(file) => sync_file(&file, &path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to open {} for sync", path.display()));
                }
            }
        }
        sync_parent_dir(&self.root)
    }

    pub(crate) fn truncate_audit_unlocked(&self, len: u64) -> AnyResult<()> {
        let path = self.audit_path();
        let file = private_open_options()
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open vault audit log {}", path.display()))?;
        crate::acl::clear_file(&file, &path)?;
        file.set_len(len)
            .with_context(|| format!("failed to truncate vault audit log {}", path.display()))?;
        sync_file(&file, &path)?;
        if let Some(parent) = path.parent() {
            sync_parent_dir(parent)?;
        }
        Ok(())
    }

    pub(crate) fn read_audit_text(&self) -> AnyResult<Option<String>> {
        read_text_no_follow(&self.audit_path(), self.audit_text_read_limit())
    }

    pub(crate) fn read_audit_bytes_bounded(
        &self,
        max_len: usize,
    ) -> AnyResult<Option<Zeroizing<Vec<u8>>>> {
        let max_len = u64::try_from(max_len).context("vault audit read limit overflow")?;
        read_bytes_no_follow(&self.audit_path(), max_len, "vault audit log")
    }

    pub(crate) fn audit_len(&self) -> AnyResult<Option<u64>> {
        regular_file_len_no_follow(&self.audit_path())
    }
}

fn inspect_home_state(root: &Path) -> AnyResult<VaultHomeState> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "Vault home {} must not be a symlink. Use a dedicated real directory.",
                root.display()
            )
        }
        Ok(_) => {
            if text_file_exists_no_follow(&root.join(VAULT_FILE))? {
                Ok(VaultHomeState::Initialized)
            } else {
                Ok(VaultHomeState::Uninitialized)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(VaultHomeState::Absent),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", root.display())),
    }
}

fn text_file_exists_no_follow(path: &Path) -> AnyResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to inspect symlinked vault file {}",
                path.display()
            )
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

fn resolve_root(explicit_home: Option<PathBuf>) -> AnyResult<PathBuf> {
    let root = match explicit_home {
        Some(path) => path,
        None => match std::env::var(VAULT_HOME_ENV) {
            Ok(value) if value.is_empty() => bail!("{VAULT_HOME_ENV} must not be empty"),
            Ok(value) => PathBuf::from(value),
            Err(std::env::VarError::NotPresent) => dirs::home_dir()
                .context("could not resolve home directory for Jig vault")?
                .join(".jig/vault"),
            Err(std::env::VarError::NotUnicode(value)) => {
                bail!(
                    "{VAULT_HOME_ENV} must be valid Unicode: {}",
                    value.to_string_lossy()
                )
            }
        },
    };
    crate::path_security::physical_path(&root, "vault home")
}

fn prepare_private_dir(root: PathBuf, initialization_kdf: KdfParams) -> AnyResult<VaultStore> {
    // Refuse an overlapping witness before creating anything.
    WitnessLocation::for_home(&root)?.ensure_disjoint(&root)?;
    if path_is_symlink(&root)? {
        bail!(
            "Vault home {} must not be a symlink. Use a dedicated real directory.",
            root.display()
        );
    }
    ensure_create_base_is_not_symlink(&root)?;
    ensure_create_ancestor_is_not_shared_writable(&root)?;
    create_dir_all_durable(&root)
        .with_context(|| format!("failed to create vault home {}", root.display()))?;
    if path_is_symlink(&root)? {
        bail!(
            "Vault home {} became a symlink while being prepared.",
            root.display()
        );
    }
    let root = fs::canonicalize(&root)
        .with_context(|| format!("failed to canonicalize vault home {}", root.display()))?;
    ensure_tree_has_no_symlinks(&root, &root)?;
    ensure_private_dir_permissions(&root)?;
    // Darwin ACL entries, including inherited ones, bypass the owner-only
    // mode; clear them before any state file here can inherit them.
    crate::acl::clear_directory(&root)?;
    // Re-walk after chmod so a same-user directory-entry race cannot trade a
    // checked file for a symlink while permissions are being tightened.
    ensure_tree_has_no_symlinks(&root, &root)?;
    let witness = WitnessLocation::for_home(&root)?;
    witness.ensure_disjoint(&root)?;
    Ok(VaultStore::at(root, initialization_kdf, witness))
}

fn lock_file(file: &File) -> AnyResult<()> {
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(()),
            Ok(false) => {
                if Instant::now() >= deadline {
                    bail!("timed out waiting for vault lock after {LOCK_TIMEOUT:?}");
                }
                std::thread::sleep(LOCK_POLL_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn read_text_no_follow(path: &Path, max_len: u64) -> AnyResult<Option<String>> {
    let mut file = match private_open_options().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            match path_is_symlink(path) {
                Ok(true) => bail!("refusing to read symlinked vault file {}", path.display()),
                Ok(false) => {}
                Err(inspect_error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to open {}; additionally failed to inspect symlink status: {inspect_error:#}",
                            path.display()
                        )
                    });
                }
            }
            return Err(error).with_context(|| format!("failed to open {}", path.display()));
        }
    };
    let len = file
        .metadata()
        .with_context(|| format!("failed to stat {}", path.display()))?
        .len();
    if len > max_len {
        bail!(
            "{} is larger than the {} byte read limit",
            path.display(),
            max_len
        );
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(Some(text))
}

fn read_bytes_no_follow(
    path: &Path,
    max_len: u64,
    label: &str,
) -> AnyResult<Option<Zeroizing<Vec<u8>>>> {
    let mut file = match private_open_options().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to open {label} {}", path.display()));
        }
    };
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to stat {label} {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{label} is not a regular file: {}", path.display());
    }
    let len = metadata.len();
    if len > max_len {
        bail!(
            "{label} is {len} bytes, exceeding the {max_len} byte read limit at {}",
            path.display()
        );
    }
    let capacity = usize::try_from(len).context("protected file length exceeds address space")?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    Read::by_ref(&mut file)
        .take(max_len.saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {label} {}", path.display()))?;
    if bytes.len() as u64 > max_len {
        bail!(
            "{label} grew beyond the {max_len} byte read limit while reading {}",
            path.display()
        );
    }
    Ok(Some(bytes))
}

fn regular_file_len_no_follow(path: &Path) -> AnyResult<Option<u64>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to inspect symlinked protected file {}",
                path.display()
            )
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!("protected path is not a regular file: {}", path.display())
        }
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

fn write_atomic_text(path: &Path, contents: &str) -> AnyResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("vault file path has no parent: {}", path.display()))?;
    let tmp_name = format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("vault"),
        std::process::id(),
        ulid::Ulid::new()
    );
    let tmp_path = parent.join(tmp_name);
    let mut file = private_open_options()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .with_context(|| format!("failed to create temp vault file {}", tmp_path.display()))?;
    let result = (|| -> AnyResult<()> {
        crate::acl::clear_file(&file, &tmp_path)?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write temp vault file {}", tmp_path.display()))?;
        sync_file(&file, &tmp_path)?;
        drop(file);
        durable::rename(&tmp_path, path).with_context(|| {
            format!(
                "failed to replace vault file {} from {}",
                path.display(),
                tmp_path.display()
            )
        })?;
        sync_parent_dir(parent)?;
        Ok(())
    })();
    if result.is_err() {
        // Best-effort cleanup: preserve the original write or rename error.
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

#[cfg(unix)]
fn private_open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    options
}

#[cfg(not(unix))]
fn private_open_options() -> OpenOptions {
    OpenOptions::new()
}

fn path_is_symlink(path: &Path) -> AnyResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}

fn ensure_create_base_is_not_symlink(path: &Path) -> AnyResult<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "Vault home creation base {} must not be a symlink. Use a dedicated real directory.",
                    ancestor.display()
                );
            }
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", ancestor.display()));
            }
        }
    }
    Ok(())
}

fn ensure_tree_has_no_symlinks(root: &Path, path: &Path) -> AnyResult<()> {
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry =
            entry.with_context(|| format!("failed to read entry below {}", path.display()))?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?;
        if metadata.file_type().is_symlink() {
            bail!(
                "Vault home {} contains symlink {}. Use a dedicated state directory without symlinks.",
                root.display(),
                entry_path.display()
            );
        }
        if metadata.is_dir() {
            ensure_tree_has_no_symlinks(root, &entry_path)?;
        }
    }
    Ok(())
}

fn ensure_private_dir_permissions(path: &Path) -> AnyResult<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!("failed to set vault home permissions on {}", path.display())
        })?;
        let mode = fs::metadata(path)
            .with_context(|| {
                format!(
                    "failed to inspect vault home permissions on {}",
                    path.display()
                )
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            bail!(
                "vault home permissions are {:o}; expected 700 for {}",
                mode,
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn ensure_create_ancestor_is_not_shared_writable(path: &Path) -> AnyResult<()> {
    #[cfg(unix)]
    {
        // This checks the first existing ancestor that would own creation of
        // the vault home. Higher ancestors are outside the directory-entry
        // boundary this local state store can harden.
        for ancestor in path.ancestors().skip(1) {
            let metadata = match fs::metadata(ancestor) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to inspect {}", ancestor.display()));
                }
            };
            if !metadata.is_dir() {
                continue;
            }
            let mode = metadata.permissions().mode() & 0o777;
            if mode & 0o002 != 0 && mode & 0o1000 == 0 {
                bail!(
                    "refusing to create vault home below shared-writable ancestor {}",
                    ancestor.display()
                );
            }
            crate::acl::reject_shared_write(ancestor)?;
            break;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests;
