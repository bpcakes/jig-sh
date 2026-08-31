use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const PROTECTED_SCHEDULE_DIR: &str = "jig/loop";
const MAX_GITDIR_FILE_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug)]
pub(super) struct ProtectedScheduleAuthority {
    pub(super) root: PathBuf,
    pub(super) dir: PathBuf,
    pub(super) path: PathBuf,
    pub(super) initialized_path: PathBuf,
    pub(super) lock_path: PathBuf,
}

pub(super) fn resolve_protected_schedule_authority(
    repo_root: &Path,
) -> Result<Option<ProtectedScheduleAuthority>> {
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
    let git_dir = resolve_git_metadata_directory(repo_root, dot_git, metadata)?;
    let dir = git_dir.join(PROTECTED_SCHEDULE_DIR);
    Ok(Some(ProtectedScheduleAuthority {
        root: git_dir,
        path: dir.join("schedule.json"),
        initialized_path: dir.join("schedule.initialized"),
        lock_path: dir.join("schedule.lock"),
        dir,
    }))
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
        let pointer = fs::read_to_string(&dot_git).with_context(|| {
            format!("Failed to read Git metadata pointer {}", dot_git.display())
        })?;
        let pointer = pointer.trim_end_matches(['\r', '\n']);
        if pointer.contains(['\r', '\n']) {
            bail!(
                "Git metadata pointer contains multiple lines at {}",
                dot_git.display()
            );
        }
        let path = pointer
            .strip_prefix("gitdir: ")
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid Git metadata pointer at {}", dot_git.display())
            })?;
        let path = PathBuf::from(path);
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
