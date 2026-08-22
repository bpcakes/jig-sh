use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use jig_owned_process::ProcessOutputOverflowPolicy;
use serde_json::{Value, json};

use crate::bootstrap::{GIT_BIN_ENV, external_program, scrub_known_repository_git_environment};
use crate::context::{CommandTimeout, RepoContext};
use crate::execution::{
    ExecutionCommandError, ExecutionControl, NoopExecutionObserver,
    run_authoritative_execution_command,
};
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecOutcome, CodexExecRequest, CodexPrompt, WorkerReceiptRequest,
    run_codex_exec,
};
use crate::state::now_ms;

use super::{
    AttemptRecord, AttemptStore, LeaseAcquire, LeaseStore, ResolvedWorkflow, WorkflowTick, github,
};

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
    let pull_requests = observed
        .get("pull_requests")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("GitHub PR snapshot did not include pull_requests array"))?;
    let default_branch = observed
        .pointer("/repository/default_branch")
        .and_then(Value::as_str)
        .unwrap_or_else(|| ctx.default_branch());

    let mut actions = Vec::new();
    for pull_request in pull_requests {
        if observer.cancelled() {
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
                        codex_home: codex_home.as_deref(),
                        observer: &mut *observer,
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

    Ok(WorkflowTick { observed, actions })
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
    !matches!(
        action.get("status").and_then(Value::as_str),
        Some("skipped" | "waiting" | "needs_attention")
    )
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

    let action_result = run_pr_repair(
        ctx,
        workflow,
        item,
        pull_request,
        &lease,
        execution.codex_home,
        execution.observer,
    );
    let _ = lease_store.release(&branch_lease_key, &lease.owner);
    record_pr_repair_outcome(
        workflow,
        attempt_store,
        item,
        &lease,
        execution.codex_home,
        action_result,
    )
}

fn record_pr_repair_outcome(
    workflow: &ResolvedWorkflow,
    attempt_store: &mut AttemptStore,
    item: &PrWorkItem,
    lease: &impl serde::Serialize,
    codex_home: Option<&Path>,
    action_result: Result<PrRepairOutcome>,
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
            let attempt = attempt_store.record_attempt_for_version(
                workflow,
                &item.item_key,
                item_version,
                attempt_status,
            )?;
            Ok(with_attempt(action, attempt))
        }
        Ok(PrRepairOutcome::Cancelled(detail)) => bail!(detail),
        Err(error) => {
            let attempt = attempt_store.record_attempt_for_version(
                workflow,
                &item.item_key,
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
                "error": format!("{error:#}"),
            }))
        }
    }
}

enum PrRepairOutcome {
    Completed(Value),
    Cancelled(String),
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
    !item.head_sha.is_empty() && attempt.item_version.as_deref() != Some(item.head_sha.as_str())
}

fn run_pr_repair(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
    pull_request: &Value,
    lease: &impl serde::Serialize,
    codex_home: Option<&Path>,
    observer: &mut dyn ExecutionControl,
) -> Result<PrRepairOutcome> {
    match run_pr_repair_steps(
        ctx,
        workflow,
        item,
        pull_request,
        lease,
        codex_home,
        observer,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(PrRepairStepError::Cancelled(detail)) => Ok(PrRepairOutcome::Cancelled(detail)),
        Err(PrRepairStepError::Failed(error)) => Err(error),
    }
}

