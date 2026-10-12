use std::path::Path;

use anyhow::{Result, anyhow};
use jig_context::RepoContext;
use jig_execution::{AdditionalCancellationControl, ExecutionControl};
use jig_owned_process::ProcessOutputOverflowPolicy;
use jig_state::now_ms;
use serde_json::{Value, json};

use self::git::git_stdout;
use self::outcome::{
    PrPushError, PrRepairOutcome, PrRepairStepError, PrRepairStepResult, record_pr_repair_outcome,
    with_branch_lease_result,
};
use self::pre_push_review::pre_push_review_authority_outcome;
use self::push::{commit_and_push, start_base_merge, validation_tree_after_base_merge};
use self::review_thread_witness::observed_review_thread_ids;
use self::review_threads::post_review_thread_updates;
pub(super) use self::tick::pr_manager_tick;
use self::worker_output::{parse_pr_worker_output, pr_worker_output_schema};
use self::worker_prompt::pr_worker_prompt;
use self::worktree::{PrWorktreeCleanup, PreparedPrWorktree, prepare_worktree};
use super::occurrence::{OccurrenceWorktreeReservation, encode_worktree_path};
use super::state::{AttemptRecord, AttemptStore, LeaseAcquire, LeaseGuard, LeaseStore};
use super::workflow::ResolvedWorkflow;
use crate::worker_runner::{
    CodexExecFailure, CodexExecOutcome, CodexExecRequest, WorkerRunLabel, run_codex_exec,
};

enum PrCandidate {
    Actionable(PrWorkItem),
    Skip(Value),
    Idle(PrIdleItem),
    Pending(PrPendingItem),
}

struct PrManagerExecution<'a> {
    codex_home: Option<&'a Path>,
    worktree_reservation: Option<&'a OccurrenceWorktreeReservation>,
    observer: &'a mut dyn ExecutionControl,
}

struct PrRepairContext<'a, L: serde::Serialize> {
    repo: &'a RepoContext,
    workflow: &'a ResolvedWorkflow,
    item: &'a PrWorkItem,
    lease: &'a L,
    codex_home: Option<&'a Path>,
    worktree_reservation: Option<&'a OccurrenceWorktreeReservation>,
}

struct PrWorkItem {
    pr_number: u64,
    item_key: String,
    title: String,
    base_ref: String,
    head_ref: String,
    head_sha: String,
    reasons: Vec<String>,
}

struct PrIdleItem {
    pr_number: u64,
    item_key: String,
    head_ref: String,
    head_sha: String,
}

struct PrPendingItem {
    pr_number: u64,
    item_key: String,
    head_ref: String,
    head_sha: String,
    pending_checks: u64,
}

fn pr_worktree_value(path: &Path) -> Value {
    Value::String(encode_worktree_path(path))
}

