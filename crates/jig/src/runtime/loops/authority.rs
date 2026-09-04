use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::git_path::{path_from_git_bytes, trim_ascii_line};

const PROTECTED_LOOP_DIR: &str = "jig/loop";
const MAX_GITDIR_FILE_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug)]
pub(super) struct ProtectedLoopAuthority {
    pub(super) root: PathBuf,
    pub(super) dir: PathBuf,
}

pub(super) fn resolve_protected_loop_authority(
    repo_root: &Path,
) -> Result<Option<ProtectedLoopAuthority>> {
    let dot_git = repo_root.join(".git");
    let metadata = match fs::symlink_metadata(&dot_git) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Failed to inspect Git metadata entry {}", dot_git.display())
            });
        }
    };
    let root = resolve_git_metadata_directory(repo_root, dot_git, metadata)?;
    let dir = root.join(PROTECTED_LOOP_DIR);
    Ok(Some(ProtectedLoopAuthority { root, dir }))
}

pub(super) fn resolve_protected_repository_authority(
    repo_root: &Path,
) -> Result<Option<ProtectedLoopAuthority>> {
    let Some(worktree_authority) = resolve_protected_loop_authority(repo_root)? else {
        return Ok(None);
    };
    let root = resolve_common_git_directory(&worktree_authority.root)?;
    let dir = root.join(PROTECTED_LOOP_DIR);
    Ok(Some(ProtectedLoopAuthority { root, dir }))
}

fn resolve_common_git_directory(git_dir: &Path) -> Result<PathBuf> {
    let common_pointer = git_dir.join("commondir");
    let metadata = match fs::symlink_metadata(&common_pointer) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(git_dir.to_path_buf());
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect common Git metadata pointer {}",
                    common_pointer.display()
                )
            });
        }
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_GITDIR_FILE_BYTES {
        bail!(
            "Common Git metadata pointer must be a bounded regular file: {}",
            common_pointer.display()
        );
    }
    let pointer = fs::read(&common_pointer).with_context(|| {
        format!(
            "Failed to read common Git metadata pointer {}",
            common_pointer.display()
        )
    })?;
    let pointer = trim_ascii_line(&pointer);
    if pointer.is_empty() || pointer.contains(&b'\r') || pointer.contains(&b'\n') {
        bail!(
            "Common Git metadata pointer is empty or contains multiple lines at {}",
            common_pointer.display()
        );
    }
    let pointer = path_from_git_bytes(pointer);
    let common_dir = if pointer.is_absolute() {
        pointer
    } else {
        git_dir.join(pointer)
    };
    let common_dir = fs::canonicalize(&common_dir).with_context(|| {
        format!(
            "Failed to resolve the common Git metadata directory {}",
            common_dir.display()
        )
    })?;
    if !common_dir.is_dir() {
        bail!(
            "Resolved common Git metadata path is not a directory: {}",
            common_dir.display()
        );
    }
    Ok(common_dir)
}

fn resolve_git_metadata_directory(
    repo_root: &Path,
    dot_git: PathBuf,
    metadata: fs::Metadata,
) -> Result<PathBuf> {
    let git_dir = if metadata.file_type().is_dir() {
        dot_git
    } else if metadata.file_type().is_file() {
        if metadata.len() > MAX_GITDIR_FILE_BYTES {
            bail!(
                "Git metadata pointer is too large at {} ({} bytes; maximum {MAX_GITDIR_FILE_BYTES})",
                dot_git.display(),
                metadata.len()
            );
        }
        let pointer = fs::read(&dot_git).with_context(|| {
            format!("Failed to read Git metadata pointer {}", dot_git.display())
        })?;
        let pointer = trim_ascii_line(&pointer);
        if pointer.contains(&b'\r') || pointer.contains(&b'\n') {
            bail!(
                "Git metadata pointer contains multiple lines at {}",
                dot_git.display()
            );
        }
        let path = pointer
            .strip_prefix(b"gitdir: ")
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid Git metadata pointer at {}", dot_git.display())
            })?;
        let path = path_from_git_bytes(path);
        if path.is_absolute() {
            path
        } else {
            repo_root.join(path)
        }
    } else {
        bail!(
            "Git metadata entry must be a directory or regular pointer file: {}",
            dot_git.display()
        );
    };
    let git_dir = fs::canonicalize(&git_dir).with_context(|| {
        format!(
            "Failed to resolve the repository Git metadata directory {}",
            git_dir.display()
        )
    })?;
    if !git_dir.is_dir() {
        bail!(
            "Resolved Git metadata path is not a directory: {}",
            git_dir.display()
        );
    }
    Ok(git_dir)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn linked_worktree_authority_preserves_non_utf8_git_paths() {
        let temp = tempdir().unwrap();
        let common = temp
            .path()
            .join(OsString::from_vec(b"common-repo-\xff".to_vec()))
            .join(".git");
        let git_dir = common.join("worktrees/scheduler");
        let checkout = temp.path().join("scheduler");
        fs::create_dir_all(&git_dir).unwrap();
        fs::create_dir_all(&checkout).unwrap();
        let mut pointer = b"gitdir: ".to_vec();
        pointer.extend_from_slice(git_dir.as_os_str().as_bytes());
        pointer.push(b'\n');
        fs::write(checkout.join(".git"), pointer).unwrap();
        fs::write(git_dir.join("commondir"), b"../..\n").unwrap();

        let worktree = resolve_protected_loop_authority(&checkout)
            .unwrap()
            .unwrap();
        let repository = resolve_protected_repository_authority(&checkout)
            .unwrap()
            .unwrap();

        assert_eq!(worktree.root, git_dir.canonicalize().unwrap());
        assert_eq!(repository.root, common.canonicalize().unwrap());
    }
}