fn run_pr_repair_steps(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
    pull_request: &Value,
    lease: &impl serde::Serialize,
    codex_home: Option<&Path>,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PrRepairOutcome> {
    let worktree = prepare_worktree(ctx, workflow, item, observer)?;
    let base_head = git_stdout(ctx, &worktree, ["rev-parse", "HEAD"], observer)?;
    let merge = if item.reasons.iter().any(|reason| reason == "merge_conflict") {
        Some(start_base_merge(ctx, &worktree, &item.base_ref, observer)?)
    } else {
        None
    };
    let prompt = pr_worker_prompt(ctx, item, pull_request, merge.as_ref());
    let output_schema = pr_worker_output_schema();
    let worker = match run_codex_exec(
        ctx,
        CodexExecRequest {
            root: &worktree,
            codex_home,
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
                workflow_id: Some(&workflow.id),
                item_key: Some(&item.item_key),
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
            let timing = if before_start {
                " before the worker started"
            } else {
                " while the worker was running"
            };
            return Ok(PrRepairOutcome::Cancelled(format!(
                "PR manager repair was cancelled{timing}; worker receipt {worker_receipt_id}"
            )));
        }
    };
    if !worker.output.status.success() {
        return Err(PrRepairStepError::failed(anyhow!(
            "PR manager worker exited with status {}",
            worker.output.status.code().unwrap_or(1)
        )));
    }

    let worker_output = parse_pr_worker_output(&worker.output.stdout)?;
    let push = commit_and_push(ctx, &worktree, &item.head_ref, &base_head, observer)?;
    let review_thread_posts =
        post_review_thread_updates(ctx, pull_request, &worker_output, observer);
    let status = if review_thread_posts.cancelled {
        "cancelled_after_commit"
    } else if review_thread_posts.failed {
        "failed"
    } else {
        "attempted"
    };
    let error = if review_thread_posts.cancelled {
        Value::String(format!(
            "PR manager repair was cancelled after pushing {}; follow-up review thread updates are incomplete",
            push["final_head"]
        ))
    } else if review_thread_posts.failed {
        Value::String("one or more review thread update intents failed".into())
    } else {
        Value::Null
    };
    Ok(PrRepairOutcome::Completed(json!({
        "kind": "pr_manager_worker",
        "status": status,
        "pr_number": item.pr_number,
        "item_key": item.item_key,
        "title": item.title,
        "branch": item.head_ref,
        "head_sha": item.head_sha,
        "reasons": item.reasons,
        "worktree": worktree,
        "lease": lease,
        "codex_home_resolved": codex_home.map(|home| home.display().to_string()),
        "merge": merge,
        "worker_output": worker_output,
        "worker_receipt_id": worker.worker_receipt_id,
        "push": push,
        "review_thread_posts": review_thread_posts.posts,
        "error": error,
    })))
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

struct ReviewThreadPostResult {
    posts: Value,
    failed: bool,
    cancelled: bool,
}

fn post_review_thread_updates(
    ctx: &RepoContext,
    pull_request: &Value,
    worker_output: &Value,
    observer: &mut dyn ExecutionControl,
) -> ReviewThreadPostResult {
    let empty = Vec::new();
    let replies = worker_output
        .get("review_thread_replies")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let allowed_thread_ids = observed_review_thread_ids(pull_request);
    let mut posts = Vec::new();
    let mut failed = false;
    let mut cancelled = false;
    for reply in replies {
        if observer.cancelled() {
            cancelled = true;
            break;
        }
        let thread_id = reply
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let body = reply
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let resolve = reply
            .get("resolve")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !allowed_thread_ids.contains(thread_id) {
            posts.push(json!({
                "thread_id": thread_id,
                "status": "skipped",
                "reason": "unknown_review_thread",
                "detail": "worker requested a review thread that was not present in the observed PR snapshot",
                "replied": false,
                "reply_comment_id": Value::Null,
                "reply_url": Value::Null,
                "reply_error": Value::Null,
                "resolved": false,
                "is_resolved": Value::Null,
                "resolve_error": Value::Null,
                "resolve_skipped": false,
                "resolve_skip_reason": Value::Null,
            }));
            continue;
        }

        let mut thread_failed = false;
        let mut reply_error = Value::Null;
        let reply_response = if body.is_empty() {
            None
        } else {
            match post_review_thread_reply(ctx, thread_id, body, observer) {
                Ok(response) => Some(response),
                Err(
                    ExecutionCommandError::CancelledBeforeStart | ExecutionCommandError::Cancelled,
                ) => {
                    cancelled = true;
                    thread_failed = true;
                    reply_error = Value::String("review thread reply was cancelled".into());
                    None
                }
                Err(ExecutionCommandError::Failed(error)) => {
                    failed = true;
                    thread_failed = true;
                    reply_error = Value::String(format!("{error:#}"));
                    None
                }
            }
        };
        let mut resolve_error = Value::Null;
        let mut resolve_skipped = false;
        let mut resolve_skip_reason = Value::Null;
        let resolve_response = if cancelled {
            resolve_skipped = resolve;
            resolve_skip_reason = if resolve {
                Value::String("cancelled".into())
            } else {
                Value::Null
            };
            None
        } else if resolve && thread_failed && !body.is_empty() {
            resolve_skipped = true;
            resolve_skip_reason = Value::String("reply_failed".into());
            None
        } else if resolve {
            match resolve_review_thread(ctx, thread_id, observer) {
                Ok(response) => Some(response),
                Err(
                    ExecutionCommandError::CancelledBeforeStart | ExecutionCommandError::Cancelled,
                ) => {
                    cancelled = true;
                    thread_failed = true;
                    resolve_error = Value::String("review thread resolution was cancelled".into());
                    None
                }
                Err(ExecutionCommandError::Failed(error)) => {
                    failed = true;
                    thread_failed = true;
                    resolve_error = Value::String(format!("{error:#}"));
                    None
                }
            }
        } else {
            None
        };
        posts.push(json!({
            "thread_id": thread_id,
            "status": if cancelled { "cancelled" } else if thread_failed { "failed" } else { "posted" },
            "replied": reply_response.is_some(),
            "reply_comment_id": reply_response
                .as_ref()
                .and_then(|value| value.pointer("/data/addPullRequestReviewThreadReply/comment/id"))
                .cloned()
                .unwrap_or(Value::Null),
            "reply_url": reply_response
                .as_ref()
                .and_then(|value| value.pointer("/data/addPullRequestReviewThreadReply/comment/url"))
                .cloned()
                .unwrap_or(Value::Null),
            "reply_error": reply_error,
            "resolved": resolve_response.is_some(),
            "is_resolved": resolve_response
                .as_ref()
                .and_then(|value| value.pointer("/data/resolveReviewThread/thread/isResolved"))
                .cloned()
                .unwrap_or(Value::Null),
            "resolve_error": resolve_error,
            "resolve_skipped": resolve_skipped,
            "resolve_skip_reason": resolve_skip_reason,
        }));
        if cancelled {
            break;
        }
    }
    ReviewThreadPostResult {
        posts: json!(posts),
        failed,
        cancelled,
    }
}

fn observed_review_thread_ids(pull_request: &Value) -> BTreeSet<String> {
    pull_request
        .pointer("/review_threads/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|thread| thread.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn post_review_thread_reply(
    ctx: &RepoContext,
    thread_id: &str,
    body: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    github::gh_json(
        ctx,
        vec![
            OsString::from("api"),
            OsString::from("graphql"),
            OsString::from("-f"),
            OsString::from(format!("query={}", add_review_thread_reply_mutation())),
            OsString::from("-f"),
            OsString::from(format!("threadId={thread_id}")),
            OsString::from("-f"),
            OsString::from(format!("body={body}")),
        ],
        &[0],
        observer,
    )
}

fn resolve_review_thread(
    ctx: &RepoContext,
    thread_id: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    github::gh_json(
        ctx,
        vec![
            OsString::from("api"),
            OsString::from("graphql"),
            OsString::from("-f"),
            OsString::from(format!("query={}", resolve_review_thread_mutation())),
            OsString::from("-f"),
            OsString::from(format!("threadId={thread_id}")),
        ],
        &[0],
        observer,
    )
}

const fn add_review_thread_reply_mutation() -> &'static str {
    r"
mutation($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $threadId, body: $body}) {
    comment {
      id
      url
    }
  }
}
"
}

