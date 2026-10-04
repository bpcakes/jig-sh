//! Extended access control list checks for restore.
//!
//! Darwin evaluates ACL entries independently of POSIX mode bits, so an
//! inherited allow entry survives `chmod 0700`/`0600` and can still grant
//! other principals access. Restore therefore clears every entry from the
//! directories and files it creates before writing contents into them, and
//! refuses existing parents whose ACLs grant shared write access. Linux POSIX
//! ACLs need no counterpart: the explicit mode change also narrows their mask.

#[cfg(target_os = "macos")]
pub(super) use darwin::{clear_directory, clear_file, reject_shared_write, require_none};

#[cfg(not(target_os = "macos"))]
pub(super) use portable::{clear_directory, clear_file, reject_shared_write, require_none};

#[cfg(target_os = "macos")]
mod darwin {
    use std::ffi::{c_int, c_void};
    use std::fs::{File, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    use anyhow::{Context, Result as AnyResult, bail};

    // Values from <sys/acl.h>; the libc crate does not expose the Darwin ACL API.
    const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
    const ACL_FIRST_ENTRY: c_int = 0;
    const ACL_NEXT_ENTRY: c_int = -1;
    const ACL_EXTENDED_ALLOW: c_int = 1;
    const ACL_ADD_FILE: c_int = 1 << 2;
    const ACL_ADD_SUBDIRECTORY: c_int = 1 << 5;
    const ACL_DELETE_CHILD: c_int = 1 << 6;
    const ACL_WRITE_SECURITY: c_int = 1 << 12;
    const ACL_CHANGE_OWNER: c_int = 1 << 13;

    /// Directory permissions equivalent to the group/other write bits that
    /// restore already refuses: adding, removing, or renaming entries, or
    /// changing the directory's own permissions or owner.
    const SHARED_WRITE_PERMISSIONS: [c_int; 5] = [
        ACL_ADD_FILE,
        ACL_ADD_SUBDIRECTORY,
        ACL_DELETE_CHILD,
        ACL_WRITE_SECURITY,
        ACL_CHANGE_OWNER,
    ];

    unsafe extern "C" {
        fn acl_get_fd_np(fd: c_int, acl_type: c_int) -> *mut c_void;
        fn acl_set_fd_np(fd: c_int, acl: *mut c_void, acl_type: c_int) -> c_int;
        fn acl_init(count: c_int) -> *mut c_void;
        fn acl_free(object: *mut c_void) -> c_int;
        fn acl_get_entry(acl: *mut c_void, entry_id: c_int, entry: *mut *mut c_void) -> c_int;
        fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
        fn acl_get_permset(entry: *mut c_void, permset: *mut *mut c_void) -> c_int;
        fn acl_get_perm_np(permset: *mut c_void, permission: c_int) -> c_int;
    }

    /// Owned `acl_t` released on drop.
    struct Acl(*mut c_void);

    impl Drop for Acl {
        fn drop(&mut self) {
            // SAFETY: the pointer came from acl_get_fd_np or acl_init and is
            // released exactly once.
            unsafe {
                acl_free(self.0);
            }
        }
    }

    struct Entry {
        allows: bool,
        grants_shared_write: bool,
    }

    impl Acl {
        fn read(file: &File, path: &Path) -> AnyResult<Option<Self>> {
            // SAFETY: the descriptor is open for the duration of the call.
            let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
            if !acl.is_null() {
                return Ok(Some(Self(acl)));
            }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                // No ACL, or a filesystem that cannot store one.
                Some(libc::ENOENT | libc::ENOTSUP | libc::EOPNOTSUPP) => Ok(None),
                _ => Err(error).with_context(|| {
                    format!(
                        "failed to read the access control list of {}",
                        path.display()
                    )
                }),
            }
        }

        fn entries(&self, path: &Path) -> AnyResult<Vec<Entry>> {
            let mut entries = Vec::new();
            let mut entry_id = ACL_FIRST_ENTRY;
            loop {
                let mut entry = std::ptr::null_mut();
                // SAFETY: `self.0` is a live ACL and `entry` is a valid out pointer.
                // Darwin reports the end of the list as -1.
                if unsafe { acl_get_entry(self.0, entry_id, &mut entry) } != 0 {
                    return Ok(entries);
                }
                entry_id = ACL_NEXT_ENTRY;
                let mut tag = 0;
                let mut permset = std::ptr::null_mut();
                // SAFETY: `entry` was returned by acl_get_entry for this ACL,
                // and both out pointers are valid.
                let inspected = unsafe {
                    acl_get_tag_type(entry, &mut tag) == 0
                        && acl_get_permset(entry, &mut permset) == 0
                };
                if !inspected {
                    return Err(std::io::Error::last_os_error()).with_context(|| {
                        format!(
                            "failed to inspect an access control entry of {}",
                            path.display()
                        )
                    });
                }
                let grants_shared_write = SHARED_WRITE_PERMISSIONS
                    .iter()
                    // SAFETY: `permset` belongs to the live entry above.
                    .any(|permission| unsafe { acl_get_perm_np(permset, *permission) } == 1);
                entries.push(Entry {
                    allows: tag == ACL_EXTENDED_ALLOW,
                    grants_shared_write,
                });
            }
        }
    }

