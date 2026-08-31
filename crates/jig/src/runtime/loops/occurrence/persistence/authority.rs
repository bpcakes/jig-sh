use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::bootstrap::{GIT_BIN_ENV, external_program, scrub_known_repository_git_environment};

use super::path_exists;

const PROTECTED_SCHEDULE_DIR: &str = "jig/loop";

#[derive(Clone, Debug)]
pub(super) struct ProtectedScheduleAuthority {
    pub(super) dir: PathBuf,
    pub(super) path: PathBuf,
    pub(super) initialized_path: PathBuf,
    pub(super) lock_path: PathBuf,
}

pub(super) fn resolve_protected_schedule_authority(
    repo_root: &Path,
) -> Result<Option<ProtectedScheduleAuthority>> {
    if !path_exists(&repo_root.join(".git"), "Git metadata entry")? {
        return Ok(None);
    }
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    command
        .current_dir(repo_root)
        .arg("--no-replace-objects")
        .args(["rev-parse", "--absolute-git-dir"]);
    scrub_known_repository_git_environment(&mut command);
    let output = command
        .output()
        .context("Failed to resolve the repository Git metadata directory")?;
    if !output.status.success() {
        let limit = output.stderr.len().min(4_000);
        let stderr = String::from_utf8_lossy(&output.stderr[..limit]);
        bail!(
            "Failed to resolve the repository Git metadata directory: {}",
            stderr.trim()
        );
    }
    let stdout =
        String::from_utf8(output.stdout).context("Git metadata directory output was not UTF-8")?;
    let git_dir = PathBuf::from(stdout.trim());
    if !git_dir.is_absolute() || !git_dir.is_dir() {
        bail!(
            "Git reported an invalid metadata directory: {}",
            git_dir.display()
        );
    }
    let dir = git_dir.join(PROTECTED_SCHEDULE_DIR);
    Ok(Some(ProtectedScheduleAuthority {
        path: dir.join("schedule.json"),
        initialized_path: dir.join("schedule.initialized"),
        lock_path: dir.join("schedule.lock"),
        dir,
    }))
}