const fn resolve_review_thread_mutation() -> &'static str {
    r"
mutation($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) {
    thread {
      id
      isResolved
    }
  }
}
"
}

fn prepare_worktree(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PathBuf> {
    let worktree = ctx
        .root()
        .join(super::LOOP_CACHE_DIR)
        .join("worktrees")
        .join(&workflow.id)
        .join(format!(
            "pr-{}-{}",
            item.pr_number,
            sanitize_path_component(&item.head_ref)
        ));
    let parent = worktree
        .parent()
        .ok_or_else(|| anyhow!("Worktree path has no parent: {}", worktree.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;

    git_checked(
        ctx,
        ctx.root(),
        ["fetch", "origin", &item.head_ref],
        observer,
    )?;
    if worktree.join(".git").exists() {
        clean_reused_worktree(ctx, &worktree, observer)?;
        git_checked(
            ctx,
            &worktree,
            ["fetch", "origin", &item.head_ref],
            observer,
        )?;
        git_checked(
            ctx,
            &worktree,
            ["checkout", "--detach", "FETCH_HEAD"],
            observer,
        )?;
    } else {
        git_checked(
            ctx,
            ctx.root(),
            vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--detach"),
                worktree.as_os_str().to_os_string(),
                OsString::from("FETCH_HEAD"),
            ],
            observer,
        )?;
    }

    git_checked(
        ctx,
        &worktree,
        ["config", "user.name", "Jig PR Manager"],
        observer,
    )?;
    git_checked(
        ctx,
        &worktree,
        [
            "config",
            "user.email",
            "jig-pr-manager@users.noreply.github.com",
        ],
        observer,
    )?;
    Ok(worktree)
}

fn clean_reused_worktree(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()> {
    match git_output(ctx, worktree, ["merge", "--abort"], observer) {
        Ok(_) | Err(PrRepairStepError::Failed(_)) => {}
        Err(cancelled @ PrRepairStepError::Cancelled(_)) => return Err(cancelled),
    }
    git_checked(ctx, worktree, ["reset", "--hard"], observer)?;
    git_checked(ctx, worktree, ["clean", "-fd"], observer)?;
    Ok(())
}

fn start_base_merge(
    ctx: &RepoContext,
    worktree: &Path,
    base_ref: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Value> {
    let fetch = git_output(ctx, worktree, ["fetch", "origin", base_ref], observer)?;
    if !fetch.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git fetch base branch failed",
            fetch,
        )));
    }
    let merge = git_output(
        ctx,
        worktree,
        ["merge", "--no-edit", "FETCH_HEAD"],
        observer,
    )?;
    Ok(json!({
        "exit_status": merge.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&merge.stdout),
        "stderr": String::from_utf8_lossy(&merge.stderr),
        "conflicts": !merge.status.success(),
    }))
}