    fn has_entries(file: &File, path: &Path) -> AnyResult<bool> {
        match Acl::read(file, path)? {
            Some(acl) => Ok(!acl.entries(path)?.is_empty()),
            None => Ok(false),
        }
    }

    fn open_nofollow(path: &Path) -> AnyResult<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .with_context(|| {
                format!(
                    "failed to open {} to inspect its access control list",
                    path.display()
                )
            })
    }

    /// Removes every extended ACL entry, including inherited ones, and
    /// verifies that none remain.
    pub(in crate::backup::restore) fn clear_file(file: &File, path: &Path) -> AnyResult<()> {
        if !has_entries(file, path)? {
            return Ok(());
        }
        // SAFETY: acl_init has no preconditions; a null result is handled.
        let empty = unsafe { acl_init(0) };
        if empty.is_null() {
            return Err(std::io::Error::last_os_error())
                .context("failed to allocate an empty access control list");
        }
        let empty = Acl(empty);
        // SAFETY: the descriptor is open and `empty` is a live ACL.
        if unsafe { acl_set_fd_np(file.as_raw_fd(), empty.0, ACL_TYPE_EXTENDED) } != 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "failed to clear the access control list of {}",
                    path.display()
                )
            });
        }
        if has_entries(file, path)? {
            bail!(
                "{} still has an access control list after clearing it",
                path.display()
            );
        }
        Ok(())
    }

    pub(in crate::backup::restore) fn clear_directory(path: &Path) -> AnyResult<()> {
        clear_file(&open_nofollow(path)?, path)
    }

    /// Refuses any extended ACL entry on a restore-owned path.
    pub(in crate::backup::restore) fn require_none(path: &Path) -> AnyResult<()> {
        if has_entries(&open_nofollow(path)?, path)? {
            bail!(
                "protected restore path has an access control list that can bypass owner-only permissions: {}",
                path.display()
            );
        }
        Ok(())
    }

    /// Refuses an existing directory whose ACL grants write-equivalent access.
    ///
    /// Deny entries, such as the "everyone deny delete" entries macOS places
    /// on home folders, only narrow access and are accepted.
    pub(in crate::backup::restore) fn reject_shared_write(path: &Path) -> AnyResult<()> {
        let file = open_nofollow(path)?;
        let Some(acl) = Acl::read(&file, path)? else {
            return Ok(());
        };
        if acl
            .entries(path)?
            .iter()
            .any(|entry| entry.allows && entry.grants_shared_write)
        {
            bail!(
                "restore target directory has an access control list that grants shared write access: {}",
                path.display()
            );
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod portable {
    use std::fs::File;
    use std::path::Path;

    use anyhow::Result as AnyResult;

    pub(in crate::backup::restore) fn clear_file(_file: &File, _path: &Path) -> AnyResult<()> {
        Ok(())
    }

    pub(in crate::backup::restore) fn clear_directory(_path: &Path) -> AnyResult<()> {
        Ok(())
    }

    pub(in crate::backup::restore) fn require_none(_path: &Path) -> AnyResult<()> {
        Ok(())
    }

    pub(in crate::backup::restore) fn reject_shared_write(_path: &Path) -> AnyResult<()> {
        Ok(())
    }
}
