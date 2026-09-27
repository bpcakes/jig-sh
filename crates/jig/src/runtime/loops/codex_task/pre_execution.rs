use super::*;
use sha2::{Digest, Sha256};

use super::super::managed_path::{ensure_managed_directory, inspect_managed_directory};
use super::super::state::LOOP_RUNTIME_DIR;
use super::super::workflow::RepositoryRevisionState;

pub(super) fn prepare_checkout(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item_key: &str,
    checkout: CodexTaskCheckout,
    worktree_reservation: Option<&OccurrenceWorktreeReservation>,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<PreparedCheckout, CheckoutPreparationFailure> {
    if checkout == CodexTaskCheckout::Repo {
        return prepare_repository_checkout(ctx, observer);
    }

    if observer.cancelled() {
        return Err(CheckoutPreparationFailure::cancelled(anyhow!(
            "Scheduled Codex task was cancelled before worktree preflight"
        )));
    }
    let digest = Sha256::digest(format!("{}\0{item_key}", workflow.id).as_bytes());
    let name = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = ctx
        .root()
        .join(LOOP_RUNTIME_DIR)
        .join("worktrees")
        .join("tasks")
        .join(name);
    let path_exists = classify_checkout_preflight(
        inspect_managed_directory(ctx.root(), &path, "Codex task worktree"),
        observer,
    )?;
    if path_exists {
        return Err(CheckoutPreparationFailure::retained(
            &path,
            anyhow!("Codex task worktree already exists: {}", path.display()),
        ));
    }
    if let Err(error) = require_ignored_task_worktree_root(ctx, observer) {
        return Err(if observer.cancelled() {
            CheckoutPreparationFailure::cancelled(error)
        } else {
            CheckoutPreparationFailure::new(error)
        });
    }
    if let Some(parent) = path.parent() {
        classify_checkout_preflight(
            ensure_managed_directory(ctx.root(), parent, "Codex task worktree parent"),
            observer,
        )?;
    }
    let initial_head = classify_checkout_preflight(
        git_stdout(ctx, ctx.root(), ["rev-parse", "HEAD"], observer),
        observer,
    )?;
    if let Some(reservation) = worktree_reservation {
        classify_checkout_preflight(reservation.reserve(&path), observer)?;
    }
    let output = match git_output(
        ctx,
        ctx.root(),
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--detach"),
            path.as_os_str().to_os_string(),
            OsString::from(&initial_head),
        ],
        observer,
    ) {
        Ok(output) => output,
        Err(error) => {
            let cancelled = observer.cancelled();
            let mut cleanup_observer = NoopExecutionObserver;
            let cleanup =
                cleanup_failed_worktree(ctx, &path, worktree_reservation, &mut cleanup_observer);
            return Err(checkout_preparation_error(
                &path,
                error,
                cleanup.err(),
                cancelled,
            ));
        }
    };
    if !output.status.success() {
        let error = git_error("Failed to create Codex task worktree", output);
        let mut cleanup_observer = NoopExecutionObserver;
        let cleanup =
            cleanup_failed_worktree(ctx, &path, worktree_reservation, &mut cleanup_observer);
        return Err(checkout_preparation_error(
            &path,
            error,
            cleanup.err(),
            false,
        ));
    }
    Ok(PreparedCheckout::Worktree {
        repo_root: ctx.root().to_path_buf(),
        path,
        initial_head,
    })
}