fn commit_and_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    base_head: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Value> {
    let dirty_before_commit = git_stdout(ctx, worktree, ["status", "--porcelain"], observer)?;
    if !dirty_before_commit.trim().is_empty() {
        git_checked(ctx, worktree, ["add", "-A"], observer)?;
        git_checked(
            ctx,
            worktree,
            [
                "commit",
                "-m",
                &format!("chore: update PR via Jig PR manager ({head_ref})"),
            ],
            observer,
        )?;
    }
    let final_head = git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer)?;
    let changed = final_head.trim() != base_head.trim();
    if !changed {
        return Ok(json!({
            "status": "no_changes",
            "pushed": false,
            "base_head": base_head.trim(),
            "final_head": final_head.trim(),
        }));
    }

    let push_ref = format!("HEAD:refs/heads/{head_ref}");
    let push_args = ["push", "origin", &push_ref];
    let push_result = git_execution_output(worktree, push_args, ctx.command_timeout(), observer);
    let push_error = match push_result {
        Ok(push) if push.status.success() => None,
        Ok(push) => Some(PrRepairStepError::failed(git_error(
            "git push failed without force",
            push,
        ))),
        Err(error) => Some(pr_git_execution_error("git push", error)),
    };
    if let Some(push_error) = push_error {
        let reconciliation = reconcile_remote_push(ctx, worktree, head_ref, final_head.trim());
        if reconciliation.confirmed {
            return Ok(push_result_value(
                base_head,
                &final_head,
                Some(reconciliation.detail),
            ));
        }
        return Err(match push_error {
            PrRepairStepError::Cancelled(detail) => PrRepairStepError::Cancelled(format!(
                "{detail}; push outcome was not confirmed: {}",
                reconciliation.detail
            )),
            PrRepairStepError::Failed(error) => PrRepairStepError::Failed(error.context(format!(
                "push outcome was not confirmed: {}",
                reconciliation.detail
            ))),
        });
    }

    Ok(push_result_value(base_head, &final_head, None))
}

