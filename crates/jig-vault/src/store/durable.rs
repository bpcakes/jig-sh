//! Durability for vault homes and the rollback witness.
//!
//! Every directory and file sync on the transaction paths goes through this
//! module. A successful operation must establish the durability it relies
//! on even when an interrupted earlier attempt already created the
//! directories it uses: a crash between a `mkdir` or `rename` and the
//! containing-directory sync leaves an entry that exists but may not
//! survive power loss, so existence is never taken as durability. Tests can
//! record the sync, rename, and remove order and inject sync failures; that
//! checks ordering and error propagation but cannot prove real power-loss
//! behavior.

use std::collections::BTreeSet;
use std::fs::{self, File};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result as AnyResult};

/// Directory entries this process has made durable, keyed by path and the
/// directory's identity when its containing directory was synced. A
/// replaced directory has a new identity and is synced again. Entries are
/// recorded only after a successful sync.
static DURABLE_ENTRIES: Mutex<BTreeSet<(PathBuf, u64, u64)>> = Mutex::new(BTreeSet::new());

/// Syncs one directory, persisting the entries renamed, created, or
/// removed in it.
pub(crate) fn sync_dir(path: &Path) -> AnyResult<()> {
    #[cfg(any(test, feature = "test-utils"))]
    recording::observe(recording::FsOp::SyncDir(path.to_path_buf()))
        .with_context(|| format!("failed to sync directory {}", path.display()))?;
    #[cfg(unix)]
    File::open(path)
        .with_context(|| format!("failed to open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync directory {}", path.display()))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Syncs one open file's contents and metadata.
pub(crate) fn sync_file(file: &File, path: &Path) -> AnyResult<()> {
    #[cfg(any(test, feature = "test-utils"))]
    recording::observe(recording::FsOp::SyncFile(path.to_path_buf()))
        .with_context(|| format!("failed to sync {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))
}

/// Renames `from` over `to`; the caller syncs the containing directory.
pub(super) fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)?;
    #[cfg(any(test, feature = "test-utils"))]
    recording::note(recording::FsOp::Rename(to.to_path_buf()));
    Ok(())
}

/// Notes a rename made by other means, such as a no-replace directory
/// rename; the caller syncs the containing directory.
pub(crate) fn renamed(to: &Path) {
    #[cfg(any(test, feature = "test-utils"))]
    recording::note(recording::FsOp::Rename(to.to_path_buf()));
    #[cfg(not(any(test, feature = "test-utils")))]
    let _ = to;
}

/// Removes one file; the caller syncs the containing directory.
pub(super) fn remove_file(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)?;
    #[cfg(any(test, feature = "test-utils"))]
    recording::note(recording::FsOp::Remove(path.to_path_buf()));
    Ok(())
}

/// Creates `path` with any missing ancestors and makes the whole chain of
/// entries durable before anything inside it is relied on.
pub(super) fn create_dir_all_durable(path: &Path) -> AnyResult<()> {
    fs::create_dir_all(path)?;
    ensure_entry_chain_durable(path)
}

/// Makes durable the entry of `dir` and of every ancestor the current user
/// could have created: a directory it owns inside a directory it can write.
/// Any of them may come from an interrupted attempt that never synced its
/// containing directory, whether in this process or another. Stops at the
/// first ancestor the user could not have created, such as the home
/// directory inside a system-owned parent.
pub(crate) fn ensure_entry_chain_durable(dir: &Path) -> AnyResult<()> {
    let mut current = dir.to_path_buf();
    while let Some(parent) = current.parent().map(Path::to_path_buf) {
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("failed to inspect {}", current.display()))?;
        if !could_have_created(&metadata, &parent) {
            break;
        }
        let key = entry_key(&current, &metadata);
        if !lock_entries().contains(&key) {
            sync_dir(&parent)?;
            lock_entries().insert(key);
        }
        current = parent;
    }
    Ok(())
}

fn lock_entries() -> std::sync::MutexGuard<'static, BTreeSet<(PathBuf, u64, u64)>> {
    DURABLE_ENTRIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(unix)]
