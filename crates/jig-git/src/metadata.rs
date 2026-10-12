//! Byte-preserving helpers for reading Git's on-disk metadata files directly.

#[cfg(unix)]
use std::ffi::OsString;
use std::io::Read as _;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};

/// Upper bound for single-line Git pointer files such as `.git`, `commondir`,
/// and a linked worktree's `gitdir` back-link.
pub const MAX_GIT_POINTER_BYTES: u64 = 16 * 1024;

#[cfg(unix)]
pub fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
pub fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

pub fn trim_ascii_line(mut bytes: &[u8]) -> &[u8] {
    while bytes
        .last()
        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
    {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

/// Reads `name` inside `directory` only when it is a regular file of at most
/// `max_bytes`, without following a final symlink.
///
/// Returns `Ok(None)` when the entry is absent, is not a regular file, or
/// exceeds the limit. The open is non-blocking so a FIFO swapped in after an
/// earlier metadata check cannot hang the caller, and the read is capped
/// independently of the opened file's reported length.
pub fn read_nofollow_regular_file(
    directory: &Dir,
    name: &str,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = match directory.open_with(name, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to open {name} without following links"));
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("Failed to read {name}"))?;
    if bytes.len() as u64 > max_bytes {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// Parses a single-line Git path file such as `commondir` or a linked
/// worktree's `gitdir` back-link, resolving a relative path against `base`.
pub fn parse_git_path_line(bytes: &[u8], base: &Path) -> Option<PathBuf> {
    let line = trim_ascii_line(bytes);
    if line.is_empty() || line.contains(&b'\n') || line.contains(&b'\r') {
        return None;
    }
    let path = path_from_git_bytes(line);
    Some(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

/// Parses a `.git` pointer file (`gitdir: PATH`), resolving a relative path
/// against the worktree that contains it.
pub fn parse_gitdir_pointer(bytes: &[u8], worktree: &Path) -> Option<PathBuf> {
    parse_git_path_line(trim_ascii_line(bytes).strip_prefix(b"gitdir: ")?, worktree)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cap_std::ambient_authority;
    use tempfile::tempdir;

    use super::*;

    fn open(path: &Path) -> Dir {
        Dir::open_ambient_dir(path, ambient_authority()).unwrap()
    }

    #[test]
    fn reads_bounded_regular_files_only() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("pointer"), b"gitdir: ../admin\n").unwrap();
        fs::write(temp.path().join("large"), vec![b'x'; 33]).unwrap();
        fs::create_dir(temp.path().join("directory")).unwrap();
        let directory = open(temp.path());

        assert_eq!(
            read_nofollow_regular_file(&directory, "pointer", 32).unwrap(),
            Some(b"gitdir: ../admin\n".to_vec())
        );
        assert_eq!(
            read_nofollow_regular_file(&directory, "large", 32).unwrap(),
            None
        );
        assert_eq!(
            read_nofollow_regular_file(&directory, "missing", 32).unwrap(),
            None
        );
        assert_eq!(
            read_nofollow_regular_file(&directory, "directory", 32).unwrap(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_does_not_block_on_fifos() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("target"), b"gitdir: ../admin\n").unwrap();
        std::os::unix::fs::symlink("target", temp.path().join("link")).unwrap();
        let fifo = std::ffi::CString::new(
            temp.path()
                .join("fifo")
                .into_os_string()
                .into_encoded_bytes(),
        )
        .unwrap();
        // SAFETY: `fifo` is a valid NUL-terminated path for the call's duration.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let directory = open(temp.path());

        assert!(read_nofollow_regular_file(&directory, "link", 32).is_err());
        assert_eq!(
            read_nofollow_regular_file(&directory, "fifo", 32).unwrap(),
            None
        );
    }

    #[test]
    fn parses_single_line_git_paths() {
        let base = Path::new("/checkout");
        assert_eq!(
            parse_gitdir_pointer(b"gitdir: ../main/.git/worktrees/wt\n", base),
            Some(PathBuf::from("/checkout/../main/.git/worktrees/wt"))
        );
        assert_eq!(
            parse_gitdir_pointer(b"gitdir: /main/.git/worktrees/wt\r\n", base),
            Some(PathBuf::from("/main/.git/worktrees/wt"))
        );
        assert_eq!(
            parse_git_path_line(b"../..\n", base),
            Some(PathBuf::from("/checkout/../.."))
        );
        assert_eq!(parse_gitdir_pointer(b"gitdir: \n", base), None);
        assert_eq!(parse_gitdir_pointer(b"../admin\n", base), None);
        assert_eq!(parse_gitdir_pointer(b"gitdir: a\ngitdir: b\n", base), None);
        assert_eq!(parse_git_path_line(b"\n", base), None);
    }
}