fn push_result_value(base_head: &str, final_head: &str, reconciliation: Option<String>) -> Value {
    let mut value = json!({
        "status": "pushed",
        "pushed": true,
        "base_head": base_head.trim(),
        "final_head": final_head.trim(),
        "force": false,
    });
    if let Some(reconciliation) = reconciliation {
        value["reconciliation"] = Value::String(reconciliation);
    }
    value
}

struct PushReconciliation {
    confirmed: bool,
    detail: String,
}

fn reconcile_remote_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    final_head: &str,
) -> PushReconciliation {
    let remote_ref = format!("refs/heads/{head_ref}");
    let mut observer = NoopExecutionObserver;
    let timeout_seconds = ctx.command_timeout().as_secs().min(30);
    let timeout = CommandTimeout::from_seconds(timeout_seconds)
        .expect("the reconciliation timeout is nonzero and within the command timeout range");
    let output = git_execution_output(
        worktree,
        ["ls-remote", "--exit-code", "origin", &remote_ref],
        timeout,
        &mut observer,
    );
    match output {
        Ok(output) if output.status.success() => {
            let observed = remote_head_from_ls_remote(&output.stdout, &remote_ref);
            PushReconciliation {
                confirmed: observed == Some(final_head),
                detail: match observed {
                    Some(observed) => {
                        format!("remote {remote_ref} resolved to {observed}; expected {final_head}")
                    }
                    None => format!("remote {remote_ref} returned no matching head"),
                },
            }
        }
        Ok(output) => PushReconciliation {
            confirmed: false,
            detail: format!(
                "remote {remote_ref} reconciliation exited with status {}",
                output.status.code().unwrap_or(1)
            ),
        },
        Err(error) => PushReconciliation {
            confirmed: false,
            detail: format!("remote {remote_ref} reconciliation failed: {error}"),
        },
    }
}

fn remote_head_from_ls_remote<'a>(stdout: &'a [u8], remote_ref: &str) -> Option<&'a str> {
    std::str::from_utf8(stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find_map(|(head, reference)| (reference.trim() == remote_ref).then_some(head.trim()))
}

fn pr_worker_prompt(
    ctx: &RepoContext,
    item: &PrWorkItem,
    pull_request: &Value,
    merge: Option<&Value>,
) -> String {
    format!(
        "You are Jig's PR manager worker for repository `{}`.\n\
         Work only on PR #{} (`{}`) on branch `{}`. Reasons: {}.\n\
         Resolve the reported PR issues in this isolated worktree. If merge conflicts are present, resolve them completely. \
         If CI is failing, inspect the failing checks and fix the underlying code. \
         If unresolved review threads are present, address the actionable feedback with code changes when possible. \
         Do not use `gh`, `curl`, or network access to reply to or resolve review threads. \
         Instead, return review-thread reply intents in the required structured output. \
         Include a reply intent only when a concise comment or resolution is needed after your code changes; set `resolve` only when the feedback is fully addressed.\n\
         Run relevant local tests when available. Do not merge the PR. Do not force-push. Keep changes minimal and commit them if you change files. \
         Always write structured output with `summary` and `review_thread_replies`.\n\n\
         Merge preparation result:\n{}\n\n\
         Normalized PR snapshot:\n{}\n",
        ctx.repo_name(),
        item.pr_number,
        item.title,
        item.head_ref,
        item.reasons.join(", "),
        merge
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".into()),
        serde_json::to_string_pretty(pull_request).unwrap_or_else(|_| pull_request.to_string()),
    )
}

fn with_attempt(mut action: Value, attempt: AttemptRecord) -> Value {
    if let Some(object) = action.as_object_mut() {
        object.insert("attempt".into(), json!(attempt));
    }
    action
}

