use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use fs4::fs_std::FileExt;
use ulid::Ulid;

#[cfg(test)]
pub(crate) const INSTALLER_CACHE_LOCK_PROTOCOL_MARKER: &str = "directory-suffix=.lock;guard-suffix=.lock.guard;mechanism=os-exclusive+legacy-directory;record=owner-v1;attempts=30;retry-seconds=1";
const LOCK_DIRECTORY_SUFFIX: &str = ".lock";
const LOCK_GUARD_SUFFIX: &str = ".lock.guard";
#[cfg(test)]
const LOCK_MECHANISM: &str = "os-exclusive+legacy-directory";
const LOCK_RECORD: &str = "owner-v1";
const LOCK_OWNER_FILE: &str = "owner-v1";
const LOCK_ATTEMPTS: u32 = 30;
const LOCK_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeCacheLockPolicy {
    attempts: u32,
    retry_delay: Duration,
}

impl RuntimeCacheLockPolicy {
    pub(crate) const INSTALLER: Self = Self {
        attempts: LOCK_ATTEMPTS,
        retry_delay: LOCK_RETRY_DELAY,
    };

    #[cfg(test)]
    pub(crate) const fn immediate() -> Self {
        Self {
            attempts: 1,
            retry_delay: Duration::ZERO,
        }
    }

    pub(crate) const fn retirement() -> Self {
        Self {
            attempts: 3,
            retry_delay: Duration::from_secs(1),
        }
    }
}

#[derive(Debug)]
struct HeldRuntimeCacheLock {
    paths: RuntimeCacheLockPaths,
    guard: File,
    owner_record: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RuntimeCacheLockPaths {
    directory: PathBuf,
    guard: PathBuf,
}

impl RuntimeCacheLockPaths {
    fn for_cache(cache: &Path) -> Self {
        Self {
            directory: path_with_suffix(cache, LOCK_DIRECTORY_SUFFIX),
            guard: path_with_suffix(cache, LOCK_GUARD_SUFFIX),
        }
    }
}

#[derive(Debug)]
pub(crate) struct RuntimeCacheLocks {
    locks: Vec<HeldRuntimeCacheLock>,
}

impl RuntimeCacheLocks {
    pub(crate) fn acquire(cache_paths: &[PathBuf], policy: RuntimeCacheLockPolicy) -> Result<Self> {
        let paths = cache_paths
            .iter()
            .map(|path| RuntimeCacheLockPaths::for_cache(path))
            .collect::<BTreeSet<_>>();
        let mut held = Self {
            locks: Vec::with_capacity(paths.len()),
        };
        for paths in paths {
            held.acquire_one(paths, policy)?;
        }
        Ok(held)
    }

    #[cfg(test)]
    pub(crate) const fn empty() -> Self {
        Self { locks: Vec::new() }
    }

    fn acquire_one(
        &mut self,
        paths: RuntimeCacheLockPaths,
        policy: RuntimeCacheLockPolicy,
    ) -> Result<()> {
        let parent = paths.directory.parent().with_context(|| {
            format!(
                "Runtime cache lock has no parent: {}",
                paths.directory.display()
            )
        })?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create runtime cache lock parent {}",
                parent.display()
            )
        })?;
        for attempt in 1..=policy.attempts.max(1) {
            let mut guard = open_guard(&paths.guard).with_context(|| {
                format!(
                    "Failed to open Jig installer guard {}",
                    paths.guard.display()
                )
            })?;
            if FileExt::try_lock_exclusive(&guard).with_context(|| {
                format!(
                    "Failed to acquire Jig installer guard {}",
                    paths.guard.display()
                )
            })? {
                let owner_record =
                    format!("{LOCK_RECORD} {} {}\n", std::process::id(), Ulid::new());
                if claim_legacy_directory(&paths.directory, &owner_record)? {
                    if let Err(error) = write_guard_record(&mut guard, &owner_record) {
                        release_legacy_directory(&paths.directory, &owner_record);
                        let _ = FileExt::unlock(&guard);
                        return Err(error).with_context(|| {
                            format!("Failed to record lock owner in {}", paths.guard.display())
                        });
                    }
                    self.locks.push(HeldRuntimeCacheLock {
                        paths,
                        guard,
                        owner_record,
                    });
                    return Ok(());
                }
                let _ = FileExt::unlock(&guard);
            }
            if attempt < policy.attempts.max(1) {
                thread::sleep(policy.retry_delay);
            }
        }
        bail!(
            "Timed out waiting for Jig installer lock {}. Another scripts/jig install may still be running; remove an unmarked legacy lock directory manually only after confirming no installer is active, then retry",
            paths.directory.display(),
        )
    }
}

