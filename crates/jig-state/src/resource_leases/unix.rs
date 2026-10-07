use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::{File, Metadata};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

use anyhow::{Result, bail};
use fs4::fs_std::FileExt;

use super::{ResourceClaimMode, ResourceLease};

#[cfg(target_os = "linux")]
const TEMPORARY_ROOT: &CStr = c"/tmp";
#[cfg(target_os = "macos")]
const TEMPORARY_ROOT: &CStr = c"/private/tmp";

pub(super) fn try_acquire(
    claims: &BTreeMap<&str, ResourceClaimMode>,
) -> Result<Option<ResourceLease>> {
    if claims.is_empty() {
        return Ok(Some(ResourceLease { files: Vec::new() }));
    }
    let root = open_directory(libc::AT_FDCWD, TEMPORARY_ROOT)?;
    let metadata = root.metadata().map_err(|_| namespace_error())?;
    // The fixed system root must not be replaceable by an unprivileged user.
    // Writable system temporary roots must have the sticky bit set.
    if metadata.uid() != 0 || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0) {
        return Err(namespace_error());
    }
    // SAFETY: geteuid has no pointer or lifetime requirements.
    let uid = unsafe { libc::geteuid() };
    let name =
        CString::new(format!("jig-resource-leases-v1-{uid}")).map_err(|_| namespace_error())?;
    let namespace = private_namespace(&root, &name, uid)?;
    let mut files = Vec::with_capacity(claims.len());
    for (key, mode) in claims {
        let name = CString::new(*key).map_err(|_| claim_error())?;
        let file = open_claim(&namespace, &name, uid)?;
        let acquired = match mode {
            ResourceClaimMode::Shared => FileExt::try_lock_shared(&file),
            ResourceClaimMode::Exclusive => FileExt::try_lock_exclusive(&file),
        };
        match acquired {
            Ok(true) => {}
            Ok(false) => return Ok(None),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            Err(_) => bail!("resource claim acquisition failed"),
        }
        // A replacement between open and locking must not split authority.
        let current = open_claim(&namespace, &name, uid)?;
        if !same_identity(&file, &current)? {
            bail!("resource claim identity changed during acquisition");
        }
        files.push(Arc::new(file));
    }
    let current = open_directory(root.as_raw_fd(), &name)?;
    validate_private_directory(&current, uid)?;
    if !same_identity(&namespace, &current)? {
        bail!("resource namespace identity changed during acquisition");
    }
    // No unlock Drop: flock ownership survives while any inherited descriptor
    // for this open file description remains, including after parent SIGKILL.
    Ok(Some(ResourceLease { files }))
}

fn private_namespace(root: &File, name: &CStr, uid: u32) -> Result<File> {
    // SAFETY: root is an open directory and name is a terminated single leaf.
    if unsafe { libc::mkdirat(root.as_raw_fd(), name.as_ptr(), 0o700) } != 0
        && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
    {
        return Err(namespace_error());
    }
    let directory = open_directory(root.as_raw_fd(), name)?;
    validate_private_directory(&directory, uid)?;
    Ok(directory)
}

fn open_directory(parent: i32, name: &CStr) -> Result<File> {
    // SAFETY: the C string is valid; a successful open transfers a new FD.
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(namespace_error());
    }
    // SAFETY: fd was just opened and has no other Rust owner.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn validate_private_directory(directory: &File, uid: u32) -> Result<()> {
    let metadata = directory.metadata().map_err(|_| namespace_error())?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
        return Err(namespace_error());
    }
    Ok(())
}

fn open_claim(namespace: &File, name: &CStr, uid: u32) -> Result<File> {
    // NONBLOCK prevents an attacker-created FIFO from blocking before fstat.
    // SAFETY: arguments reference live FD/string storage; success owns a new FD.
    let fd = unsafe {
        libc::openat(
            namespace.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(claim_error());
    }
    // SAFETY: fd was just opened and has no other Rust owner.
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|_| claim_error())?;
    if !valid_claim_metadata(&metadata, uid) {
        return Err(claim_error());
    }
    Ok(file)
}

fn valid_claim_metadata(metadata: &Metadata, uid: u32) -> bool {
    metadata.is_file()
        && metadata.uid() == uid
        && metadata.mode() & 0o7777 == 0o600
        && metadata.nlink() == 1
}

fn same_identity(first: &File, second: &File) -> Result<bool> {
    let first = first.metadata().map_err(|_| claim_error())?;
    let second = second.metadata().map_err(|_| claim_error())?;
    Ok(first.dev() == second.dev() && first.ino() == second.ino())
}

pub(super) fn inherit_into(files: &[Arc<File>], command: &mut Command) -> Result<()> {
    for file in files {
        // SAFETY: fcntl reads flags of an owned live descriptor.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        if flags < 0 || flags & libc::FD_CLOEXEC == 0 {
            bail!("resource claim descriptor is not private");
        }
    }
    let files = files.to_vec();
    // SAFETY: this child-only hook performs fcntl calls on retained descriptors,
    // does not allocate or lock, and leaves the parent's FD flags unchanged.
    unsafe {
        command.pre_exec(move || {
            for file in &files {
                let flags = libc::fcntl(file.as_raw_fd(), libc::F_GETFD);
                if flags < 0
                    || libc::fcntl(file.as_raw_fd(), libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(())
}

fn namespace_error() -> anyhow::Error {
    anyhow::anyhow!("resource coordination namespace is unavailable or unsafe")
}

fn claim_error() -> anyhow::Error {
    anyhow::anyhow!("resource claim file is unavailable or unsafe")
}

#[cfg(test)]
mod tests;
