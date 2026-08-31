fn record_pr_repair_outcome<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    outcome: PrRepairOutcome,
    release_error: Option<&anyhow::Error>,
) -> Result<Value> {
    match outcome {
        PrRepairOutcome::Completed(action) => {
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
            );
            let action = match attempt {
                Ok(attempt) => with_attempt(action, attempt),
                Err(error) => attempt_state_attention(action, error),
            };
            let action = with_branch_lease_result(action, release_error);
            Ok(finalize_pr_worktree(repair.repo, action, false))
        }
        PrRepairOutcome::Cancelled { detail, worktree } => {
            Ok(cancelled_before_start_action(
                repair,
                &detail,
                worktree.as_deref(),
                None,
                release_error,
            ))
        }
        PrRepairOutcome::WorkerCancelled {
            before_start,
            worker_receipt_id,
            worktree,
        } => {
            let timing = if before_start {
                "before the worker started"
            } else {
                "while the worker was running"
            };
            let error = format!("PR manager repair was cancelled {timing}");
            if before_start {
                return Ok(cancelled_before_start_action(
                    repair,
                    &error,
                    Some(&worktree),
                    Some(&worker_receipt_id),
                    release_error,
                ));
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
        PrRepairOutcome::PreExecutionFailed {
            error,
            worktree,
            worker_receipt_id,
        } => Ok(unexecuted_pr_action(
            repair,
            &format!("{error:#}"),
            worktree.as_deref(),
            worker_receipt_id.as_deref(),
            release_error,
            UnexecutedReason::PreExecutionError,
        )),
        PrRepairOutcome::PreparationCleanupFailed {
            error,
            cleanup_error,
            worktree,
            reason,
        } => Ok(preparation_cleanup_attention(
            repair,
            error,
            cleanup_error,
            worktree,
            reason,
            release_error,
        )),
        PrRepairOutcome::WorkerFailed {
            error,
            worker_receipt_id,
            worktree,
        } => {
            let action = failed_pr_repair_action(
                repair,
                attempt_store,
                &error,
                Some(&worktree),
                worker_receipt_id.as_deref(),
            );
            let action = with_branch_lease_result(action, release_error);
            Ok(finalize_failed_pr_worktree(repair, action))
        }
    }
}

fn preparation_cleanup_attention<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    error: anyhow::Error,
    cleanup_error: anyhow::Error,
    worktree: PathBuf,
    reason: UnexecutedReason,
    release_error: Option<&anyhow::Error>,
) -> Value {
    let error = format!("{error:#}");
    let cleanup_error = format!("{cleanup_error:#}");
    let mut action = pr_worker_action(
        repair.item,
        repair.lease,
        repair.codex_home,
        "needs_attention",
        &format!(
            "PR repair preparation failed and its worktree could not be cleaned while the branch lease was held: {cleanup_error}; preparation error: {error}"
        ),
        Some(&worktree),
        None,
    );
    action["attention_kind"] = json!("worktree_cleanup_failed");
    action["unexecuted_reason"] = json!(reason.as_str());
    action["worktree_retained"] = json!(true);
    action["completed_status"] = json!("failed");
    action["completed_error"] = json!(error);
    action["cleanup_error"] = json!(cleanup_error);
    if let Some(release_error) = release_error {
        action["lease_error"] = json!(format!("{release_error:#}"));
        action["error"] = json!(format!(
            "{}; branch lease renewal or release also failed: {release_error:#}",
            action["error"]
                .as_str()
                .unwrap_or("PR repair preparation cleanup failed")
        ));
    }
    action
}

fn cancelled_before_start_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    detail: &str,
    worktree: Option<&Path>,
    worker_receipt_id: Option<&str>,
    release_error: Option<&anyhow::Error>,
) -> Value {
    unexecuted_pr_action(
        repair,
        detail,
        worktree,
        worker_receipt_id,
        release_error,
        UnexecutedReason::CancelledBeforeStart,
    )
}

fn unexecuted_pr_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    detail: &str,
    worktree: Option<&Path>,
    worker_receipt_id: Option<&str>,
    release_error: Option<&anyhow::Error>,
    reason: UnexecutedReason,
) -> Value {
    let mut action = pr_worker_action(
        repair.item,
        repair.lease,
        repair.codex_home,
        "failed",
        detail,
        worktree,
        worker_receipt_id,
    );
    action["unexecuted_reason"] = json!(reason.as_str());
    if let Some(worktree) = worktree {
        if let Some(release_error) = release_error {
            return branch_lease_cleanup_attention(action, release_error);
        }
        match remove_pr_worktree(repair.repo, worktree, true) {
            Ok(()) => action["worktree_retained"] = json!(false),
            Err(cleanup_error) => return worktree_cleanup_attention(action, cleanup_error),
        }
    } else if let Some(release_error) = release_error {
        action["lease_error"] = json!(format!("{release_error:#}"));
        action["error"] = json!(format!(
            "{detail}; branch lease renewal or release also failed: {release_error:#}"
        ));
    }
    action
}

fn failed_pr_repair_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    error: &anyhow::Error,
    worktree: Option<&Path>,
    worker_receipt_id: Option<&str>,
) -> Value {
    let mut action = pr_worker_action(
        repair.item,
        repair.lease,
        repair.codex_home,
        "failed",
        &format!("{error:#}"),
        worktree,
        worker_receipt_id,
    );
    let attempt = attempt_store.record_attempt_for_transition(
        repair.workflow,
        &repair.item.item_key,
        Some(&repair.item.head_sha),
        Some(&repair.item.head_sha),
        "failed",
    );
    match attempt {
        Ok(attempt) => action["attempt"] = json!(attempt),
        Err(error) => return attempt_state_attention(action, error),
    }
    action
}

