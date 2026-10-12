//! PR worktree preparation and cleanup.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use jig_context::RepoContext;
use jig_execution::{AdditionalCancellationControl, ExecutionControl, NoopExecutionObserver};
use sha2::{Digest, Sha256};

use super::PrWorkItem;
use super::git::{bounded_path_component, git_checked, git_error, git_output, git_stdout};
use super::outcome::{PrRepairStepError, PrRepairStepResult, pr_step_error};
use super::push::{remote_branch_ref, require_remote_head};
use super::worktree_identity::pr_worktree_is_registered;
use crate::managed_path::{ensure_managed_directory, inspect_managed_directory};
use crate::occurrence::OccurrenceWorktreeReservation;
use crate::state::{LOOP_RUNTIME_DIR, LeaseGuard};
use crate::workflow::ResolvedWorkflow;

pub(super) fn prepare_worktree(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
    worktree_reservation: Option<&OccurrenceWorktreeReservation>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<PreparedPrWorktree, PrWorktreePreparationError> {
    let worktree = pr_worktree_path(ctx, workflow, item);
    let existed_before_preflight =
        match inspect_managed_directory(ctx.root(), &worktree, "PR repair worktree") {
            Ok(exists) => exists,
            Err(error) => {
                return Err(PrWorktreePreparationError {
                    source: PrRepairStepError::failed(anyhow!(error).context(format!(
                        "Failed to inspect PR repair worktree {}",
                        worktree.display()
                    ))),
                    worktree: Some(PreparedPrWorktree::Retained(worktree)),
                });
            }
        };
    let preflight = (|| -> PrRepairStepResult<()> {
        if !is_git_object_id(&item.head_sha) {
            return Err(PrRepairStepError::failed(anyhow!(
                "GitHub PR snapshot did not include a valid head object ID for PR #{}",
                item.pr_number
            )));
        }
        let parent = worktree
            .parent()
            .ok_or_else(|| anyhow!("Worktree path has no parent: {}", worktree.display()))?;
        ensure_managed_directory(ctx.root(), parent, "PR repair worktree parent")?;

        let head_ref = remote_branch_ref(&item.head_ref);
        git_checked(ctx, ctx.root(), ["fetch", "origin", &head_ref], observer)?;
        let expected_head = format!("{}^{{commit}}", item.head_sha);
        git_checked(
            ctx,
            ctx.root(),
            ["cat-file", "-e", &expected_head],
            observer,
        )?;
        require_remote_head(ctx, ctx.root(), &head_ref, &item.head_sha, observer)
    })();
    if let Err(error) = preflight {
        return Err(PrWorktreePreparationError {
            source: error,
            worktree: existed_before_preflight.then_some(PreparedPrWorktree::Retained(worktree)),
        });
    }
    if let Some(reservation) = worktree_reservation
        && let Err(error) = reservation.reserve(&worktree)
    {
        return Err(PrWorktreePreparationError {
            source: PrRepairStepError::failed(
                error.context("Failed to reserve the PR repair worktree in occurrence state"),
            ),
            worktree: existed_before_preflight.then_some(PreparedPrWorktree::Retained(worktree)),
        });
    }
    let mut created_by_current_attempt = !existed_before_preflight;
    let result = (|| {
        if existed_before_preflight {
            if !pr_worktree_is_registered(ctx, &worktree, observer)? {
                return Err(PrRepairStepError::failed(anyhow!(
                    "Refusing to reuse untrusted PR repair directory {}: it is not an authenticated registered worktree",
                    worktree.display()
                )));
            }
            // A registered checkout authenticates the path, but it is not a safe cache
            // boundary: ordinary `git clean` intentionally retains ignored files and
            // nested repositories. Recreate it so every worker starts from one tree.
            git_checked(
                ctx,
                ctx.root(),
                vec![
                    OsString::from("worktree"),
                    OsString::from("remove"),
                    OsString::from("--force"),
                    worktree.as_os_str().to_os_string(),
                ],
                observer,
            )?;
            created_by_current_attempt = true;
        }
        git_checked(
            ctx,
            ctx.root(),
            vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--detach"),
                worktree.as_os_str().to_os_string(),
                OsString::from(&item.head_sha),
            ],
            observer,
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(PreparedPrWorktree::Created(worktree)),
        Err(error) => Err(PrWorktreePreparationError {
            source: error,
            worktree: Some(if created_by_current_attempt {
                PreparedPrWorktree::Created(worktree)
            } else {
                PreparedPrWorktree::Retained(worktree)
            }),
        }),
    }
}

#[derive(Clone, Debug)]
pub(super) enum PreparedPrWorktree {
    Created(PathBuf),
    Retained(PathBuf),
}

impl PreparedPrWorktree {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Created(path) | Self::Retained(path) => path,
        }
    }

    pub(super) const fn created_by_current_attempt(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

#[derive(Debug)]
pub(super) struct PrWorktreePreparationError {
    pub(super) source: PrRepairStepError,
    pub(super) worktree: Option<PreparedPrWorktree>,
}

enum PrCleanupLease<'a> {
    Guard(&'a mut LeaseGuard),
    #[cfg(test)]
    AssumedHeld,
}

impl PrCleanupLease<'_> {
    fn refresh(&mut self) -> Result<()> {
        match self {
            Self::Guard(guard) => guard.refresh(),
            #[cfg(test)]
            Self::AssumedHeld => Ok(()),
        }
    }

    fn renewal_failed(&self) -> bool {
        match self {
            Self::Guard(guard) => guard.renewal_failed(),
            #[cfg(test)]
            Self::AssumedHeld => false,
        }
    }
}

pub(super) struct PrWorktreeCleanup<'a> {
    ctx: &'a RepoContext,
    lease: PrCleanupLease<'a>,
    observer: NoopExecutionObserver,
}

