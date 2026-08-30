use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use jig_owned_process::ProcessOutputOverflowPolicy;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::bootstrap::{GIT_BIN_ENV, external_program, scrub_known_repository_git_environment};
use crate::context::{CommandTimeout, RepoContext};
use crate::execution::{
    AdditionalCancellationControl, ExecutionCommandError, ExecutionControl, NoopExecutionObserver,
    run_authoritative_execution_command,
};
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecOutcome, CodexExecRequest, CodexPrompt, WorkerReceiptRequest,
    run_codex_exec,
};
use crate::state::now_ms;

use super::github;
use super::state::{
    AttemptRecord, AttemptStore, LOOP_CACHE_DIR, LeaseAcquire, LeaseGuard, LeaseStore,
};
use super::workflow::{ResolvedWorkflow, WorkflowTick};

pub(super) fn pr_manager_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkflowTick> {
    let codex_home = workflow
        .codex_home_configured
        .as_deref()
        .map(|home| crate::codex::resolve_configured_home_from_dir(home, ctx.root()))
        .transpose()?;
    let observed = github::github_pr_status_snapshot(ctx, observer)?;
    pr_manager_tick_from_snapshot(
        ctx,
        workflow,
        lease_store,
        attempt_store,
        observed,
        PrManagerExecution {
            codex_home: codex_home.as_deref(),
            observer,
        },
    )
}

fn pr_manager_tick_from_snapshot(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    observed: Value,
    execution: PrManagerExecution<'_>,
) -> Result<WorkflowTick> {
    let pull_requests = observed
        .get("pull_requests")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("GitHub PR snapshot did not include pull_requests array"))?;
    if let Some(action) = incomplete_snapshot_action(&observed, pull_requests) {
        return Ok(WorkflowTick::from_actions(observed, vec![action]));
    }
    let default_branch = observed
        .pointer("/repository/default_branch")
        .and_then(Value::as_str)
        .unwrap_or_else(|| ctx.default_branch());

    let mut actions = Vec::new();
    for pull_request in pull_requests {
        if execution.observer.cancelled() {
            bail!("PR manager tick was cancelled");
        }
        let candidate = classify_pull_request(pull_request, default_branch);
        match candidate {
            PrCandidate::Skip(action) => actions.push(action),
            PrCandidate::Idle(item) => {
                if let Some(action) =
                    clear_observed_healthy_attempt(workflow, attempt_store, &item)?
                {
                    actions.push(action);
                }
            }
            PrCandidate::Pending(item) => actions.push(pending_checks_action(&item)),
            PrCandidate::Actionable(item) => {
                let action = handle_actionable_pr(
                    ctx,
                    workflow,
                    lease_store,
                    attempt_store,
                    &item,
                    pull_request,
                    PrManagerExecution {
                        codex_home: execution.codex_home,
                        observer: &mut *execution.observer,
                    },
                )?;
                let consumed_tick = pr_manager_action_consumed_tick(&action);
                actions.push(action);
                if consumed_tick {
                    break;
                }
            }
        }
    }

    Ok(WorkflowTick::from_actions(observed, actions))
}

fn incomplete_snapshot_action(observed: &Value, pull_requests: &[Value]) -> Option<Value> {
    let pr_list_truncated = observed
        .pointer("/summary/pr_list_truncated")
        .and_then(Value::as_bool)
        == Some(true);
    let review_threads_truncated = pull_requests.iter().any(|pull_request| {
        pull_request
            .pointer("/review_threads/page_info/truncated")
            .and_then(Value::as_bool)
            == Some(true)
    });
    let truncated_review_threads = pull_requests
        .iter()
        .filter(|pull_request| {
            pull_request
                .pointer("/review_threads/page_info/truncated")
                .and_then(Value::as_bool)
                == Some(true)
        })
        .filter_map(|pull_request| pull_request.get("number").and_then(Value::as_u64))
        .collect::<Vec<_>>();
    if !pr_list_truncated && !review_threads_truncated {
        return None;
    }
    Some(json!({
        "kind": "pr_manager_observation",
        "status": "failed",
        "reason": "incomplete_github_snapshot",
        "pr_list_truncated": pr_list_truncated,
        "review_thread_prs_truncated": truncated_review_threads,
        "error": "PR manager refused to mutate attempts or branches from an incomplete GitHub snapshot",
    }))
}