#[cfg(test)]
mod cancellation_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;

    use super::*;

    struct CancelledControl;

    impl crate::execution::ExecutionObserver for CancelledControl {}

    impl crate::execution::ExecutionCancellation for CancelledControl {
        fn cancelled(&self) -> bool {
            true
        }
    }

    struct CancelAfterStart(AtomicUsize);

    impl crate::execution::ExecutionObserver for CancelAfterStart {}

    impl crate::execution::ExecutionCancellation for CancelAfterStart {
        fn cancelled(&self) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst) > 0
        }
    }

    #[test]
    fn cancelled_repair_does_not_consume_attempt_budget() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 1,
            backoff_seconds: 1,
            codex_home_configured: None,
        };
        let item = PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "abc123".into(),
            reasons: vec!["failing_checks".into()],
        };
        let mut attempt_store = AttemptStore::new(&ctx);

        let mut observer = CancelledControl;
        let action_result = run_pr_repair(
            &ctx,
            &workflow,
            &item,
            &json!({}),
            &json!({"owner": "test"}),
            None,
            &mut observer,
        )
        .unwrap();
        let PrRepairOutcome::Cancelled(detail) = &action_result else {
            panic!("pre-start Git cancellation must cancel the repair");
        };
        assert!(detail.contains("git fetch was cancelled before it started"));

        let error = record_pr_repair_outcome(
            &workflow,
            &mut attempt_store,
            &item,
            &json!({"owner": "test"}),
            None,
            Ok(action_result),
        )
        .unwrap_err();

        assert!(error.to_string().contains("git fetch was cancelled"));
        assert!(attempt_store.snapshot().unwrap().is_empty());
    }

    #[test]
    fn post_commit_cancellation_records_the_pushed_head() {
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let workflow = ResolvedWorkflow {
            id: "pr-manager".into(),
            kind: super::super::PR_MANAGER_KIND.into(),
            enabled: true,
            configured: true,
            lease_ttl_seconds: 60,
            max_attempts: 2,
            backoff_seconds: 1,
            codex_home_configured: None,
        };
        let item = PrWorkItem {
            pr_number: 7,
            item_key: "pr-7".into(),
            title: "Example repair".into(),
            base_ref: "main".into(),
            head_ref: "repair/example".into(),
            head_sha: "observed-head".into(),
            reasons: vec!["failing_checks".into()],
        };
        let mut attempt_store = AttemptStore::new(&ctx);

        let action = record_pr_repair_outcome(
            &workflow,
            &mut attempt_store,
            &item,
            &json!({"owner": "test"}),
            None,
            Ok(PrRepairOutcome::Completed(json!({
                "kind": "pr_manager_worker",
                "status": "cancelled_after_commit",
                "push": {"final_head": "pushed-head"},
            }))),
        )
        .unwrap();

        assert_eq!(action["status"], "cancelled_after_commit");
        assert_eq!(action["attempt"]["item_version"], "pushed-head");
        assert_eq!(action["attempt"]["last_status"], "attempted");
        let attempts = attempt_store.snapshot().unwrap();
        assert_eq!(attempts[0].item_version.as_deref(), Some("pushed-head"));
    }

    #[cfg(unix)]
    #[test]
    fn in_flight_git_cancellation_remains_typed() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        let _env_lock = lock_env();
        let temp = tempdir().unwrap();
        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let git = temp.path().join("slow-git");
        fs::write(&git, "#!/bin/sh\nsleep 60\n").unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, git.as_os_str());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut observer = CancelAfterStart(AtomicUsize::new(0));

        let error = git_output(&ctx, temp.path(), ["fetch"], &mut observer).unwrap_err();

        let PrRepairStepError::Cancelled(detail) = error else {
            panic!("in-flight Git cancellation must remain typed");
        };
        assert!(detail.contains("git fetch was cancelled while it was running"));
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_push_is_reconciled_when_the_remote_received_the_commit() {
        use std::os::unix::fs::PermissionsExt;

        use crate::test_env::{EnvVarGuard, lock_env};

        fn checked_git(cwd: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        struct MarkerCancellation(PathBuf);

        impl crate::execution::ExecutionObserver for MarkerCancellation {}

        impl crate::execution::ExecutionCancellation for MarkerCancellation {
            fn cancelled(&self) -> bool {
                self.0.exists()
            }
        }

        let _env_lock = lock_env();
        let temp = tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        let worktree = temp.path().join("worktree");
        fs::create_dir(&remote).unwrap();
        fs::create_dir(&worktree).unwrap();
        checked_git(&remote, &["init", "--bare"]);
        checked_git(&worktree, &["init"]);
        checked_git(&worktree, &["config", "user.name", "Example User"]);
        checked_git(
            &worktree,
            &["config", "user.email", "example@example.invalid"],
        );
        fs::write(worktree.join("example.txt"), "before\n").unwrap();
        checked_git(&worktree, &["add", "example.txt"]);
        checked_git(&worktree, &["commit", "-m", "initial"]);
        checked_git(&worktree, &["branch", "-M", "repair/example"]);
        checked_git(
            &worktree,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        checked_git(&worktree, &["push", "-u", "origin", "repair/example"]);
        let base_head = checked_git(&worktree, &["rev-parse", "HEAD"]);
        fs::write(worktree.join("example.txt"), "after\n").unwrap();

        crate::test_env::TestRepoBuilder::new(temp.path())
            .required_commands(Vec::<String>::new())
            .write();
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let marker = temp.path().join("push-completed");
        let wrapper = temp.path().join("git-wrapper");
        fs::write(
            &wrapper,
            "#!/bin/sh\n\
             if [ \"$2\" = push ]; then\n\
               git \"$@\"\n\
               status=$?\n\
               if [ \"$status\" -eq 0 ]; then\n\
                 : > \"$JIG_TEST_CANCEL_MARKER\"\n\
                 sleep 60\n\
               fi\n\
               exit \"$status\"\n\
             fi\n\
             exec git \"$@\"\n",
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, wrapper.as_os_str());
        let _marker = EnvVarGuard::set("JIG_TEST_CANCEL_MARKER", marker.as_os_str());
        let mut observer = MarkerCancellation(marker);

        let push =
            commit_and_push(&ctx, &worktree, "repair/example", &base_head, &mut observer).unwrap();

        let final_head = checked_git(&worktree, &["rev-parse", "HEAD"]);
        let remote_head = checked_git(
            temp.path(),
            &[
                "--git-dir",
                remote.to_str().unwrap(),
                "rev-parse",
                "refs/heads/repair/example",
            ],
        );
        assert_eq!(remote_head, final_head);
        assert_eq!(push["pushed"], true);
        assert!(
            push["reconciliation"]
                .as_str()
                .unwrap()
                .contains("resolved to")
        );
    }

    #[test]
    fn remote_head_parser_requires_the_exact_requested_ref() {
        let stdout = b"abc123\trefs/heads/example\ndef456\trefs/heads/example-old\n";

        assert_eq!(
            remote_head_from_ls_remote(stdout, "refs/heads/example"),
            Some("abc123")
        );
        assert_eq!(
            remote_head_from_ls_remote(stdout, "refs/heads/missing"),
            None
        );
    }
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn git_checked<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(ctx, cwd, args, observer)?;
    if !output.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git command failed",
            output,
        )));
    }
    Ok(())
}