fn entry_key(path: &Path, metadata: &fs::Metadata) -> (PathBuf, u64, u64) {
    (path.to_path_buf(), metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn entry_key(path: &Path, _metadata: &fs::Metadata) -> (PathBuf, u64, u64) {
    (path.to_path_buf(), 0, 0)
}

#[cfg(unix)]
fn could_have_created(metadata: &fs::Metadata, parent: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return false;
    }
    let Ok(parent) = std::ffi::CString::new(parent.as_os_str().as_bytes()) else {
        return false;
    };
    // Effective IDs, as the creating `mkdir` would have used.
    unsafe {
        libc::faccessat(
            libc::AT_FDCWD,
            parent.as_ptr(),
            libc::W_OK,
            libc::AT_EACCESS,
        ) == 0
    }
}

#[cfg(not(unix))]
fn could_have_created(_metadata: &fs::Metadata, _parent: &Path) -> bool {
    false
}

/// Test-only recording of the durability operations on this thread, and
/// injected sync failures.
#[cfg(any(test, feature = "test-utils"))]
pub(crate) mod recording {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum FsOp {
        SyncDir(PathBuf),
        SyncFile(PathBuf),
        Rename(PathBuf),
        Remove(PathBuf),
    }

    /// A kind of directory-entry publication a sync failure can follow.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Publication {
        Rename,
        Remove,
    }

    struct AfterPublication {
        dir: PathBuf,
        kind: Publication,
        skip: usize,
        armed: bool,
    }

    thread_local! {
        static LOG: RefCell<Option<Vec<FsOp>>> = const { RefCell::new(None) };
        /// Paths whose sync fails after skipping the given number of
        /// matching syncs.
        static FAIL_SYNC: RefCell<Vec<(PathBuf, usize)>> = const { RefCell::new(Vec::new()) };
        /// Directories whose sync fails right after a given publication in
        /// them, whatever other syncs of them come first.
        static FAIL_AFTER_PUBLICATION: RefCell<Vec<AfterPublication>> =
            const { RefCell::new(Vec::new()) };
    }

    /// Runs `f` and returns the durability operations it performed on this
    /// thread, in order.
    pub fn record<T>(f: impl FnOnce() -> T) -> (T, Vec<FsOp>) {
        LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
        let value = f();
        let ops = LOG.with(|log| log.borrow_mut().take()).unwrap_or_default();
        (value, ops)
    }

    /// Makes the next sync of exactly `path` on this thread fail.
    pub fn fail_next_sync_of(path: &Path) {
        fail_sync_after(path, 0);
    }

    /// Makes the sync of exactly `path` that follows `skip` successful
    /// syncs of it on this thread fail.
    pub fn fail_sync_after(path: &Path, skip: usize) {
        FAIL_SYNC.with(|fail| fail.borrow_mut().push((path.to_path_buf(), skip)));
    }

    /// Makes the sync of `dir` that follows its (`skip` + 1)th publication
    /// of `kind` on this thread fail, so a test can fail exactly the sync
    /// that would make that publication durable.
    pub fn fail_sync_after_publication(dir: &Path, kind: Publication, skip: usize) {
        FAIL_AFTER_PUBLICATION.with(|triggers| {
            triggers.borrow_mut().push(AfterPublication {
                dir: dir.to_path_buf(),
                kind,
                skip,
                armed: false,
            });
        });
    }

    /// Forgets which entries under `prefix` this process made durable, as a
    /// fresh process would, so a retry cannot rely on an earlier attempt's
    /// barriers. Scoped to one test's own directory, it cannot disturb
    /// tests running in parallel.
    pub fn forget_durable_entries_under(prefix: &Path) {
        super::lock_entries().retain(|(path, _, _)| !path.starts_with(prefix));
    }

    pub(super) fn note(op: FsOp) {
        let published = match &op {
            FsOp::Rename(path) => Some((path, Publication::Rename)),
            FsOp::Remove(path) => Some((path, Publication::Remove)),
            FsOp::SyncDir(_) | FsOp::SyncFile(_) => None,
        };
        if let Some((path, kind)) = published {
            FAIL_AFTER_PUBLICATION.with(|triggers| {
                for trigger in triggers.borrow_mut().iter_mut() {
                    if trigger.armed
                        || trigger.kind != kind
                        || path.parent() != Some(trigger.dir.as_path())
                    {
                        continue;
                    }
                    if trigger.skip == 0 {
                        trigger.armed = true;
                    } else {
                        trigger.skip -= 1;
                    }
                }
            });
        }
        LOG.with(|log| {
            if let Some(log) = log.borrow_mut().as_mut() {
                log.push(op);
            }
        });
    }

    pub(super) fn observe(op: FsOp) -> std::io::Result<()> {
        let path = match &op {
            FsOp::SyncDir(path) | FsOp::SyncFile(path) => path.clone(),
            FsOp::Rename(_) | FsOp::Remove(_) => return Ok(()),
        };
        if let FsOp::SyncDir(dir) = &op {
            let armed = FAIL_AFTER_PUBLICATION.with(|triggers| {
                let mut triggers = triggers.borrow_mut();
                let index = triggers
                    .iter()
                    .position(|trigger| trigger.armed && trigger.dir == *dir);
                index.map(|index| triggers.remove(index)).is_some()
            });
            if armed {
                return Err(std::io::Error::other("injected sync failure"));
            }
        }
        let injected = FAIL_SYNC.with(|fail| {
            let mut fail = fail.borrow_mut();
            let Some(index) = fail.iter().position(|(candidate, _)| *candidate == path) else {
                return false;
            };
            if fail[index].1 == 0 {
                fail.remove(index);
                return true;
            }
            fail[index].1 -= 1;
            false
        });
        if injected {
            return Err(std::io::Error::other("injected sync failure"));
        }
        note(op);
        Ok(())
    }
}