impl Drop for RuntimeCacheLocks {
    fn drop(&mut self) {
        for held in self.locks.drain(..).rev() {
            release_legacy_directory(&held.paths.directory, &held.owner_record);
            if let Err(error) = FileExt::unlock(&held.guard) {
                eprintln!(
                    "Warning: failed to release Jig installer guard {}: {error}",
                    held.paths.guard.display()
                );
            }
        }
    }
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut result = OsString::from(path.as_os_str());
    result.push(suffix);
    PathBuf::from(result)
}

fn open_guard(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    validate_guard_identity(path, &file)?;
    Ok(file)
}

fn validate_guard_identity(path: &Path, file: &File) -> std::io::Result<()> {
    let opened = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    if !opened.is_file() || !named.file_type().is_file() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "guard path is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != named.dev()
            || opened.ino() != named.ino()
            || opened.nlink() != 1
            || named.nlink() != 1
        {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "guard path is not a standalone regular file",
            ));
        }
    }
    Ok(())
}

fn write_guard_record(file: &mut File, record: &str) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(record.as_bytes())?;
    file.sync_data()
}

fn claim_legacy_directory(directory: &Path, owner_record: &str) -> Result<bool> {
    if !try_create_legacy_directory(directory)? {
        let owner = directory.join(LOCK_OWNER_FILE);
        let stale_owner_record = match fs::read(&owner) {
            Ok(record) if owner_record_is_valid(&record) => record,
            // A live legacy mkdir-only installer does not hold the OS guard,
            // so an unmarked directory cannot safely be distinguished from
            // one whose new owner crashed before writing its record.
            Ok(_) | Err(_) => return Ok(false),
        };
        match fs::remove_file(&owner) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Failed to recover stale lock owner {}", owner.display())
                });
            }
        }
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                let restore_error = fs::write(&owner, &stale_owner_record).err();
                return Err(error).with_context(|| {
                    let message = format!(
                        "Failed to recover stale installer lock {}",
                        directory.display()
                    );
                    match restore_error {
                        Some(restore_error) => format!(
                            "{message}; also failed to restore {}: {restore_error}",
                            owner.display()
                        ),
                        None => message,
                    }
                });
            }
        }
        if !try_create_legacy_directory(directory)? {
            return Ok(false);
        }
    }
    let owner = directory.join(LOCK_OWNER_FILE);
    if let Err(error) = fs::write(&owner, owner_record) {
        cleanup_failed_legacy_claim(directory, &owner);
        return Err(error)
            .with_context(|| format!("Failed to write lock owner {}", owner.display()));
    }
    Ok(true)
}

fn cleanup_failed_legacy_claim(directory: &Path, owner: &Path) {
    let _ = fs::remove_file(owner);
    let _ = fs::remove_dir(directory);
}

fn try_create_legacy_directory(directory: &Path) -> Result<bool> {
    match fs::create_dir(directory) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("Failed to claim installer lock {}", directory.display())),
    }
}

fn release_legacy_directory(directory: &Path, owner_record: &str) {
    let owner = directory.join(LOCK_OWNER_FILE);
    if !fs::read_to_string(&owner).is_ok_and(|record| record == owner_record) {
        return;
    }
    if let Err(error) = fs::remove_file(&owner) {
        eprintln!(
            "Warning: failed to remove Jig installer lock owner {}: {error}",
            owner.display()
        );
        return;
    }
    if let Err(error) = fs::remove_dir(directory) {
        if error.kind() != ErrorKind::NotFound {
            let restore_error = fs::write(&owner, owner_record).err();
            match restore_error {
                Some(restore_error) => eprintln!(
                    "Warning: failed to release legacy Jig installer lock {}: {error}; also failed to restore {}: {restore_error}",
                    directory.display(),
                    owner.display()
                ),
                None => eprintln!(
                    "Warning: failed to release legacy Jig installer lock {}: {error}; restored its reclaimable owner record",
                    directory.display()
                ),
            }
        }
    }
}

