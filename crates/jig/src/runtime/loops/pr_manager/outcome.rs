fn record_pr_repair_outcome(
    workflow: &ResolvedWorkflow,
    attempt_store: &mut AttemptStore,
    item: &PrWorkItem,
    lease: &impl serde::Serialize,
    codex_home: Option<&Path>,
    action_result: Result<PrRepairOutcome>,
    release_error: Option<&anyhow::Error>,
) -> Result<Value> {
    match action_result {
        Ok(PrRepairOutcome::Completed(action)) => {
            let item_version = action
                .pointer("/push/final_head")
                .and_then(Value::as_str)
                .filter(|version| !version.is_empty())
                .or(Some(item.head_sha.as_str()));
            let attempt_status = action
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| *status == "failed")
                .unwrap_or("attempted");
            let attempt = attempt_store.record_attempt_for_transition(
                workflow,
                &item.item_key,
                Some(&item.head_sha),
                item_version,
                attempt_status,
            )?;
            Ok(with_branch_lease_result(
                with_attempt(action, attempt),
                release_error,
            ))
        }
        Ok(PrRepairOutcome::Cancelled(detail)) => {
            if let Some(release_error) = release_error {
                bail!("{detail}; branch lease renewal or release also failed: {release_error:#}")
            }
            bail!(detail)
        }
        Ok(PrRepairOutcome::WorkerCancelled {
            before_start,
            worker_receipt_id,
            worktree,
        }) => {
            let timing = if before_start {
                "before the worker started"
            } else {
                "while the worker was running"
            };
            let error = format!("PR manager repair was cancelled {timing}");
            if before_start {
                if let Some(release_error) = release_error {
                    bail!("{error}; branch lease renewal or release also failed: {release_error:#}")
                }
                bail!("{error}; worker receipt {worker_receipt_id}")
            }
            Ok(with_branch_lease_result(
                json!({
                    "kind": "pr_manager_worker",
                    "status": "needs_attention",
                    "attention_kind": "cancelled_after_start",
                    "pr_number": item.pr_number,
                    "item_key": item.item_key,
                    "title": item.title,
                    "branch": item.head_ref,
                    "head_sha": item.head_sha,
                    "reasons": item.reasons,
                    "worktree": worktree,
                    "lease": lease,
                    "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                    "worker_receipt_id": worker_receipt_id,
                    "error": error,
                }),
                release_error,
            ))
        }
        Err(error) => {
            let attempt = attempt_store.record_attempt_for_transition(
                workflow,
                &item.item_key,
                Some(&item.head_sha),
                Some(&item.head_sha),
                "failed",
            )?;
            Ok(json!({
                "kind": "pr_manager_worker",
                "status": "failed",
                "pr_number": item.pr_number,
                "item_key": item.item_key,
                "branch": item.head_ref,
                "reasons": item.reasons,
                "lease": lease,
                "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
                "attempt": attempt,
                "error": release_error.map_or_else(
                    || format!("{error:#}"),
                    |release_error| format!(
                        "{error:#}; branch lease renewal or release also failed: {release_error:#}"
                    ),
                ),
            }))
        }
    }
}

enum PrRepairOutcome {
    Completed(Value),
    Cancelled(String),
    WorkerCancelled {
        before_start: bool,
        worker_receipt_id: String,
        worktree: PathBuf,
    },
}

#[derive(Debug)]
enum PrRepairStepError {
    Cancelled(String),
    Failed(anyhow::Error),
}

impl PrRepairStepError {
    fn failed(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed(error.into())
    }
}

impl From<anyhow::Error> for PrRepairStepError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

type PrRepairStepResult<T> = std::result::Result<T, PrRepairStepError>;

#[derive(Debug)]
enum PrPushError {
    Step(PrRepairStepError),
    Ambiguous {
        error: anyhow::Error,
        final_head: String,
    },
}

impl From<PrRepairStepError> for PrPushError {
    fn from(error: PrRepairStepError) -> Self {
        Self::Step(error)
    }
}

type PrPushResult<T> = std::result::Result<T, PrPushError>;