fn classify_pull_request(
    pull_request: &Value,
    default_branch: &str,
    expected_repository: Option<&str>,
) -> PrCandidate {
    let pr_number = pull_request
        .get("number")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let item_key = format!("pr-{pr_number}");
    let head_ref = pull_request
        .pointer("/head/ref")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if pull_request
        .pointer("/stack/is_stacked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return PrCandidate::Skip(skip_action(
            pr_number,
            &item_key,
            "stacked_pr",
            "PR base is not the repository default branch",
        ));
    }
    let cross_repository = pull_request
        .pointer("/head/is_cross_repository")
        .and_then(Value::as_bool);
    let head_repository = pull_request
        .pointer("/head/repository_name_with_owner")
        .and_then(Value::as_str)
        .filter(|repository| !repository.is_empty());
    let same_repository = expected_repository
        .zip(head_repository)
        .is_some_and(|(expected, observed)| expected.eq_ignore_ascii_case(observed));
    if cross_repository != Some(false) || !same_repository {
        let explicitly_cross_repository = cross_repository == Some(true)
            || expected_repository
                .zip(head_repository)
                .is_some_and(|(expected, observed)| !expected.eq_ignore_ascii_case(observed));
        return PrCandidate::Skip(skip_action(
            pr_number,
            &item_key,
            if explicitly_cross_repository {
                "cross_repository_pr"
            } else {
                "unverified_head_repository"
            },
            if explicitly_cross_repository {
                "PR head branch is in another repository"
            } else {
                "PR head repository identity was missing or malformed, so its branch is not proven writable through origin"
            },
        ));
    }
    if head_ref.is_empty() {
        return PrCandidate::Skip(skip_action(
            pr_number,
            &item_key,
            "missing_head_ref",
            "PR does not expose a writable head ref",
        ));
    }
    if pull_request
        .get("is_draft")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return PrCandidate::Skip(skip_action(
            pr_number,
            &item_key,
            "draft_pr",
            "Draft PRs require human intent before automated repair",
        ));
    }

    let mut reasons = Vec::new();
    if pull_request
        .pointer("/mergeability/mergeable")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("CONFLICTING"))
        || pull_request
            .pointer("/mergeability/merge_state_status")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("DIRTY"))
    {
        reasons.push("merge_conflict".to_string());
    }
    if pull_request
        .pointer("/checks/summary/fail")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        reasons.push("failing_checks".to_string());
    }
    let pending_checks = pull_request
        .pointer("/checks/summary/pending")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let trusted_unresolved_threads = pull_request
        .pointer("/review_threads/summary/trusted_unresolved")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if trusted_unresolved_threads > 0 {
        reasons.push("unresolved_review_threads".to_string());
    }
    if pull_request
        .get("review_decision")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("CHANGES_REQUESTED"))
        && trusted_unresolved_threads > 0
    {
        reasons.push("changes_requested".to_string());
    }

    let head_sha = pull_request
        .pointer("/head/sha")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if reasons.is_empty() && pending_checks > 0 {
        return PrCandidate::Pending(PrPendingItem {
            pr_number,
            item_key,
            head_ref: head_ref.to_string(),
            head_sha,
            pending_checks,
        });
    }

    if reasons.is_empty() {
        return PrCandidate::Idle(PrIdleItem {
            pr_number,
            item_key,
            head_ref: head_ref.to_string(),
            head_sha,
        });
    }

    PrCandidate::Actionable(PrWorkItem {
        pr_number,
        item_key,
        title: pull_request
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        base_ref: pull_request
            .pointer("/base/ref")
            .and_then(Value::as_str)
            .unwrap_or(default_branch)
            .to_string(),
        head_ref: head_ref.to_string(),
        head_sha,
        reasons,
    })
}

fn skip_action(pr_number: u64, item_key: &str, reason: &str, detail: &str) -> Value {
    json!({
        "kind": "pr_manager_skip",
        "status": "skipped",
        "pr_number": pr_number,
        "item_key": item_key,
        "reason": reason,
        "detail": detail,
    })
}