fn owner_record_is_valid(record: &[u8]) -> bool {
    let Ok(record) = std::str::from_utf8(record) else {
        return false;
    };
    let mut fields = record.split_whitespace();
    fields.next() == Some(LOCK_RECORD)
        && fields.next().is_some_and(|pid| pid.parse::<u32>().is_ok())
        && fields.next().is_some_and(|token| !token.is_empty())
        && fields.next().is_none()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    #[cfg(unix)]
    use std::process::{Child, Command};
    #[cfg(unix)]
    use std::time::Instant;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn protocol_marker_matches_policy() {
        assert_eq!(
            INSTALLER_CACHE_LOCK_PROTOCOL_MARKER,
            format!(
                "directory-suffix={LOCK_DIRECTORY_SUFFIX};guard-suffix={LOCK_GUARD_SUFFIX};mechanism={LOCK_MECHANISM};record={LOCK_RECORD};attempts={LOCK_ATTEMPTS};retry-seconds={}",
                LOCK_RETRY_DELAY.as_secs()
            )
        );
    }

    #[test]
    fn locks_are_ordered_deduplicated_and_released() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("contract-4");
        let second = temp.path().join("contract-4-runtime");
        let locks = RuntimeCacheLocks::acquire(
            &[second.clone(), first.clone(), second.clone()],
            RuntimeCacheLockPolicy::immediate(),
        )
        .unwrap();
        assert_eq!(
            locks
                .locks
                .iter()
                .map(|held| held.paths.directory.clone())
                .collect::<Vec<_>>(),
            [
                path_with_suffix(&second, ".lock"),
                path_with_suffix(&first, ".lock")
            ]
        );
        drop(locks);
        assert!(!path_with_suffix(&first, ".lock").exists());
        assert!(path_with_suffix(&first, ".lock.guard").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path_with_suffix(&first, ".lock.guard"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn guard_symlink_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let cache = temp.path().join("contract-4");
        let guard = path_with_suffix(&cache, LOCK_GUARD_SUFFIX);
        let target = temp.path().join("guard-target");
        fs::write(&target, "preserve me\n").unwrap();
        symlink(&target, &guard).unwrap();

        let error = RuntimeCacheLocks::acquire(
            std::slice::from_ref(&cache),
            RuntimeCacheLockPolicy::immediate(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to open Jig installer guard")
        );
        assert_eq!(fs::read_to_string(target).unwrap(), "preserve me\n");
    }

    #[test]
    fn live_owner_is_never_reclaimed_by_age() {
        let temp = tempdir().unwrap();
        let cache = temp.path().join("contract-4");
        let owner = RuntimeCacheLocks::acquire(
            std::slice::from_ref(&cache),
            RuntimeCacheLockPolicy::immediate(),
        )
        .unwrap();
        assert!(
            RuntimeCacheLocks::acquire(
                std::slice::from_ref(&cache),
                RuntimeCacheLockPolicy::immediate()
            )
            .is_err()
        );
        drop(owner);
        RuntimeCacheLocks::acquire(&[cache], RuntimeCacheLockPolicy::immediate()).unwrap();
    }

    #[test]
    fn legacy_directory_claim_treats_a_racing_owner_as_contention() {
        let temp = tempdir().unwrap();
        let directory = temp.path().join("contract-4.lock");
        fs::create_dir(&directory).unwrap();

        assert!(!try_create_legacy_directory(&directory).unwrap());
    }

    #[test]
    fn failed_legacy_claim_cleanup_removes_a_partial_owner_record() {
        let temp = tempdir().unwrap();
        let directory = temp.path().join("contract-4.lock");
        let owner = directory.join(LOCK_OWNER_FILE);
        fs::create_dir(&directory).unwrap();
        fs::write(&owner, b"partial").unwrap();

        cleanup_failed_legacy_claim(&directory, &owner);

        assert!(!directory.exists());
    }

    #[test]
    fn failed_stale_recovery_restores_the_owner_record() {
        let temp = tempdir().unwrap();
        let directory = temp.path().join("contract-4.lock");
        let owner = directory.join(LOCK_OWNER_FILE);
        let owner_record = b"owner-v1 123 stale-owner\n";
        fs::create_dir(&directory).unwrap();
        fs::write(&owner, owner_record).unwrap();
        fs::write(directory.join("unexpected"), b"blocks rmdir").unwrap();

        assert!(claim_legacy_directory(&directory, "owner-v1 456 contender\n").is_err());
        assert_eq!(fs::read(owner).unwrap(), owner_record);
    }

    #[test]
    fn failed_release_restores_the_owner_record() {
        let temp = tempdir().unwrap();
        let directory = temp.path().join("contract-4.lock");
        let owner = directory.join(LOCK_OWNER_FILE);
        let owner_record = "owner-v1 123 live-owner\n";
        fs::create_dir(&directory).unwrap();
        fs::write(&owner, owner_record).unwrap();
        fs::write(directory.join("unexpected"), b"blocks rmdir").unwrap();

        release_legacy_directory(&directory, owner_record);

        assert_eq!(fs::read_to_string(owner).unwrap(), owner_record);
    }

    #[cfg(unix)]
    #[test]
    fn crashed_python_owner_is_recovered_after_its_guard_releases() {
        let temp = tempdir().unwrap();
        let cache = temp.path().join("contract-4");
        let ready = temp.path().join("ready");
        assert_python_owner_blocks_and_recovers(&cache, &ready);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_cache_path_preserves_guard_bytes() {
        let mut cache_name = b"contract-4-".to_vec();
        cache_name.push(0xff);
        let cache = PathBuf::from(OsString::from_vec(cache_name.clone()));
        let paths = RuntimeCacheLockPaths::for_cache(&cache);
        cache_name.extend_from_slice(b".lock.guard");

        assert_eq!(paths.guard.as_os_str().as_bytes(), cache_name);
    }

    #[cfg(all(unix, not(target_vendor = "apple")))]
    #[test]
    fn non_utf8_cache_path_preserves_cross_language_guard_identity() {
        let temp = tempdir().unwrap();
        let mut cache_name = b"contract-4-".to_vec();
        cache_name.push(0xff);
        let cache = temp.path().join(OsString::from_vec(cache_name));
        let ready = temp.path().join("ready");

        assert_python_owner_blocks_and_recovers(&cache, &ready);
    }

    #[cfg(unix)]
    fn assert_python_owner_blocks_and_recovers(cache: &Path, ready: &Path) {
        let cache = cache.to_path_buf();
        let mut owner = spawn_python_lock_owner(&cache, ready);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.is_file() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.is_file());
        assert!(
            RuntimeCacheLocks::acquire(
                std::slice::from_ref(&cache),
                RuntimeCacheLockPolicy::immediate(),
            )
            .is_err()
        );
        owner.kill().unwrap();
        owner.wait().unwrap();
        RuntimeCacheLocks::acquire(&[cache], RuntimeCacheLockPolicy::immediate()).unwrap();
    }

    #[cfg(unix)]
    fn spawn_python_lock_owner(cache: &Path, ready: &Path) -> Child {
        Command::new("python3")
            .args([
                "-c",
                r#"
import os, sys, time
cache, ready = sys.argv[1:]
if os.name == "nt":
    guard = cache + ".lock.guard"
    directory = cache + ".lock"
    owner_name = "owner-v1"
else:
    cache, ready = os.fsencode(cache), os.fsencode(ready)
    guard = cache + b".lock.guard"
    directory = cache + b".lock"
    owner_name = b"owner-v1"
descriptor = os.open(guard, os.O_RDWR | os.O_CREAT, 0o600)
if os.name == "nt":
    import msvcrt
    if os.fstat(descriptor).st_size == 0:
        os.write(descriptor, b"\0")
    os.lseek(descriptor, 0, os.SEEK_SET)
    msvcrt.locking(descriptor, msvcrt.LK_LOCK, 1)
else:
    import fcntl
    fcntl.flock(descriptor, fcntl.LOCK_EX)
os.mkdir(directory)
owner = os.open(os.path.join(directory, owner_name), os.O_WRONLY | os.O_CREAT, 0o600)
os.write(owner, f"owner-v1 {os.getpid()} test-owner\n".encode("ascii"))
os.close(owner)
ready_file = os.open(ready, os.O_WRONLY | os.O_CREAT, 0o600)
os.write(ready_file, b"ready\n")
os.close(ready_file)
while True: time.sleep(1)
"#,
            ])
            .args([cache, ready])
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn old_owner_cannot_release_successor_generation() {
        let temp = tempdir().unwrap();
        let cache = temp.path().join("contract-4");
        let directory = path_with_suffix(&cache, ".lock");
        let guard = path_with_suffix(&cache, ".lock.guard");
        let old = RuntimeCacheLocks::acquire(
            std::slice::from_ref(&cache),
            RuntimeCacheLockPolicy::immediate(),
        )
        .unwrap();
        fs::remove_file(directory.join("owner-v1")).unwrap();
        fs::remove_dir(&directory).unwrap();
        fs::remove_file(&guard).unwrap();
        let successor = RuntimeCacheLocks::acquire(
            std::slice::from_ref(&cache),
            RuntimeCacheLockPolicy::immediate(),
        )
        .unwrap();
        drop(old);
        assert!(
            RuntimeCacheLocks::acquire(
                std::slice::from_ref(&cache),
                RuntimeCacheLockPolicy::immediate(),
            )
            .is_err()
        );
        assert!(directory.is_dir());
        drop(successor);
        RuntimeCacheLocks::acquire(&[cache], RuntimeCacheLockPolicy::immediate()).unwrap();
    }
}