enum PrCandidate {
    Actionable(PrWorkItem),
    Skip(Value),
    Idle(PrIdleItem),
    Pending(PrPendingItem),
}

struct PrManagerExecution<'a> {
    codex_home: Option<&'a Path>,
    observer: &'a mut dyn ExecutionControl,
}

struct PrRepairContext<'a, L: serde::Serialize> {
    repo: &'a RepoContext,
    workflow: &'a ResolvedWorkflow,
    item: &'a PrWorkItem,
    lease: &'a L,
    codex_home: Option<&'a Path>,
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

fn classify_pull_request(pull_request: &Value, default_branch: &str) -> PrCandidate {
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
    if pull_request
        .pointer("/head/is_cross_repository")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return PrCandidate::Skip(skip_action(
            pr_number,
            &item_key,
            "cross_repository_pr",
            "PR head branch is in another repository",
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
    if pull_request
        .pointer("/review_threads/summary/unresolved")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        reasons.push("unresolved_review_threads".to_string());
    }
    if pull_request
        .get("review_decision")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("CHANGES_REQUESTED"))
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
) -> Result<Option<Value>> {
    if !attempt_store.clear_attempt(&workflow.id, &item.item_key)? {
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
    if let Some(action) = attempt_blocking_action(workflow, attempt_store, item)? {
        return Ok(action);
    }

    let branch_lease_key = format!("branch:{}", item.head_ref);
    let lease = match lease_store.acquire(&branch_lease_key, workflow.lease_ttl_seconds)? {
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
    };
    let action_result = {
        let mut branch_control =
            AdditionalCancellationControl::new(execution.observer, &branch_lease_cancelled);
        run_pr_repair(&repair, pull_request, &mut branch_control)
    };
    let release_error = lease_guard.finish().err();
    record_pr_repair_outcome(
        &repair,
        attempt_store,
        action_result,
        release_error.as_ref(),
    )
}

include!("pr_manager/outcome.rs");

fn attempt_blocking_action(
    workflow: &ResolvedWorkflow,
    attempt_store: &mut AttemptStore,
    item: &PrWorkItem,
) -> Result<Option<Value>> {
    let Some(attempt) = attempt_store.get(&workflow.id, &item.item_key)? else {
        return Ok(None);
    };
    if attempt_version_is_stale(&attempt, item) {
        attempt_store.clear_attempt(&workflow.id, &item.item_key)?;
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
) -> Result<PrRepairOutcome> {
    let worktree = match prepare_worktree(repair.repo, repair.workflow, repair.item, observer) {
        Ok(worktree) => worktree,
        Err(PrRepairStepError::Cancelled(detail)) => {
            return Ok(PrRepairOutcome::Cancelled {
                detail,
                worktree: None,
            });
        }
        Err(PrRepairStepError::Failed(error)) => return Err(error),
    };
    match run_pr_repair_in_worktree(repair, pull_request, &worktree, observer) {
        Ok(outcome) => Ok(outcome),
        Err(PrRepairStepError::Cancelled(detail)) => Ok(PrRepairOutcome::Cancelled {
            detail,
            worktree: Some(worktree),
        }),
        Err(PrRepairStepError::Failed(error)) => Ok(PrRepairOutcome::Failed { error, worktree }),
    }
}

fn run_pr_repair_in_worktree<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    pull_request: &Value,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PrRepairOutcome> {
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
    let prompt = pr_worker_prompt(repair.repo, repair.item, pull_request, merge.as_ref());
    let output_schema = pr_worker_output_schema();
    let worker = match run_codex_exec(
        repair.repo,
        CodexExecRequest {
            root: worktree,
            codex_home: repair.codex_home,
            mode: CodexExecMode::Exec,
            model: None,
            approval_policy: Some("never"),
            sandbox: Some("workspace-write"),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: Some(&output_schema),
            transcript_overflow_policy: ProcessOutputOverflowPolicy::Truncate,
            prompt: CodexPrompt::Stdin(&prompt),
            receipt: WorkerReceiptRequest {
                purpose: "pr_manager",
                plan_id: None,
                workflow_id: Some(&repair.workflow.id),
                item_key: Some(&repair.item.item_key),
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
            },
            phase: None,
        },
        observer,
    )? {
        CodexExecOutcome::Completed(worker) => worker,
        CodexExecOutcome::Cancelled {
            before_start,
            worker_receipt_id,
        } => {
            return Ok(PrRepairOutcome::WorkerCancelled {
                before_start,
                worker_receipt_id,
                worktree: worktree.to_path_buf(),
            });
        }
    };
    if !worker.status().success() {
        return Err(PrRepairStepError::failed(anyhow!(
            "PR manager worker exited with status {}",
            worker.status().code().unwrap_or(1)
        )));
    }

    let worker_output = parse_pr_worker_output(worker.authoritative_stdout())?;
    let push = match commit_and_push(
        repair.repo,
        worktree,
        &repair.item.head_ref,
        &base_head,
        observer,
    ) {
        Ok(push) => push,
        Err(PrPushError::Ambiguous { error, final_head }) => {
            return Ok(PrRepairOutcome::Completed(json!({
                "kind": "pr_manager_worker",
                "status": "needs_attention",
                "attention_kind": "ambiguous_push",
                "pr_number": repair.item.pr_number,
                "item_key": repair.item.item_key,
                "title": repair.item.title,
                "branch": repair.item.head_ref,
                "head_sha": repair.item.head_sha,
                "reasons": repair.item.reasons,
                "worktree": worktree,
                "lease": repair.lease,
                "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
                "merge": merge,
                "worker_output": worker_output,
                "worker_receipt_id": worker.worker_receipt_id(),
                "push": {
                    "status": "unconfirmed",
                    "pushed": Value::Null,
                    "base_head": base_head,
                    "final_head": final_head,
                    "force": false,
                },
                "review_thread_posts": [],
                "error": format!("{error:#}"),
            })));
        }
        Err(PrPushError::Step(error)) => return Err(error),
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
    Ok(PrRepairOutcome::Completed(json!({
        "kind": "pr_manager_worker",
        "status": status,
        "pr_number": repair.item.pr_number,
        "item_key": repair.item.item_key,
        "title": repair.item.title,
        "branch": repair.item.head_ref,
        "head_sha": repair.item.head_sha,
        "reasons": repair.item.reasons,
        "worktree": worktree,
        "lease": repair.lease,
        "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
        "merge": merge,
        "worker_output": worker_output,
        "worker_receipt_id": worker.worker_receipt_id(),
        "push": push,
        "review_thread_posts": review_thread_posts.posts,
        "error": error,
    })))
}

fn post_commit_cancellation_error(repair_version: &str) -> String {
    format!(
        "PR manager repair was cancelled after pushing {repair_version}; follow-up review thread updates are incomplete"
    )
}

fn pr_worker_output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "review_thread_replies"],
        "properties": {
            "summary": {
                "type": "string",
                "description": "Concise summary of the repair attempt."
            },
            "review_thread_replies": {
                "type": "array",
                "description": "GitHub review thread reply intents for Jig to post outside the sandbox.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["thread_id"],
                    "properties": {
                        "thread_id": {
                            "type": "string",
                            "description": "GitHub pull request review thread node ID, such as PRRT_..."
                        },
                        "body": {
                            "type": "string",
                            "description": "Reply body to post. Leave empty only for resolve-only actions."
                        },
                        "resolve": {
                            "type": "boolean",
                            "description": "Whether Jig should resolve the review thread after posting any reply."
                        }
                    }
                }
            }
        }
    })
}

fn parse_pr_worker_output(stdout: &[u8]) -> Result<Value> {
    if stdout.is_empty() {
        bail!("PR manager worker did not write structured output");
    }
    let value = serde_json::from_slice::<Value>(stdout)
        .context("Failed to parse PR manager worker structured output")?;
    let replies = value
        .get("review_thread_replies")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("PR manager worker output did not include review_thread_replies"))?;
    for reply in replies {
        let thread_id = reply
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if thread_id.is_empty() {
            bail!("PR manager worker output included a reply without thread_id");
        }
        let body = reply
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let resolve = reply
            .get("resolve")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if body.is_empty() && !resolve {
            bail!("PR manager worker output for thread {thread_id} has no body or resolve action");
        }
    }
    Ok(value)
}

include!("pr_manager/review_threads.rs");
include!("pr_manager/worktree_and_push.rs");
include!("pr_manager/push_error_tests.rs");
include!("pr_manager/review_round4_tests.rs");
include!("pr_manager/cancellation_tests.rs");
include!("pr_manager/git.rs");
include!("pr_manager/tests.rs");
