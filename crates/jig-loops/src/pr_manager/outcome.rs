use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::worktree::{PrWorktreeCleanup, PreparedPrWorktree};
use super::{PrRepairContext, PrWorkItem, pr_worktree_value};
use crate::state::AttemptRecord;
use crate::state::AttemptStore;
use crate::workflow::UnexecutedReason;

pub(super) fn record_pr_repair_outcome<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    outcome: PrRepairOutcome,
    cleanup_authority_error: Option<&anyhow::Error>,
    cleanup: &mut PrWorktreeCleanup<'_>,
) -> Result<Value> {
    match outcome {
        PrRepairOutcome::Completed { action, worktree } => {
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
            let action = with_branch_lease_result(action, cleanup_authority_error);
            Ok(finalize_pr_worktree(cleanup, action, &worktree, false))
        }
        PrRepairOutcome::NeedsAttention { action, worktree } => {
            let action = with_branch_lease_result(action, cleanup_authority_error);
            Ok(finalize_pr_worktree(cleanup, action, &worktree, false))
        }
        PrRepairOutcome::Cancelled { detail, worktree } => Ok(cancelled_before_start_action(
            repair,
            &detail,
            worktree.as_ref(),
            None,
            cleanup_authority_error,
            cleanup,
        )),
        PrRepairOutcome::WorkerCancelled {
            before_start,
            worker,
            worktree,
        } => {
            let timing = if before_start {
                "before the worker started"
            } else {
                "after the worker started"
            };
            let error = format!("PR manager repair was cancelled {timing}");
            if before_start {
                return Ok(cancelled_before_start_action(
                    repair,
                    &error,
                    Some(&worktree),
                    Some(&worker),
                    cleanup_authority_error,
                    cleanup,
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
                    "worktree": pr_worktree_value(worktree.path()),
                    "lease": repair.lease,
                    "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
                    "worker": worker,
                    "error": error,
                }),
                cleanup_authority_error,
            );
            Ok(finalize_pr_worktree(
                cleanup,
                action,
                worktree.path(),
                false,
            ))
        }
        PrRepairOutcome::PreExecutionFailed {
            error,
            worktree,
            worker,
        } => Ok(unexecuted_pr_action(
            repair,
            &format!("{error:#}"),
            worktree.as_ref(),
            worker.as_ref(),
            cleanup_authority_error,
            UnexecutedReason::PreExecutionError,
            cleanup,
        )),
        PrRepairOutcome::WorkerFailed {
            error,
            worker,
            worktree,
        } => {
            let action = failed_pr_repair_action(
                repair,
                attempt_store,
                &error,
                Some(&worktree),
                worker.as_ref(),
            );
            let action = with_branch_lease_result(action, cleanup_authority_error);
            Ok(finalize_failed_pr_worktree(
                repair, &worktree, action, cleanup,
            ))
        }
    }
}

#[cfg(test)]
pub(super) fn record_pr_repair_outcome_under_branch_lease<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    outcome: PrRepairOutcome,
) -> Result<Value> {
    let mut cleanup = PrWorktreeCleanup::assuming_lease(repair.repo);
    record_pr_repair_outcome(repair, attempt_store, outcome, None, &mut cleanup)
}

fn cancelled_before_start_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    detail: &str,
    worktree: Option<&PreparedPrWorktree>,
    worker: Option<&Value>,
    cleanup_authority_error: Option<&anyhow::Error>,
    cleanup: &mut PrWorktreeCleanup<'_>,
) -> Value {
    unexecuted_pr_action(
        repair,
        detail,
        worktree,
        worker,
        cleanup_authority_error,
        UnexecutedReason::CancelledBeforeStart,
        cleanup,
    )
}

fn unexecuted_pr_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    detail: &str,
    worktree: Option<&PreparedPrWorktree>,
    worker: Option<&Value>,
    cleanup_authority_error: Option<&anyhow::Error>,
    reason: UnexecutedReason,
    cleanup: &mut PrWorktreeCleanup<'_>,
) -> Value {
    let mut action = pr_worker_action(
        repair.item,
        repair.lease,
        repair.codex_home,
        "failed",
        detail,
        worktree.map(PreparedPrWorktree::path),
        worker,
    );
    action["unexecuted_reason"] = json!(reason.as_str());
    if let Some(worktree) = worktree {
        if !worktree.created_by_current_attempt() {
            action["status"] = json!("needs_attention");
            action["attention_kind"] = json!("preexisting_repair_worktree_retained");
            action["worktree_retained"] = json!(true);
            action["error"] = json!(format!(
                "{detail}; the pre-existing repair worktree was retained because this attempt did not create it"
            ));
            return with_branch_lease_result(action, cleanup_authority_error);
        }
        if let Some(authority_error) = cleanup_authority_error {
            return branch_lease_cleanup_attention(action, authority_error);
        }
        match cleanup.cleanup_candidate(worktree.path()) {
            Ok(_) => action["worktree_retained"] = json!(false),
            Err(cleanup_error) => return worktree_cleanup_attention(action, cleanup_error),
        }
    } else if let Some(authority_error) = cleanup_authority_error {
        action["lease_error"] = json!(format!("{authority_error:#}"));
        action["error"] = json!(format!(
            "{detail}; branch lease authority proof also failed: {authority_error:#}"
        ));
    }
    action
}

