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

mod existing;

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
    #[cfg(test)]
    fail_next_vault_write: Arc<AtomicBool>,
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
        prepare_private_dir(root, initialization_kdf)
    }

    pub(crate) fn inspect(explicit_home: Option<PathBuf>) -> Result<(PathBuf, VaultHomeState)> {
        let root = resolve_root(explicit_home)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))?;
        let home_state = inspect_home_state(&root)
            .map_err(|error| VaultError::from_anyhow(VaultErrorKind::Io, error))?;
        Ok((root, home_state))
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn initialization_kdf(&self) -> &KdfParams {
        &self.initialization_kdf
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

        #[cfg(unix)]
        if let Ok(output_metadata) = fs::metadata(&normalized_output) {
            for source in [self.vault_path(), self.audit_path()] {
                let source_metadata = fs::metadata(&source).with_context(|| {
                    format!(
                        "failed to inspect {operation_label} source {}",
                        source.display()
                    )
                })?;
                if output_metadata.dev() == source_metadata.dev()
                    && output_metadata.ino() == source_metadata.ino()
                {
                    bail!("{operation_label} output must not alias a source vault file");
                }
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
        file.sync_all()
            .with_context(|| format!("failed to sync vault audit log {}", path.display()))?;
        if let Some(parent) = path.parent() {
            sync_parent_dir(parent)?;
        }
        Ok(())
    }

    pub(crate) fn truncate_audit_unlocked(&self, len: u64) -> AnyResult<()> {
        let path = self.audit_path();
        let file = private_open_options()
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open vault audit log {}", path.display()))?;
        file.set_len(len)
            .with_context(|| format!("failed to truncate vault audit log {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync vault audit log {}", path.display()))?;
        if let Some(parent) = path.parent() {
            sync_parent_dir(parent)?;
        }
        Ok(())
    }

    pub(crate) fn read_audit_text(&self) -> AnyResult<Option<String>> {
        read_text_no_follow(&self.audit_path(), AUDIT_TEXT_READ_LIMIT)
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

    fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    pub(crate) fn with_lock<T>(&self, f: impl FnOnce() -> AnyResult<T>) -> AnyResult<T> {
        let file = private_open_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())
            .context("failed to open vault lock")?;
        lock_file(&file)?;
        let result = f();
        let unlock = FileExt::unlock(&file);
        match (result, unlock) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(error)) => Err(error).context("failed to unlock vault lock"),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(unlock_error)) => Err(error.context(format!(
                "vault operation failed; additionally failed to unlock vault lock: {unlock_error}"
            ))),
        }
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
    if path_is_symlink(&root)? {
        bail!(
            "Vault home {} must not be a symlink. Use a dedicated real directory.",
            root.display()
        );
    }
    ensure_create_base_is_not_symlink(&root)?;
    ensure_create_ancestor_is_not_shared_writable(&root)?;
    fs::create_dir_all(&root)
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
    // Re-walk after chmod so a same-user directory-entry race cannot trade a
    // checked file for a symlink while permissions are being tightened.
    ensure_tree_has_no_symlinks(&root, &root)?;
    Ok(VaultStore {
        root,
        initialization_kdf,
        #[cfg(test)]
        fail_next_vault_write: Arc::new(AtomicBool::new(false)),
    })
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
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write temp vault file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp vault file {}", tmp_path.display()))?;
        drop(file);
        fs::rename(&tmp_path, path).with_context(|| {
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

fn sync_parent_dir(path: &Path) -> AnyResult<()> {
    #[cfg(unix)]
    {
        let dir = File::open(path)
            .with_context(|| format!("failed to open parent directory {}", path.display()))?;
        dir.sync_all()
            .with_context(|| format!("failed to sync parent directory {}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
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
            break;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_creates_private_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        assert!(store.root().is_dir());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.root()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolve_uses_the_verified_physical_macos_temp_path() {
        let temp = tempfile::Builder::new()
            .prefix("jig-vault-path-")
            .tempdir_in("/tmp")
            .unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let logical_root = temp.path().join("vault");

        let store = VaultStore::resolve_for_test(Some(logical_root.clone())).unwrap();

        assert!(store.root().starts_with("/private/tmp"));
        assert_eq!(store.root(), fs::canonicalize(logical_root).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_home() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = VaultStore::resolve_for_test(Some(link))
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not be a symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_ancestor() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = VaultStore::resolve_for_test(Some(link.join("vault")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("creation base"));
    }

    #[test]
    fn resolve_rejects_regular_file_home() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("vault");
        fs::write(&home, "not a directory").unwrap();
        let error = VaultStore::resolve_for_test(Some(home))
            .unwrap_err()
            .to_string();
        assert!(error.contains("failed to create vault home"));
    }

    #[cfg(unix)]
    #[test]
    fn exists_refuses_symlinked_vault_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let target = temp.path().join("outside-vault.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, store.vault_path()).unwrap();

        assert!(store.exists().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn read_refuses_symlinked_vault_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let target = temp.path().join("outside-vault.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, store.vault_path()).unwrap();

        let error = store.read_vault_text().unwrap_err().to_string();
        assert!(error.contains("refusing to read symlinked vault file"));
    }

    #[test]
    fn read_rejects_oversized_vault_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let file = File::create(store.vault_path()).unwrap();
        file.set_len(VAULT_TEXT_READ_LIMIT + 1).unwrap();

        let error = store.read_vault_text().unwrap_err().to_string();

        assert!(error.contains("read limit"));
    }

    #[test]
    fn resolve_rejects_empty_env_home() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::env::var_os(VAULT_HOME_ENV);
        unsafe {
            std::env::set_var(VAULT_HOME_ENV, "");
        }
        let error = VaultStore::resolve_for_test(None).unwrap_err().to_string();
        unsafe {
            if let Some(previous) = previous {
                std::env::set_var(VAULT_HOME_ENV, previous);
            } else {
                std::env::remove_var(VAULT_HOME_ENV);
            }
        }
        assert!(error.contains("must not be empty"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_non_utf8_env_home() {
        use std::os::unix::ffi::OsStringExt;

        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::env::var_os(VAULT_HOME_ENV);
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(PathBuf::from(std::ffi::OsString::from_vec(
            b"vault-\xff".to_vec(),
        )));
        unsafe {
            std::env::set_var(VAULT_HOME_ENV, home.as_os_str());
        }
        let result = VaultStore::resolve_for_test(None);
        unsafe {
            if let Some(previous) = previous {
                std::env::set_var(VAULT_HOME_ENV, previous);
            } else {
                std::env::remove_var(VAULT_HOME_ENV);
            }
        }

        let error = result.unwrap_err().to_string();
        assert!(error.contains("must be valid Unicode"));
    }
}
