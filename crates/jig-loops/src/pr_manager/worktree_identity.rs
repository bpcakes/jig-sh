use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use jig_context::RepoContext;
use jig_execution::ExecutionControl;
use jig_git::metadata::{
    MAX_GIT_POINTER_BYTES, parse_gitdir_pointer, path_from_git_bytes, read_nofollow_regular_file,
    trim_ascii_line,
};

use super::git::{git_output, git_stdout_path};
use super::outcome::pr_step_error;
use crate::managed_path::inspect_managed_directory;

pub(super) fn pr_worktree_is_registered(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    if !inspect_managed_directory(ctx.root(), worktree, "PR repair worktree")? {
        return Ok(false);
    }
    let listing = git_output(
        ctx,
        ctx.root(),
        ["worktree", "list", "--porcelain", "-z"],
        observer,
    )
    .map_err(pr_step_error)?;
    if !listing.status.success() {
        bail!(
            "Failed to list registered Git worktrees: {}",
            String::from_utf8_lossy(&listing.stderr).trim()
        );
    }
    let expected = fs::canonicalize(worktree).with_context(|| {
        format!(
            "Failed to resolve candidate PR repair worktree {}",
            worktree.display()
        )
    })?;
    let registered = worktree_paths_from_porcelain(&listing.stdout)
        .into_iter()
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .any(|candidate| candidate == expected);
    if !registered {
        return Ok(false);
    }
    validate_linked_worktree_gitfile(ctx, worktree, observer)
}

fn worktree_paths_from_porcelain(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .map(path_from_git_bytes)
        .collect()
}

fn validate_linked_worktree_gitfile(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> Result<bool> {
    let worktree_dir = Dir::open_ambient_dir(worktree, ambient_authority())
        .with_context(|| format!("Failed to open PR repair worktree {}", worktree.display()))?;
    let Some(gitdir_pointer) =
        read_nofollow_regular_file(&worktree_dir, ".git", MAX_GIT_POINTER_BYTES)?
    else {
        return Ok(false);
    };
    let Some(gitdir_path) = parse_gitdir_pointer(&gitdir_pointer, worktree) else {
        return Ok(false);
    };
    let gitdir = match fs::canonicalize(&gitdir_path) {
        Ok(path) => path,
        Err(_) => return Ok(false),
    };
    let common = git_stdout_path(ctx, ctx.root(), ["rev-parse", "--git-common-dir"], observer)
        .map_err(pr_step_error)?;
    let common = if common.is_absolute() {
        common
    } else {
        ctx.root().join(common)
    };
    let common = fs::canonicalize(common).context("Failed to resolve the common Git directory")?;
    if gitdir.parent() != Some(common.join("worktrees").as_path()) {
        return Ok(false);
    }
    let gitdir_directory =
        Dir::open_ambient_dir(&gitdir, ambient_authority()).with_context(|| {
            format!(
                "Failed to open linked-worktree Git directory {}",
                gitdir.display()
            )
        })?;
    let Some(back_pointer) =
        read_nofollow_regular_file(&gitdir_directory, "gitdir", MAX_GIT_POINTER_BYTES)?
    else {
        return Ok(false);
    };
    let back_pointer = path_from_git_bytes(trim_ascii_line(&back_pointer));
    let back_pointer = if back_pointer.is_absolute() {
        back_pointer
    } else {
        gitdir.join(back_pointer)
    };
    let expected_gitfile = fs::canonicalize(worktree.join(".git"))?;
    Ok(fs::canonicalize(back_pointer).ok().as_ref() == Some(&expected_gitfile))
}