fn failed_pr_repair_action<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    error: &anyhow::Error,
    worktree: Option<&Path>,
    worker: Option<&Value>,
) -> Value {
    let mut action = pr_worker_action(
        repair.item,
        repair.lease,
        repair.codex_home,
        "failed",
        &format!("{error:#}"),
        worktree,
        worker,
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
    worktree: &Path,
    action: Value,
    cleanup: &mut PrWorktreeCleanup<'_>,
) -> Value {
    if action.get("status").and_then(Value::as_str) == Some("needs_attention") {
        return finalize_pr_worktree(cleanup, action, worktree, false);
    }
    match cleanup.failed_worktree_has_evidence(worktree, &repair.item.head_sha) {
        Ok(true) => failed_worktree_attention(action),
        Ok(false) => finalize_pr_worktree(cleanup, action, worktree, false),
        Err(error) => worktree_inspection_attention(action, error),
    }
}

pub(super) fn pr_step_error(error: PrRepairStepError) -> anyhow::Error {
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
        action["inspection_error"]
            .as_str()
            .unwrap_or("unknown inspection failure"),
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

pub(super) fn pr_worker_action(
    item: &PrWorkItem,
    lease: &impl serde::Serialize,
    codex_home: Option<&Path>,
    status: &str,
    error: &str,
    worktree: Option<&Path>,
    worker: Option<&Value>,
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
        action["worktree"] = pr_worktree_value(worktree);
    }
    if let Some(worker) = worker {
        action["worker"] = worker.clone();
    }
    action
}

pub(super) fn finalize_pr_worktree(
    cleanup: &mut PrWorktreeCleanup<'_>,
    mut action: Value,
    worktree: &Path,
    force: bool,
) -> Value {
    let retained = matches!(
        action.get("status").and_then(Value::as_str),
        Some("needs_attention" | "cancelled_after_commit")
    );
    if retained {
        action["worktree_retained"] = json!(true);
        return action;
    }
    match cleanup.remove(worktree, force) {
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

pub(super) fn branch_lease_cleanup_attention(
    mut action: Value,
    authority_error: &anyhow::Error,
) -> Value {
    let completed_error = action["error"].as_str().map(str::to_string);
    action["completed_status"] = action["status"].clone();
    if let Some(completed_error) = completed_error.as_deref() {
        action["completed_error"] = json!(completed_error);
    }
    action["status"] = json!("needs_attention");
    action["attention_kind"] = json!("branch_lease_lost_before_cleanup");
    action["worktree_retained"] = json!(true);
    action["lease_error"] = json!(format!("{authority_error:#}"));
    action["error"] = json!(match completed_error {
        Some(completed_error) => format!(
            "PR repair did not start, but its worktree could not be safely removed because branch lease authority could not be refreshed: {authority_error:#}; completed action: {completed_error}"
        ),
        None => format!(
            "PR repair did not start, but its worktree could not be safely removed because branch lease authority could not be refreshed: {authority_error:#}"
        ),
    });
    action
}

pub(super) enum PrRepairOutcome {
    Completed {
        action: Value,
        worktree: PathBuf,
    },
    NeedsAttention {
        action: Value,
        worktree: PathBuf,
    },
    Cancelled {
        detail: String,
        worktree: Option<PreparedPrWorktree>,
    },
    PreExecutionFailed {
        error: anyhow::Error,
        worktree: Option<PreparedPrWorktree>,
        worker: Option<Value>,
    },
    WorkerFailed {
        error: anyhow::Error,
        worker: Option<Value>,
        worktree: PathBuf,
    },
    WorkerCancelled {
        before_start: bool,
        worker: Value,
        worktree: PreparedPrWorktree,
    },
}

#[derive(Debug)]
pub(super) enum PrRepairStepError {
    Cancelled(String),
    Failed(anyhow::Error),
}

impl PrRepairStepError {
    pub(super) fn failed(error: impl Into<anyhow::Error>) -> Self {
        Self::Failed(error.into())
    }
}

impl From<anyhow::Error> for PrRepairStepError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

pub(super) type PrRepairStepResult<T> = std::result::Result<T, PrRepairStepError>;

#[derive(Debug)]
pub(super) enum PrPushError {
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

pub(super) type PrPushResult<T> = std::result::Result<T, PrPushError>;

pub(super) fn with_attempt(mut action: Value, attempt: AttemptRecord) -> Value {
    if let Some(object) = action.as_object_mut() {
        object.insert("attempt".into(), json!(attempt));
    }
    action
}

pub(super) fn with_branch_lease_result(
    mut action: Value,
    release_error: Option<&anyhow::Error>,
) -> Value {
    if let Some(release_error) = release_error {
        if action.get("lease_error").is_some() {
            action["lease_release_error"] = json!(format!("{release_error:#}"));
            return action;
        }
        let completed_error = action["error"].as_str().map(str::to_string);
        action["completed_status"] = action["status"].clone();
        if let Some(completed_error) = completed_error.as_deref() {
            action["completed_error"] = json!(completed_error);
        }
        action["status"] = json!("needs_attention");
        action["attention_kind"] = json!("branch_lease_lost_after_start");
        action["lease_error"] = json!(format!("{release_error:#}"));
        action["error"] = json!(match completed_error {
            Some(completed_error) => format!(
                "Branch repair completed, but lease renewal or release failed: {release_error:#}; completed action: {completed_error}"
            ),
            None => format!(
                "Branch repair completed, but lease renewal or release failed: {release_error:#}"
            ),
        });
    }
    action
}