fn git_stdout<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(ctx, cwd, args, observer)?;
    if !output.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git command failed",
            output,
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_output<I, S>(
    ctx: &RepoContext,
    cwd: &Path,
    args: I,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let label = pr_git_label(&args);
    git_execution_output(cwd, args, ctx.command_timeout(), observer)
        .map_err(|error| pr_git_execution_error(&label, error))
}

fn git_execution_output<I, S>(
    cwd: &Path,
    args: I,
    timeout: CommandTimeout,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Output, ExecutionCommandError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let label = pr_git_label(&args);
    let mut command = git_command(cwd, &args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_authoritative_execution_command(&mut command, timeout, &label, observer)?;
    Ok(Output {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn pr_git_label(args: &[OsString]) -> String {
    let operation = args
        .first()
        .map(|arg| arg.to_string_lossy())
        .unwrap_or_else(|| "command".into());
    format!("PR manager git {operation}")
}

fn pr_git_execution_error(label: &str, error: ExecutionCommandError) -> PrRepairStepError {
    match error {
        ExecutionCommandError::CancelledBeforeStart => {
            PrRepairStepError::Cancelled(format!("{label} was cancelled before it started"))
        }
        ExecutionCommandError::Cancelled => {
            PrRepairStepError::Cancelled(format!("{label} was cancelled while it was running"))
        }
        ExecutionCommandError::Failed(error) => PrRepairStepError::Failed(error),
    }
}

fn git_command<I, S>(cwd: &Path, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(external_program(GIT_BIN_ENV, "git"));
    command
        .current_dir(cwd)
        .arg("--no-replace-objects")
        .args(args);
    // PR-manager commands target this known checkout but may fetch or push.
    // Strip repository redirection while retaining the transport/authentication
    // variables allowed by the shared known-repository policy.
    scrub_known_repository_git_environment(&mut command);
    command
}

fn git_error(label: &str, output: std::process::Output) -> anyhow::Error {
    anyhow!(
        "{} with status {}. stdout: {} stderr: {}",
        label,
        output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".into()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::{EnvVarGuard, lock_env};

    #[test]
    fn git_command_uses_configured_program_and_scrubs_repository_redirects() {
        let _env_lock = lock_env();
        let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, "custom-git");
        let _git_dir = EnvVarGuard::set("GIT_DIR", "/tmp/redirected.git");
        let _git_trace = EnvVarGuard::set("GIT_TRACE", "1");
        let _git_ssh = EnvVarGuard::set("GIT_SSH_COMMAND", "ssh -i test-key");

        let command = git_command(Path::new("/tmp/repository"), ["status", "--short"]);

        assert_eq!(command.get_program(), OsStr::new("custom-git"));
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("/tmp/repository"))
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--no-replace-objects", "status", "--short"]
                .map(OsStr::new)
                .as_slice()
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == OsStr::new("GIT_DIR") && value.is_none() })
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| { name == OsStr::new("GIT_TRACE") && value.is_none() })
        );
        assert!(
            !command
                .get_envs()
                .any(|(name, _)| { name == OsStr::new("GIT_SSH_COMMAND") })
        );
    }

    #[test]
    fn classify_uses_observed_default_branch_when_base_ref_is_missing() {
        let pull_request = json!({
            "number": 7,
            "title": "Fix widgets",
            "is_draft": false,
            "head": {
                "ref": "codex/widgets",
                "sha": "abc123",
                "is_cross_repository": false,
            },
            "stack": {
                "is_stacked": false,
            },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "REVIEW_REQUIRED",
            "checks": {
                "summary": {
                    "fail": 1,
                },
            },
            "review_threads": {
                "summary": {
                    "unresolved": 0,
                },
            },
        });

        match classify_pull_request(&pull_request, "trunk") {
            PrCandidate::Actionable(item) => assert_eq!(item.base_ref, "trunk"),
            PrCandidate::Skip(_) | PrCandidate::Idle(_) | PrCandidate::Pending(_) => {
                panic!("expected actionable PR candidate")
            }
        }
    }

    #[test]
    fn classify_pending_checks_are_not_observed_healthy() {
        let pull_request = json!({
            "number": 7,
            "title": "Fix widgets",
            "is_draft": false,
            "head": {
                "ref": "codex/widgets",
                "sha": "abc123",
                "is_cross_repository": false,
            },
            "stack": {
                "is_stacked": false,
            },
            "mergeability": {
                "mergeable": "MERGEABLE",
                "merge_state_status": "CLEAN",
            },
            "review_decision": "REVIEW_REQUIRED",
            "checks": {
                "summary": {
                    "fail": 0,
                    "pending": 1,
                },
            },
            "review_threads": {
                "summary": {
                    "unresolved": 0,
                },
            },
        });

        match classify_pull_request(&pull_request, "main") {
            PrCandidate::Pending(item) => {
                assert_eq!(item.item_key, "pr-7");
                assert_eq!(item.pending_checks, 1);
            }
            PrCandidate::Actionable(_) | PrCandidate::Skip(_) | PrCandidate::Idle(_) => {
                panic!("expected pending PR candidate")
            }
        }
    }
}
