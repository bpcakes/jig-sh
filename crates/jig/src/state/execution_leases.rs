use std::fs::{self, File, OpenOptions};

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use jig_contract::ActionEffect;

use crate::context::RepoContext;

use super::support::ensure_state_layout;

const REPOSITORY_EXECUTION_LEASE: &str = ".agent/.cache/repository-execution.lock";

pub(crate) struct RepositoryExecutionLease {
    _file: File,
}

pub(crate) fn acquire_repository_execution_lease(
    ctx: &RepoContext,
    effects: &[ActionEffect],
) -> Result<RepositoryExecutionLease> {
    ensure_state_layout(ctx)?;
    let path = ctx.root().join(REPOSITORY_EXECUTION_LEASE);
    let parent = path
        .parent()
        .expect("repository execution lease path has a parent");
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| {
            format!(
                "Failed to open repository execution lease {}",
                path.display()
            )
        })?;
    if requires_exclusive_execution(effects) {
        FileExt::lock_exclusive(&file)
            .context("Failed to acquire exclusive repository execution lease")?;
    } else {
        FileExt::lock_shared(&file)
            .context("Failed to acquire shared repository execution lease")?;
    }
    Ok(RepositoryExecutionLease { _file: file })
}

fn requires_exclusive_execution(effects: &[ActionEffect]) -> bool {
    effects.contains(&ActionEffect::Worktree)
        || effects.contains(&ActionEffect::External)
        || (!effects.is_empty() && !effects.contains(&ActionEffect::ReadOnly))
}