fn finalize_failed_pr_worktree<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    action: Value,
) -> Value {
    if action.get("status").and_then(Value::as_str) == Some("needs_attention") {
        return finalize_pr_worktree(repair.repo, action, false);
    }
    let Some(worktree) = action
        .get("worktree")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return action;
    };
    match failed_worktree_has_evidence(repair.repo, &worktree, &repair.item.head_sha) {
        Ok(true) => failed_worktree_attention(action),
        Ok(false) => finalize_pr_worktree(repair.repo, action, false),
        Err(error) => worktree_inspection_attention(action, error),
    }
}

fn failed_worktree_has_evidence(
    ctx: &RepoContext,
    worktree: &Path,
    expected_head: &str,
) -> Result<bool> {
    let mut observer = NoopExecutionObserver;
    let status = git_stdout(ctx, worktree, ["status", "--porcelain"], &mut observer)
        .map_err(pr_step_error)?;
    let head = git_stdout(ctx, worktree, ["rev-parse", "HEAD"], &mut observer)
        .map_err(pr_step_error)?;
    Ok(!status.trim().is_empty() || head.trim() != expected_head)
}

fn pr_step_error(error: PrRepairStepError) -> anyhow::Error {
    match error {
        PrRepairStepError::Cancelled(detail) => anyhow!(detail),
        PrRepairStepError::Failed(error) => error,
    }
}

fn failed_worktree_attention(mut action: Value) -> Value {
    let completed_error = action["error"].clone();
    action["completed_status"] = action["status"].clone();
    action["completed_error"] = completed_error.clone();
    action["status"] = json!("needs_attention");
    action["attention_kind"] = json!("failed_repair_worktree_retained");
    action["worktree_retained"] = json!(true);
    action["error"] = json!(format!(
        "PR repair failed after producing local worktree evidence; inspect or acknowledge the retained worktree: {}",
        completed_error.as_str().unwrap_or("unknown repair failure")
    ));
    action
}

fn worktree_inspection_attention(mut action: Value, inspection_error: anyhow::Error) -> Value {
    let completed_error = action["error"].clone();
    let inspection_error = format!("{inspection_error:#}");
    action["completed_status"] = action["status"].clone();
    action["completed_error"] = completed_error.clone();
    action["status"] = json!("needs_attention");
    action["attention_kind"] = json!("failed_repair_worktree_inspection_failed");
    action["worktree_retained"] = json!(true);
    action["inspection_error"] = json!(inspection_error);
    action["error"] = json!(format!(
        "PR repair failed and its worktree could not be proven disposable: {}; completed action: {}",
        action["inspection_error"].as_str().unwrap_or("unknown inspection failure"),
        completed_error.as_str().unwrap_or("unknown repair failure")
    ));
    action
}

fn attempt_state_attention(mut action: Value, attempt_error: anyhow::Error) -> Value {
    let completed_status = action["status"].clone();
    let completed_error = action["error"].as_str().map(str::to_string);
    let attempt_error = format!("{attempt_error:#}");
    action["completed_status"] = completed_status;
    if let Some(completed_error) = completed_error.as_deref() {
        action["completed_error"] = json!(completed_error);
    }
    action["status"] = json!("needs_attention");
    action["attention_kind"] = json!("attempt_state_persistence_failed");
    action["attempt_error"] = json!(attempt_error);
    action["error"] = json!(match completed_error {
        Some(completed_error) => format!(
            "PR repair evidence requires attention because attempt state persistence failed: {attempt_error}; completed action: {completed_error}"
        ),
        None => format!(
            "PR repair evidence requires attention because attempt state persistence failed: {attempt_error}"
        ),
    });
    action
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

fn branch_lease_cleanup_attention(mut action: Value, release_error: &anyhow::Error) -> Value {
    let completed_error = action["error"].as_str().map(str::to_string);
    action["completed_status"] = action["status"].clone();
    if let Some(completed_error) = completed_error.as_deref() {
        action["completed_error"] = json!(completed_error);
    }
    action["status"] = json!("needs_attention");
    action["attention_kind"] = json!("branch_lease_lost_before_cleanup");
    action["worktree_retained"] = json!(true);
    action["lease_error"] = json!(format!("{release_error:#}"));
    action["error"] = json!(match completed_error {
        Some(completed_error) => format!(
            "PR repair did not start, but its worktree could not be safely removed after branch lease authority was lost: {release_error:#}; completed action: {completed_error}"
        ),
        None => format!(
            "PR repair did not start, but its worktree could not be safely removed after branch lease authority was lost: {release_error:#}"
        ),
    });
    action
}

enum PrRepairOutcome {
    Completed(Value),
    Cancelled {
        detail: String,
        worktree: Option<PathBuf>,
    },
    PreExecutionFailed {
        error: anyhow::Error,
        worktree: Option<PathBuf>,
        worker_receipt_id: Option<String>,
    },
    PreparationCleanupFailed {
        error: anyhow::Error,
        cleanup_error: anyhow::Error,
        worktree: PathBuf,
        reason: UnexecutedReason,
    },
    WorkerFailed {
        error: anyhow::Error,
        worker_receipt_id: Option<String>,
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