fn clear_observed_healthy_attempt(
    workflow: &ResolvedWorkflow,
    attempt_store: &mut AttemptStore,
    item: &PrIdleItem,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Value>> {
    if !attempt_store.clear_attempt_for_observed_version_with_cancellation(
        &workflow.id,
        &item.item_key,
        &item.head_sha,
        cancelled,
    )? {
        return Ok(None);
    }

    Ok(Some(json!({
        "kind": "pr_manager_attempt_clear",
        "status": "skipped",
        "pr_number": item.pr_number,
        "item_key": item.item_key,
        "branch": item.head_ref,
        "head_sha": item.head_sha,
        "reason": "observed_healthy",
        "detail": "PR has no actionable reasons in the latest observed snapshot",
    })))
}

fn pending_checks_action(item: &PrPendingItem) -> Value {
    json!({
        "kind": "pr_manager_wait",
        "status": "waiting",
        "pr_number": item.pr_number,
        "item_key": item.item_key,
        "branch": item.head_ref,
        "head_sha": item.head_sha,
        "reason": "pending_checks",
        "pending_checks": item.pending_checks,
        "detail": "PR checks are still pending; waiting for a completed CI result before classifying the PR as healthy",
    })
}

fn pr_manager_action_consumed_tick(action: &Value) -> bool {
    match action.get("status").and_then(Value::as_str) {
        Some("skipped" | "waiting" | "exhausted") => false,
        Some("needs_attention") => {
            action.get("attention_kind").and_then(Value::as_str) != Some("exhausted_attempt")
        }
        _ => true,
    }
}

fn handle_actionable_pr(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    item: &PrWorkItem,
    pull_request: &Value,
    execution: PrManagerExecution<'_>,
) -> Result<Value> {
    if let Some(action) = attempt_blocking_action(workflow, attempt_store, item, &|| {
        execution.observer.cancelled()
    })? {
        return Ok(action);
    }

    let branch_lease_key = format!("branch:{}", item.head_ref);
    let lease = match lease_store.acquire_with_cancellation(
        &branch_lease_key,
        workflow.lease_ttl_seconds,
        &|| execution.observer.cancelled(),
    )? {
        LeaseAcquire::Acquired(lease) => lease,
        LeaseAcquire::Held(lease) => {
            return Ok(json!({
                "kind": "pr_manager_worker",
                "status": "waiting",
                "pr_number": item.pr_number,
                "item_key": item.item_key,
                "branch": item.head_ref,
                "reasons": item.reasons,
                "lease": lease,
                "detail": "branch lease is already held",
            }));
        }
    };

    let lease_guard = LeaseGuard::start(
        lease_store.clone(),
        &branch_lease_key,
        &lease,
        workflow.lease_ttl_seconds,
    )?;
    let branch_lease_cancelled = || lease_guard.renewal_failed();
    let repair = PrRepairContext {
        repo: ctx,
        workflow,
        item,
        lease: &lease,
        codex_home: execution.codex_home,
        worktree_reservation: execution.worktree_reservation,
    };
    let outcome = {
        let mut branch_control =
            AdditionalCancellationControl::new(execution.observer, &branch_lease_cancelled);
        run_pr_repair(&repair, pull_request, &mut branch_control)
    };
    finalize_pr_repair_outcome(&repair, attempt_store, outcome, lease_guard)
}

fn finalize_pr_repair_outcome<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    attempt_store: &mut AttemptStore,
    outcome: PrRepairOutcome,
    mut lease_guard: LeaseGuard,
) -> Result<Value> {
    let cleanup_authority_error = lease_guard.refresh().err();
    let action = {
        let mut cleanup = PrWorktreeCleanup::new(repair.repo, &mut lease_guard);
        record_pr_repair_outcome(
            repair,
            attempt_store,
            outcome,
            cleanup_authority_error.as_ref(),
            &mut cleanup,
        )?
    };
    let release_error = lease_guard.finish().err();
    Ok(with_branch_lease_result(action, release_error.as_ref()))
}

fn attempt_blocking_action(
    workflow: &ResolvedWorkflow,
    attempt_store: &mut AttemptStore,
    item: &PrWorkItem,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Value>> {
    let Some(attempt) =
        attempt_store.get_with_cancellation(&workflow.id, &item.item_key, cancelled)?
    else {
        return Ok(None);
    };
    if attempt_version_is_stale(&attempt, item) {
        attempt_store.clear_attempt_with_cancellation(&workflow.id, &item.item_key, cancelled)?;
        return Ok(None);
    }
    if attempt.exhausted {
        return Ok(Some(json!({
            "kind": "pr_manager_worker",
            "status": "needs_attention",
            "attention_kind": "exhausted_attempt",
            "pr_number": item.pr_number,
            "item_key": item.item_key,
            "branch": item.head_ref,
            "reasons": item.reasons,
            "attempt": attempt,
            "detail": "attempt budget is exhausted",
        })));
    }
    let now = now_ms();
    if attempt.in_backoff(now) {
        return Ok(Some(json!({
            "kind": "pr_manager_worker",
            "status": "waiting",
            "pr_number": item.pr_number,
            "item_key": item.item_key,
            "branch": item.head_ref,
            "reasons": item.reasons,
            "attempt": attempt,
            "next_eligible_ms": attempt.next_eligible_ms,
            "detail": "attempt is in backoff",
        })));
    }
    Ok(None)
}

fn attempt_version_is_stale(attempt: &AttemptRecord, item: &PrWorkItem) -> bool {
    !item.head_sha.is_empty()
        && attempt.item_version.as_deref() != Some(item.head_sha.as_str())
        && attempt.observed_item_version.as_deref() != Some(item.head_sha.as_str())
}

fn run_pr_repair<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    pull_request: &Value,
    observer: &mut dyn ExecutionControl,
) -> PrRepairOutcome {
    let worktree = match prepare_worktree(
        repair.repo,
        repair.workflow,
        repair.item,
        repair.worktree_reservation,
        observer,
    ) {
        Ok(worktree) => worktree,
        Err(failure) => {
            return match failure.source {
                PrRepairStepError::Cancelled(detail) => PrRepairOutcome::Cancelled {
                    detail,
                    worktree: failure.worktree,
                },
                PrRepairStepError::Failed(error) => PrRepairOutcome::PreExecutionFailed {
                    error,
                    worktree: failure.worktree,
                    worker: None,
                },
            };
        }
    };
    match run_pr_repair_in_worktree(repair, pull_request, &worktree, observer) {
        Ok(outcome) => outcome,
        Err(PrRepairStepError::Cancelled(detail)) => PrRepairOutcome::Cancelled {
            detail,
            worktree: Some(worktree),
        },
        Err(PrRepairStepError::Failed(error)) => PrRepairOutcome::PreExecutionFailed {
            error,
            worktree: Some(worktree),
            worker: None,
        },
    }
}