impl<'a> PrWorktreeCleanup<'a> {
    pub(super) fn new(ctx: &'a RepoContext, lease_guard: &'a mut LeaseGuard) -> Self {
        Self {
            ctx,
            lease: PrCleanupLease::Guard(lease_guard),
            observer: NoopExecutionObserver,
        }
    }

    #[cfg(test)]
    pub(super) fn assuming_lease(ctx: &'a RepoContext) -> Self {
        Self {
            ctx,
            lease: PrCleanupLease::AssumedHeld,
            observer: NoopExecutionObserver,
        }
    }

    fn refresh(&mut self, operation: &str) -> Result<()> {
        self.lease.refresh().with_context(|| {
            format!("Branch lease authority was lost before PR worktree {operation}")
        })
    }

    fn with_control<T>(
        &mut self,
        operation: impl FnOnce(&RepoContext, &mut dyn ExecutionControl) -> Result<T>,
    ) -> Result<T> {
        let lease = &self.lease;
        let cancelled = || lease.renewal_failed();
        let mut control = AdditionalCancellationControl::new(&mut self.observer, &cancelled);
        operation(self.ctx, &mut control)
    }

    pub(super) fn cleanup_candidate(&mut self, worktree: &Path) -> Result<bool> {
        self.refresh("path inspection")?;
        let path_exists =
            inspect_managed_directory(self.ctx.root(), worktree, "PR repair worktree")?;
        self.refresh("registration inspection")?;
        if self.with_control(|ctx, observer| pr_worktree_is_registered(ctx, worktree, observer))? {
            self.remove(worktree, true)?;
            return Ok(true);
        }
        if path_exists {
            self.refresh("partial-directory removal")?;
            fs::remove_dir(worktree).with_context(|| {
                format!(
                    "Failed to remove partial unregistered PR repair worktree {}",
                    worktree.display()
                )
            })?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn failed_worktree_has_evidence(
        &mut self,
        worktree: &Path,
        expected_head: &str,
    ) -> Result<bool> {
        self.refresh("status inspection")?;
        let status = self.with_control(|ctx, observer| {
            git_stdout(ctx, worktree, ["status", "--porcelain"], observer).map_err(pr_step_error)
        })?;
        self.refresh("revision inspection")?;
        let head = self.with_control(|ctx, observer| {
            git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer).map_err(pr_step_error)
        })?;
        Ok(!status.trim().is_empty() || head.trim() != expected_head)
    }

    pub(super) fn remove(&mut self, worktree: &Path, force: bool) -> Result<()> {
        self.refresh("removal")?;
        self.with_control(|ctx, observer| remove_pr_worktree(ctx, worktree, force, observer))
    }
}

fn is_git_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn pr_worktree_root(ctx: &RepoContext, workflow_id: &str) -> PathBuf {
    let digest = Sha256::digest(workflow_id.as_bytes());
    let workflow_key = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    ctx.root()
        .join(LOOP_RUNTIME_DIR)
        .join("worktrees/prs")
        .join(workflow_key)
}

pub(super) fn pr_worktree_path(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
) -> PathBuf {
    pr_worktree_root(ctx, &workflow.id).join(format!(
        "pr-{}-{}",
        item.pr_number,
        bounded_path_component(&item.head_ref)
    ))
}

fn remove_pr_worktree(
    ctx: &RepoContext,
    worktree: &Path,
    force: bool,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    let mut args = vec![OsString::from("worktree"), OsString::from("remove")];
    if force {
        args.push(OsString::from("--force"));
    }
    args.push(worktree.as_os_str().to_os_string());
    match git_output(ctx, ctx.root(), args, observer) {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(git_error("Failed to remove PR repair worktree", output)),
        Err(PrRepairStepError::Cancelled(detail)) => Err(anyhow!(detail)),
        Err(PrRepairStepError::Failed(error)) => Err(error),
    }
}
