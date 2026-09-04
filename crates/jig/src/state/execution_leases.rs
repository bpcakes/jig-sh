use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use jig_contract::ActionEffect;

use crate::context::RepoContext;

use super::support::{AdvisoryLeaseFile, ensure_state_layout};

const REPOSITORY_EXECUTION_LEASE: &str = ".agent/.cache/repository-execution.lock";

pub(crate) struct RepositoryExecutionLease {
    _file: AdvisoryLeaseFile,
    exclusive: bool,
}

#[derive(Debug)]
pub(crate) struct RepositoryExecutionBusy;

impl fmt::Display for RepositoryExecutionBusy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "repository execution is busy with an incompatible run; retry after it finishes or cancel that run first",
        )
    }
}

impl std::error::Error for RepositoryExecutionBusy {}

pub(crate) fn acquire_repository_execution_lease(
    ctx: &RepoContext,
    effects: &[ActionEffect],
) -> Result<RepositoryExecutionLease> {
    let file = open_repository_execution_lease(ctx)?;
    let exclusive = requires_exclusive_execution(effects);
    if exclusive {
        FileExt::lock_exclusive(&file)
            .context("Failed to acquire exclusive repository execution lease")?;
    } else {
        FileExt::lock_shared(&file)
            .context("Failed to acquire shared repository execution lease")?;
    }
    Ok(RepositoryExecutionLease {
        _file: AdvisoryLeaseFile::new(file),
        exclusive,
    })
}

pub(crate) fn try_acquire_repository_execution_lease(
    ctx: &RepoContext,
    effects: &[ActionEffect],
) -> Result<Option<RepositoryExecutionLease>> {
    let file = open_repository_execution_lease(ctx)?;
    let exclusive = requires_exclusive_execution(effects);
    let (acquired, kind) = if exclusive {
        (FileExt::try_lock_exclusive(&file), "exclusive")
    } else {
        (FileExt::try_lock_shared(&file), "shared")
    };
    let acquired = match acquired {
        Ok(acquired) => acquired,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect {kind} repository execution lease"));
        }
    };
    Ok(acquired.then_some(RepositoryExecutionLease {
        _file: AdvisoryLeaseFile::new(file),
        exclusive,
    }))
}

pub(crate) fn acquire_repository_execution_lease_without_wait(
    ctx: &RepoContext,
    effects: &[ActionEffect],
) -> Result<RepositoryExecutionLease> {
    try_acquire_repository_execution_lease(ctx, effects)?
        .ok_or_else(|| RepositoryExecutionBusy.into())
}

impl RepositoryExecutionLease {
    pub(crate) fn permits(&self, effects: &[ActionEffect]) -> bool {
        self.exclusive || !requires_exclusive_execution(effects)
    }
}

fn open_repository_execution_lease(ctx: &RepoContext) -> Result<File> {
    ensure_state_layout(ctx)?;
    let path = ctx.root().join(REPOSITORY_EXECUTION_LEASE);
    let parent = path
        .parent()
        .expect("repository execution lease path has a parent");
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    OpenOptions::new()
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
        })
}

fn requires_exclusive_execution(effects: &[ActionEffect]) -> bool {
    effects.contains(&ActionEffect::Worktree)
        || effects.contains(&ActionEffect::External)
        || (!effects.is_empty() && !effects.contains(&ActionEffect::ReadOnly))
}