fn run_pr_repair_in_worktree<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    pull_request: &Value,
    prepared_worktree: &PreparedPrWorktree,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PrRepairOutcome> {
    let worktree = prepared_worktree.path();
    let base_head = git_stdout(repair.repo, worktree, ["rev-parse", "HEAD"], observer)?;
    let merge = if repair
        .item
        .reasons
        .iter()
        .any(|reason| reason == "merge_conflict")
    {
        Some(start_base_merge(
            repair.repo,
            worktree,
            &repair.item.base_ref,
            observer,
        )?)
    } else {
        None
    };
    let validation_tree =
        validation_tree_after_base_merge(repair.repo, worktree, merge.as_ref(), observer)?;
    let prompt = pr_worker_prompt(repair.repo, repair.item, pull_request, merge.as_ref());
    let output_schema = pr_worker_output_schema(observed_review_thread_ids(pull_request).len());
    let worker = match run_codex_exec(
        repair.repo,
        CodexExecRequest {
            root: worktree,
            codex_home: repair.codex_home,
            model: None,
            approval_policy: Some("never"),
            sandbox: Some("workspace-write"),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: Some(&output_schema),
            transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
            prompt: &prompt,
            run: WorkerRunLabel {
                purpose: "pr_manager",
                workflow_id: Some(&repair.workflow.id),
                item_key: Some(&repair.item.item_key),
            },
            phase: None,
        },
        observer,
    ) {
        Err(error) => {
            let failure = error.downcast_ref::<CodexExecFailure>();
            let worker = failure.map(|failure| failure.evidence().clone());
            if failure.is_some_and(CodexExecFailure::worker_was_unexecuted) {
                return Ok(PrRepairOutcome::PreExecutionFailed {
                    error,
                    worktree: Some(prepared_worktree.clone()),
                    worker,
                });
            }
            return Ok(PrRepairOutcome::WorkerFailed {
                error,
                worker,
                worktree: worktree.to_path_buf(),
            });
        }
        Ok(outcome) => match outcome {
            CodexExecOutcome::Completed(worker) => worker,
            CodexExecOutcome::Cancelled {
                before_start,
                evidence: worker,
            } => {
                return Ok(PrRepairOutcome::WorkerCancelled {
                    before_start,
                    worker,
                    worktree: prepared_worktree.clone(),
                });
            }
        },
    };
    if !worker.status().success() {
        return Ok(PrRepairOutcome::WorkerFailed {
            error: anyhow!(
                "PR manager worker exited with status {}",
                worker.status().code().unwrap_or(1)
            ),
            worker: Some(worker.evidence().clone()),
            worktree: worktree.to_path_buf(),
        });
    }

    let worker_output = match parse_pr_worker_output(worker.authoritative_stdout()) {
        Ok(output) => output,
        Err(error) => {
            return Ok(PrRepairOutcome::WorkerFailed {
                error,
                worker: Some(worker.evidence().clone()),
                worktree: worktree.to_path_buf(),
            });
        }
    };
    if let Some(outcome) = pre_push_review_authority_outcome(
        repair,
        pull_request,
        prepared_worktree,
        &worker_output,
        merge.as_ref(),
        worker.evidence(),
        observer,
    ) {
        return Ok(outcome);
    }
    let push = match commit_and_push(
        repair.repo,
        worktree,
        &repair.item.head_ref,
        &base_head,
        merge
            .as_ref()
            .and_then(|merge| merge.get("base_head"))
            .and_then(Value::as_str),
        &validation_tree,
        observer,
    ) {
        Ok(push) => push,
        Err(PrPushError::Ambiguous { error, final_head }) => {
            return Ok(PrRepairOutcome::Completed {
                action: json!({
                    "kind": "pr_manager_worker",
                    "status": "needs_attention",
                    "attention_kind": "ambiguous_push",
                    "pr_number": repair.item.pr_number,
                    "item_key": repair.item.item_key,
                    "title": repair.item.title,
                    "branch": repair.item.head_ref,
                    "head_sha": repair.item.head_sha,
                    "reasons": repair.item.reasons,
                    "worktree": pr_worktree_value(worktree),
                    "lease": repair.lease,
                    "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
                    "merge": merge,
                    "worker_output": worker_output,
                    "worker": worker.evidence(),
                    "push": {
                        "status": "unconfirmed",
                        "pushed": Value::Null,
                        "base_head": base_head,
                        "final_head": final_head,
                        "force": true,
                        "force_with_lease": true,
                        "expected_remote_head": base_head,
                    },
                    "review_thread_posts": [],
                    "error": format!("{error:#}"),
                }),
                worktree: worktree.to_path_buf(),
            });
        }
        Err(PrPushError::Step(error)) => {
            let error = match error {
                PrRepairStepError::Cancelled(detail) => anyhow!(detail),
                PrRepairStepError::Failed(error) => error,
            };
            return Ok(PrRepairOutcome::WorkerFailed {
                error,
                worker: Some(worker.evidence().clone()),
                worktree: worktree.to_path_buf(),
            });
        }
    };
    let repair_version = push["final_head"].as_str().unwrap_or(&repair.item.head_sha);
    let review_thread_posts = post_review_thread_updates(
        repair.repo,
        pull_request,
        &worker_output,
        repair_version,
        observer,
    );
    let status = if review_thread_posts.cancelled {
        "cancelled_after_commit"
    } else if review_thread_posts.failed {
        "failed"
    } else {
        "attempted"
    };
    let error = if review_thread_posts.cancelled {
        Value::String(post_commit_cancellation_error(repair_version))
    } else if review_thread_posts.failed {
        Value::String("one or more review thread update intents failed".into())
    } else {
        Value::Null
    };
    Ok(PrRepairOutcome::Completed {
        action: json!({
            "kind": "pr_manager_worker",
            "status": status,
            "pr_number": repair.item.pr_number,
            "item_key": repair.item.item_key,
            "title": repair.item.title,
            "branch": repair.item.head_ref,
            "head_sha": repair.item.head_sha,
            "reasons": repair.item.reasons,
            "worktree": pr_worktree_value(worktree),
            "lease": repair.lease,
            "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
            "merge": merge,
            "worker_output": worker_output,
            "worker": worker.evidence(),
            "push": push,
            "review_thread_posts": review_thread_posts.posts,
            "error": error,
        }),
        worktree: worktree.to_path_buf(),
    })
}

fn post_commit_cancellation_error(repair_version: &str) -> String {
    format!(
        "PR manager repair was cancelled after pushing {repair_version}; follow-up review thread updates are incomplete"
    )
}

#[cfg(test)]
mod attempt_clear_tests;
#[cfg(test)]
mod cancellation_tests;
mod git;
mod outcome;
mod pre_push_review;
#[cfg(all(test, unix))]
mod preparation_tests;
mod push;
#[cfg(test)]
mod push_error_tests;
#[cfg(all(test, unix))]
mod review_round37_tests;
#[cfg(test)]
mod review_round4_tests;
#[cfg(all(test, unix))]
mod review_thread_boundary_tests;
mod review_thread_budget;
#[cfg(test)]
mod review_thread_budget_tests;
#[cfg(test)]
mod review_thread_capability_tests;
mod review_thread_queries;
mod review_thread_reply;
#[cfg(all(test, unix))]
mod review_thread_reply_tests;
mod review_thread_witness;
mod review_threads;
#[cfg(test)]
mod tests;
mod tick;
mod worker_output;
mod worker_prompt;
mod worktree;
mod worktree_identity;
