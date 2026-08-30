use super::*;

pub(super) fn unexecuted_task_failure(
    settings: &CodexTaskSettings,
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
            execution: WorkflowExecution::Unexecuted(UnexecutedReason::PreExecutionError),
            worker_receipt_id: None,
            worktree: retained_worktree,
            error: Some(error),
        },
    )
}

#[derive(Debug)]
pub(super) struct CheckoutPreparationFailure {
    retained_worktree: Option<String>,
    error: anyhow::Error,
}

impl CheckoutPreparationFailure {
    pub(super) fn new(error: impl Into<anyhow::Error>) -> Self {
        Self {
            retained_worktree: None,
            error: error.into(),
        }
    }

    pub(super) fn retained(path: &Path, error: impl Into<anyhow::Error>) -> Self {
        Self {
            retained_worktree: Some(path.display().to_string()),
            error: error.into(),
        }
    }

    pub(super) fn retained_worktree(&self) -> Option<&str> {
        self.retained_worktree.as_deref()
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
