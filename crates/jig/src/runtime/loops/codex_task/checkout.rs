use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::execution::NoopExecutionObserver;

use super::{git_is_dirty, git_stdout, remove_worktree, repo_task_has_changes};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskOutcome {
    Succeeded,
    Failed,
}

pub(super) enum PreparedCheckout {
    Repo {
        path: PathBuf,
    },
    Worktree {
        repo_root: PathBuf,
        path: PathBuf,
        initial_head: String,
    },
}

pub(super) struct CheckoutCompletion {
    pub(super) report: CheckoutReport,
    pub(super) error: Option<String>,
}

pub(super) enum CheckoutReport {
    Repository {
        path: PathBuf,
        dirty: Option<bool>,
    },
    Worktree {
        path: PathBuf,
        retained: bool,
        dirty: Option<bool>,
        head_changed: Option<bool>,
    },
}

impl CheckoutReport {
    pub(super) fn repository_requires_attention(&self) -> bool {
        matches!(
            self,
            Self::Repository {
                dirty: Some(true) | None,
                ..
            }
        )
    }

    pub(super) fn retained_worktree(&self) -> Option<String> {
        match self {
            Self::Worktree {
                path,
                retained: true,
                ..
            } => Some(path.display().to_string()),
            Self::Repository { .. } | Self::Worktree { .. } => None,
        }
    }

    pub(super) fn value(&self) -> Value {
        match self {
            Self::Repository { path, dirty } => json!({
                "mode": "repo",
                "path": path,
                "retained": true,
                "dirty": dirty,
            }),
            Self::Worktree {
                path,
                retained,
                dirty,
                head_changed,
            } => json!({
                "mode": "worktree",
                "path": path,
                "retained": retained,
                "dirty": dirty,
                "head_changed": head_changed,
            }),
        }
    }
}

impl PreparedCheckout {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Repo { path } | Self::Worktree { path, .. } => path,
        }
    }

    pub(super) fn finish(self, outcome: TaskOutcome, ctx: &RepoContext) -> CheckoutCompletion {
        let mut cleanup_observer = NoopExecutionObserver;
        match self {
            Self::Repo { path } => match repo_task_has_changes(ctx, &path, &mut cleanup_observer) {
                Ok(dirty) => CheckoutCompletion {
                    report: CheckoutReport::Repository {
                        path,
                        dirty: Some(dirty),
                    },
                    error: None,
                },
                Err(error) => CheckoutCompletion {
                    report: CheckoutReport::Repository { path, dirty: None },
                    error: Some(format!(
                        "Failed to inspect retained task checkout: {error:#}"
                    )),
                },
            },
            Self::Worktree {
                repo_root,
                path,
                initial_head,
            } => {
                let dirty = git_is_dirty(ctx, &path, &mut cleanup_observer);
                let final_head =
                    git_stdout(ctx, &path, ["rev-parse", "HEAD"], &mut cleanup_observer);
                let mut errors = Vec::new();
                if let Err(error) = &dirty {
                    errors.push(format!("Failed to inspect task worktree status: {error:#}"));
                }
                if let Err(error) = &final_head {
                    errors.push(format!("Failed to inspect task worktree HEAD: {error:#}"));
                }
                let dirty = dirty.ok();
                let head_changed = final_head.ok().map(|head| head != initial_head);
                let mut retained = outcome == TaskOutcome::Failed
                    || dirty.unwrap_or(true)
                    || head_changed.unwrap_or(true);
                if !retained
                    && let Err(error) =
                        remove_worktree(ctx, &repo_root, &path, false, &mut cleanup_observer)
                {
                    retained = true;
                    errors.push(format!("Failed to remove clean task worktree: {error:#}"));
                }
                CheckoutCompletion {
                    report: CheckoutReport::Worktree {
                        path,
                        retained,
                        dirty,
                        head_changed,
                    },
                    error: (!errors.is_empty()).then(|| errors.join("; ")),
                }
            }
        }
    }
}
