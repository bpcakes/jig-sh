fn record_pr_repair_outcome<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    action_result: Result<PrRepairOutcome>,
    release_error: Option<&anyhow::Error>,
) -> Result<Value> {
    match action_result {
        Ok(PrRepairOutcome::Completed(action)) => {
            let item_version = action
                .pointer("/push/final_head")
                .and_then(Value::as_str)
                .filter(|version| !version.is_empty())
                .or(Some(repair.item.head_sha.as_str()));
            let attempt_status = action
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| *status == "failed")
                .unwrap_or("attempted");
            let attempt = attempt_store.record_attempt_for_transition(
                repair.workflow,
                &repair.item.item_key,
                Some(&repair.item.head_sha),
                item_version,
                attempt_status,
            )?;
            let action = with_branch_lease_result(
                with_attempt(action, attempt),
                release_error,
            );
            Ok(finalize_pr_worktree(repair.repo, action, false))
        }
        Ok(PrRepairOutcome::Cancelled { detail, worktree }) => {
            if let Some(worktree) = worktree
                && let Err(cleanup_error) = remove_pr_worktree(repair.repo, &worktree, true)
            {
                let error = release_error.map_or_else(
                    || detail.clone(),
                    |release_error| {
                        format!(
                            "{detail}; branch lease renewal or release also failed: {release_error:#}"
                        )
                    },
                );
                return Ok(worktree_cleanup_attention(
                    pr_worker_action(
                        repair.item,
                        repair.lease,
                        repair.codex_home,
                        "failed",
                        &error,
                        Some(&worktree),
                        None,
                    ),
                    cleanup_error,
                ));
            }
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
                if let Err(cleanup_error) = remove_pr_worktree(repair.repo, &worktree, true) {
                    let error = match release_error {
                        Some(release_error) => format!(
                            "{error}; branch lease renewal or release also failed: {release_error:#}"
                        ),
                        None => error,
                    };
                    return Ok(worktree_cleanup_attention(
                        pr_worker_action(
                            repair.item,
                            repair.lease,
                            repair.codex_home,
                            "failed",
                            &error,
                            Some(&worktree),
                            Some(&worker_receipt_id),
                        ),
                        cleanup_error,
                    ));
                }
                if let Some(release_error) = release_error {
                    bail!("{error}; branch lease renewal or release also failed: {release_error:#}")
                }
                bail!("{error}; worker receipt {worker_receipt_id}")
            }
            let action = with_branch_lease_result(
                json!({
                    "kind": "pr_manager_worker",
                    "status": "needs_attention",
                    "attention_kind": "cancelled_after_start",
                    "pr_number": repair.item.pr_number,
                    "item_key": repair.item.item_key,
                    "title": repair.item.title,
                    "branch": repair.item.head_ref,
                    "head_sha": repair.item.head_sha,
                    "reasons": repair.item.reasons,
                    "worktree": worktree,
                    "lease": repair.lease,
                    "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
                    "worker_receipt_id": worker_receipt_id,
                    "error": error,
                }),
                release_error,
            );
            Ok(finalize_pr_worktree(repair.repo, action, false))
        }
        Ok(PrRepairOutcome::Failed { error, worktree }) => {
            let action = failed_pr_repair_action(
                repair,
                attempt_store,
                &error,
                release_error,
                Some(&worktree),
            )?;
            Ok(finalize_pr_worktree(repair.repo, action, true))
        }
        Err(error) => {
            failed_pr_repair_action(
                repair,
                attempt_store,
                &error,
                release_error,
                None,
            )
        }
    }
}

fn failed_pr_repair_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    error: &anyhow::Error,
    release_error: Option<&anyhow::Error>,
    worktree: Option<&Path>,
) -> Result<Value> {
    let attempt = attempt_store.record_attempt_for_transition(
        repair.workflow,
        &repair.item.item_key,
        Some(&repair.item.head_sha),
        Some(&repair.item.head_sha),
        "failed",
    )?;
    let mut action = pr_worker_action(
        repair.item,
        repair.lease,
        repair.codex_home,
        "failed",
        &release_error.map_or_else(
            || format!("{error:#}"),
            |release_error| {
                format!(
                    "{error:#}; branch lease renewal or release also failed: {release_error:#}"
                )
            },
        ),
        worktree,
        None,
    );
    action["attempt"] = json!(attempt);
    Ok(action)
}

fn pr_worker_action(
    item: &PrWorkItem,
    lease: &impl serde::Serialize,
    codex_home: Option<&Path>,
    status: &str,
    error: &str,
    worktree: Option<&Path>,
    worker_receipt_id: Option<&str>,
) -> Value {
    let mut action = json!({
        "kind": "pr_manager_worker",
        "status": status,
        "pr_number": item.pr_number,
        "item_key": item.item_key,
        "title": item.title,
        "branch": item.head_ref,
        "head_sha": item.head_sha,
        "reasons": item.reasons,
        "lease": lease,
        "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
        "error": error,
    });
    if let Some(worktree) = worktree {
        action["worktree"] = json!(worktree);
    }
    if let Some(worker_receipt_id) = worker_receipt_id {
        action["worker_receipt_id"] = json!(worker_receipt_id);
    }
    action
}

fn finalize_pr_worktree(ctx: &RepoContext, mut action: Value, force: bool) -> Value {
    let retained = matches!(
        action.get("status").and_then(Value::as_str),
        Some("needs_attention" | "cancelled_after_commit")
    );
    if retained {
        action["worktree_retained"] = json!(true);
        return action;
    }
    let Some(worktree) = action
        .get("worktree")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return action;
    };
    match remove_pr_worktree(ctx, &worktree, force) {
        Ok(()) => {
            action["worktree_retained"] = json!(false);
            action
        }
        Err(error) => worktree_cleanup_attention(action, error),
    }
}

fn worktree_cleanup_attention(mut action: Value, cleanup_error: anyhow::Error) -> Value {
    let completed_status = action["status"].clone();
    let completed_error = action["error"].as_str().map(str::to_string);
    let cleanup_error = format!("{cleanup_error:#}");
    action["completed_status"] = completed_status;
    if let Some(completed_error) = completed_error.as_deref() {
        action["completed_error"] = json!(completed_error);
    }
    action["status"] = json!("needs_attention");
    action["attention_kind"] = json!("worktree_cleanup_failed");
    action["worktree_retained"] = json!(true);
    action["cleanup_error"] = json!(cleanup_error);
    action["error"] = json!(match completed_error {
        Some(completed_error) => format!(
            "PR repair worktree cleanup failed: {cleanup_error}; completed action: {completed_error}"
        ),
        None => format!("PR repair worktree cleanup failed: {cleanup_error}"),
    });
    action
}

enum PrRepairOutcome {
    Completed(Value),
    Cancelled {
        detail: String,
        worktree: Option<PathBuf>,
    },
    Failed {
        error: anyhow::Error,
        worktree: PathBuf,
    },
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
