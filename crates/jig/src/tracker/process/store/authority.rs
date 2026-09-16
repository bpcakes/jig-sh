use std::fs::{self, File};
use std::path::{Path, PathBuf};

use super::{database_error, jsonl_error};
use crate::tracker::{InvalidWorkspaceReason, TrackerError};

pub(super) const SNAPSHOT_DATABASE_FAMILY_SUFFIXES: &[&str] = &[
    "-wal",
    "-journal",
    "-fsqlite-ns-gate",
    "-fsqlite-ns-use",
    "-wal-cert",
    "-wal-cert-head",
    ".fsqlite-migration-state",
];
const LIVE_DATABASE_FAMILY_SUFFIXES: &[&str] = &[
    "-wal",
    "-shm",
    "-journal",
    "-fsqlite-ns-gate",
    "-fsqlite-ns-use",
    "-wal-cert",
    "-wal-cert-head",
    ".fsqlite-migration-state",
];
pub(super) const LEGACY_LOCK_FILE: &str = ".beads.lock";
const FIXED_LIVE_LOCK_FILES: &[&str] = &[LEGACY_LOCK_FILE, ".write.lock", ".sync.lock"];

pub(super) fn open_authority_file(path: &Path) -> std::io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path)
}

pub(super) fn validate_live_database_family(database_path: &Path) -> Result<(), TrackerError> {
    for path in live_database_family_paths(database_path)? {
        match open_authority_file(&path) {
            Ok(file) => {
                DatabaseIdentity::capture(&file, &path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(database_error()),
        }
    }
    Ok(())
}

fn live_database_family_paths(database_path: &Path) -> Result<Vec<PathBuf>, TrackerError> {
    let parent = database_path.parent().ok_or_else(database_error)?;
    let mut paths =
        Vec::with_capacity(LIVE_DATABASE_FAMILY_SUFFIXES.len() + FIXED_LIVE_LOCK_FILES.len());
    for suffix in LIVE_DATABASE_FAMILY_SUFFIXES {
        let mut path = database_path.as_os_str().to_os_string();
        path.push(suffix);
        paths.push(PathBuf::from(path));
    }
    paths.extend(FIXED_LIVE_LOCK_FILES.iter().map(|name| parent.join(name)));
    Ok(paths)
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct SnapshotIdentity {
    database: DatabaseIdentity,
    pub(super) len: u64,
    pub(super) modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

impl SnapshotIdentity {
    pub(super) fn capture(file: &File, path: &Path) -> Result<Self, TrackerError> {
        let database = DatabaseIdentity::capture(file, path)?;
        let metadata = file.metadata().map_err(|_| database_error())?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            database,
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }

    pub(super) fn verify(&self, file: &File, path: &Path) -> Result<(), TrackerError> {
        let current = match Self::capture(file, path) {
            Ok(current) => current,
            Err(_)
                if fs::symlink_metadata(path)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Err(TrackerError::StoreChangedDuringSnapshot);
            }
            Err(error) => return Err(error),
        };
        if current.database != self.database {
            Err(database_error())
        } else if current == *self {
            Ok(())
        } else {
            Err(TrackerError::StoreChangedDuringSnapshot)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DatabaseIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl DatabaseIdentity {
    pub(super) fn capture(file: &File, path: &Path) -> Result<Self, TrackerError> {
        let opened = file.metadata().map_err(|_| database_error())?;
        let named = fs::symlink_metadata(path).map_err(|_| database_error())?;
        if !opened.is_file() || !named.is_file() || named.file_type().is_symlink() {
            return Err(database_error());
        }
        if has_multiple_links(&opened) || has_multiple_links(&named) {
            return Err(hard_link_error());
        }
        let identity = Self::from_metadata(&opened);
        if identity != Self::from_metadata(&named) {
            return Err(database_error());
        }
        Ok(identity)
    }

    pub(super) fn verify(&self, file: &File, path: &Path) -> Result<(), TrackerError> {
        (Self::capture(file, path)? == *self)
            .then_some(())
            .ok_or_else(database_error)
    }

    fn from_metadata(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct JsonlIdentity {
    database: DatabaseIdentity,
    pub(super) len: u64,
    pub(super) modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

impl JsonlIdentity {
    pub(super) fn capture(file: &File, path: &Path) -> Result<Self, TrackerError> {
        let database = DatabaseIdentity::capture(file, path).map_err(as_jsonl_error)?;
        let metadata = file.metadata().map_err(|_| jsonl_error())?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            database,
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }

    pub(super) fn verify(&self, file: &File, path: &Path) -> Result<(), TrackerError> {
        let current = match Self::capture(file, path) {
            Ok(current) => current,
            Err(_)
                if fs::symlink_metadata(path)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Err(TrackerError::StoreChangedDuringSnapshot);
            }
            Err(error) => return Err(error),
        };
        if current.database != self.database {
            Err(jsonl_error())
        } else if current == *self {
            Ok(())
        } else {
            Err(TrackerError::StoreChangedDuringSnapshot)
        }
    }
}

pub(super) fn as_jsonl_error(error: TrackerError) -> TrackerError {
    if error
        == (TrackerError::InvalidWorkspace {
            reason: InvalidWorkspaceReason::HardLinkedAuthority,
        })
    {
        error
    } else {
        jsonl_error()
    }
}

const fn hard_link_error() -> TrackerError {
    TrackerError::InvalidWorkspace {
        reason: InvalidWorkspaceReason::HardLinkedAuthority,
    }
}

#[cfg(unix)]
fn has_multiple_links(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
const fn has_multiple_links(_metadata: &fs::Metadata) -> bool {
    false
}
