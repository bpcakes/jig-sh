//! Extended access control list handling for private vault paths.
//!
//! Darwin evaluates ACL entries independently of POSIX mode bits, so an
//! inherited allow entry survives `chmod 0700`/`0600` and can still grant
//! other principals access. Callers therefore clear every entry from the
//! private directories and files they create or reuse before writing
//! contents into them, and refuse a directory they rely on whose ACL grants
//! other principals write, delete, or permission-change access. Linux POSIX
//! ACLs need no counterpart: the explicit mode change also narrows their mask.

#[cfg(target_os = "macos")]
pub(crate) use darwin::{clear_directory, clear_file, reject_shared_write, require_none};

#[cfg(not(target_os = "macos"))]
pub(crate) use portable::{clear_directory, clear_file, reject_shared_write, require_none};

#[cfg(all(test, target_os = "macos"))]
pub(crate) mod test_support;

#[cfg(target_os = "macos")]
mod darwin {
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::fs::{File, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    use anyhow::{Context, Result as AnyResult, bail};

    // Values from <sys/acl.h>; the libc crate does not expose the Darwin ACL API.
    const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
    const ACL_FIRST_ENTRY: c_int = 0;
    const ACL_NEXT_ENTRY: c_int = -1;
    const ACL_EXTENDED_ALLOW: c_int = 1;
    const ACL_ADD_FILE: c_int = 1 << 2;
    const ACL_DELETE: c_int = 1 << 4;
    const ACL_ADD_SUBDIRECTORY: c_int = 1 << 5;
    const ACL_DELETE_CHILD: c_int = 1 << 6;
    const ACL_WRITE_SECURITY: c_int = 1 << 12;
    const ACL_CHANGE_OWNER: c_int = 1 << 13;

    /// Directory permissions that let another principal disturb a protected
    /// parent: adding, removing, or renaming its entries (the group/other
    /// write bits callers already refuse), deleting or renaming the
    /// directory itself, or changing its permissions or owner. XNU honors an
    /// allowed `delete` before sticky-directory protection, so even a parent
    /// inside a sticky shared directory can be moved away when it grants it.
    const UNSAFE_PARENT_PERMISSIONS: [c_int; 6] = [
        ACL_ADD_FILE,
        ACL_DELETE,
        ACL_ADD_SUBDIRECTORY,
        ACL_DELETE_CHILD,
        ACL_WRITE_SECURITY,
        ACL_CHANGE_OWNER,
    ];

    unsafe extern "C" {
        fn acl_get_fd_np(fd: c_int, acl_type: c_int) -> *mut c_void;
        fn acl_get_link_np(path: *const c_char, acl_type: c_int) -> *mut c_void;
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
            // SAFETY: the pointer came from an acl_get_* call or acl_init and is
            // released exactly once.
            unsafe {
                acl_free(self.0);
            }
        }
    }

    struct Entry {
        allows: bool,
        grants_unsafe_parent_access: bool,
    }

    impl Acl {
        fn read(file: &File, path: &Path) -> AnyResult<Option<Self>> {
            // SAFETY: the descriptor is open for the duration of the call.
            Self::from_result(
                unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) },
                path,
            )
        }

        /// Reads without opening or following `path`, so a directory the
        /// current user may traverse but not list can still be inspected.
        fn read_link(path: &Path) -> AnyResult<Option<Self>> {
            let c_path = CString::new(path.as_os_str().as_bytes()).context("path contains NUL")?;
            // SAFETY: `c_path` is NUL-terminated and outlives the call.
            Self::from_result(
                unsafe { acl_get_link_np(c_path.as_ptr(), ACL_TYPE_EXTENDED) },
                path,
            )
        }

        fn from_result(acl: *mut c_void, path: &Path) -> AnyResult<Option<Self>> {
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
                let grants_unsafe_parent_access = UNSAFE_PARENT_PERMISSIONS
                    .iter()
                    // SAFETY: `permset` belongs to the live entry above.
                    .any(|permission| unsafe { acl_get_perm_np(permset, *permission) } == 1);
                entries.push(Entry {
                    allows: tag == ACL_EXTENDED_ALLOW,
                    grants_unsafe_parent_access,
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
    pub(crate) fn clear_file(file: &File, path: &Path) -> AnyResult<()> {
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

    pub(crate) fn clear_directory(path: &Path) -> AnyResult<()> {
        clear_file(&open_nofollow(path)?, path)
    }

    /// Refuses any extended ACL entry on a path the caller owns and keeps
    /// private.
    pub(crate) fn require_none(path: &Path) -> AnyResult<()> {
        if has_entries(&open_nofollow(path)?, path)? {
            bail!(
                "protected path has an access control list that can bypass owner-only permissions: {}",
                path.display()
            );
        }
        Ok(())
    }

    /// Refuses an existing directory whose ACL grants another principal
    /// write, delete, or permission-change access.
    ///
    /// Deny entries, such as the "everyone deny delete" entries macOS places
    /// on home folders, only narrow access and are accepted.
    pub(crate) fn reject_shared_write(path: &Path) -> AnyResult<()> {
        let Some(acl) = Acl::read_link(path)? else {
            return Ok(());
        };
        if acl
            .entries(path)?
            .iter()
            .any(|entry| entry.allows && entry.grants_unsafe_parent_access)
        {
            bail!(
                "directory has an access control list that lets other users write to, delete, or re-permission it: {}",
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

    pub(crate) fn clear_file(_file: &File, _path: &Path) -> AnyResult<()> {
        Ok(())
    }

    pub(crate) fn clear_directory(_path: &Path) -> AnyResult<()> {
        Ok(())
    }

    pub(crate) fn require_none(_path: &Path) -> AnyResult<()> {
        Ok(())
    }

    pub(crate) fn reject_shared_write(_path: &Path) -> AnyResult<()> {
        Ok(())
    }
}