pub(super) fn prepare_repository_checkout(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<PreparedCheckout, CheckoutPreparationFailure> {
    if observer.cancelled() {
        return Err(CheckoutPreparationFailure::cancelled(anyhow!(
            "Scheduled Codex task was cancelled before shared-checkout preflight"
        )));
    }
    super::super::pre_execution::require_ignored_runtime_path(
        ctx,
        Path::new(LOOP_RUNTIME_DIR),
        "Codex task runtime path",
        "repo checkout",
        observer,
    )?;
    match repo_task_has_changes(ctx, ctx.root(), observer) {
        Ok(false) => {}
        Ok(true) => {
            return Err(CheckoutPreparationFailure::new(anyhow!(
                "Shared repository checkout is dirty before Codex task execution; preserve or discard the existing changes before retrying"
            )));
        }
        Err(error) => {
            let error = error.context(
                "Failed to verify that the shared repository checkout is clean before Codex task execution",
            );
            return Err(if observer.cancelled() {
                CheckoutPreparationFailure::cancelled(error)
            } else {
                CheckoutPreparationFailure::new(error)
            });
        }
    }
    let initial_head = classify_checkout_preflight(
        git_stdout(ctx, ctx.root(), ["rev-parse", "HEAD"], observer),
        observer,
    )?;
    if observer.cancelled() {
        return Err(CheckoutPreparationFailure::cancelled(anyhow!(
            "Scheduled Codex task was cancelled before receipt-journal preflight"
        )));
    }
    let receipt_journal =
        classify_checkout_preflight(checkout::ReceiptJournalBaseline::capture(ctx), observer)?;
    Ok(PreparedCheckout::Repo {
        path: ctx.root().to_path_buf(),
        initial_head,
        receipt_journal,
    })
}

pub(super) fn classify_checkout_preflight<T>(
    result: Result<T>,
    observer: &dyn ExecutionControl,
) -> std::result::Result<T, CheckoutPreparationFailure> {
    result.map_err(|error| {
        if observer.cancelled() {
            CheckoutPreparationFailure::cancelled(error)
        } else {
            CheckoutPreparationFailure::new(error)
        }
    })
}

pub(super) fn require_ignored_task_worktree_root(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> Result<()> {
    super::super::pre_execution::require_ignored_runtime_path(
        ctx,
        Path::new(LOOP_RUNTIME_DIR),
        "Loop runtime root",
        "worktree checkout",
        observer,
    )?;
    super::super::pre_execution::require_ignored_runtime_path(
        ctx,
        &Path::new(LOOP_RUNTIME_DIR).join("worktrees/tasks"),
        "Codex task worktree path",
        "worktree checkout",
        observer,
    )
}

pub(super) fn unexecuted_task_failure(
    settings: &CodexTaskSettings,
    reason: UnexecutedReason,
    item_key: &str,
    codex_home: Option<&Path>,
    retained_worktree: Option<String>,
    error: String,
) -> WorkflowTick {
    let needs_attention = retained_worktree.is_some();
    let status = if needs_attention {
        "needs_attention"
    } else {
        "failed"
    };
    let action = json!({
        "kind": "codex_task_worker",
        "status": status,
        "item_key": item_key,
        "worker_started": false,
        "worker_receipt_id": Value::Null,
        "checkout": {
            "mode": settings.checkout.as_str(),
            "path": retained_worktree,
            "retained": needs_attention,
        },
        "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
        "output": Value::Null,
        "error": error,
    });
    WorkflowTick::with_completion(
        json!({
            "kind": "codex_task",
            "prompt_file": settings.prompt_file.display().to_string(),
            "sandbox": settings.sandbox,
            "checkout": settings.checkout.as_str(),
        }),
        vec![action],
        WorkflowCompletion {
            outcome: if needs_attention {
                WorkflowOutcome::NeedsAttention
            } else {
                WorkflowOutcome::Failed
            },
            execution: WorkflowExecution::Unexecuted(reason),
            repository_revision: RepositoryRevisionState::NotApplicable,
            worker_receipt_id: None,
            worktree: retained_worktree,
            error: Some(error),
        },
    )
}

#[derive(Debug)]
pub(super) struct CheckoutPreparationFailure {
    retained_worktree: Option<String>,
    reason: UnexecutedReason,
    error: anyhow::Error,
}

impl CheckoutPreparationFailure {
    pub(super) fn new(error: impl Into<anyhow::Error>) -> Self {
        Self {
            retained_worktree: None,
            reason: UnexecutedReason::PreExecutionError,
            error: error.into(),
        }
    }

    pub(super) fn cancelled(error: impl Into<anyhow::Error>) -> Self {
        Self {
            retained_worktree: None,
            reason: UnexecutedReason::CancelledBeforeStart,
            error: error.into(),
        }
    }

    pub(super) fn retained(path: &Path, error: impl Into<anyhow::Error>) -> Self {
        Self {
            retained_worktree: Some(super::super::occurrence::encode_worktree_path(path)),
            reason: UnexecutedReason::PreExecutionError,
            error: error.into(),
        }
    }

    pub(super) fn retained_worktree(&self) -> Option<&str> {
        self.retained_worktree.as_deref()
    }

    pub(super) const fn reason(&self) -> UnexecutedReason {
        self.reason
    }
}

impl std::fmt::Display for CheckoutPreparationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, formatter)
    }
}

impl std::error::Error for CheckoutPreparationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

impl From<anyhow::Error> for CheckoutPreparationFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::new(error)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Context as _;

    use super::*;

    #[test]
    fn alternate_display_preserves_checkout_error_context() {
        let source = Err::<(), _>(std::io::Error::other("disk unavailable"))
            .context("failed to create checkout parent")
            .unwrap_err();
        let failure = CheckoutPreparationFailure::new(source);

        assert_eq!(
            format!("{failure:#}"),
            "failed to create checkout parent: disk unavailable"
        );
    }
}
