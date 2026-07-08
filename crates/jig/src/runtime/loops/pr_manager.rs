use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::runtime::worker_runner::{
    CodexExecMode, CodexExecRequest, CodexPrompt, WorkerReceiptRequest, run_codex_exec,
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
) -> Result<WorkflowTick> {
    let observed = github::github_pr_status_snapshot(ctx)?;
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

    let action_result = run_pr_repair(ctx, workflow, item, pull_request, &lease);
    let _ = lease_store.release(&branch_lease_key, &lease.owner);
    match action_result {
        Ok(action) => {
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
                "attempt": attempt,
                "error": format!("{error:#}"),
            }))
        }
    }
}

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
) -> Result<Value> {
    let worktree = prepare_worktree(ctx, workflow, item)?;
    let base_head = git_stdout(&worktree, ["rev-parse", "HEAD"])?;
    let merge = if item.reasons.iter().any(|reason| reason == "merge_conflict") {
        Some(start_base_merge(&worktree, &item.base_ref)?)
    } else {
        None
    };
    let prompt = pr_worker_prompt(ctx, item, pull_request, merge.as_ref());
    let output_schema = pr_worker_output_schema();
    let worker = run_codex_exec(
        ctx,
        CodexExecRequest {
            root: &worktree,
            mode: CodexExecMode::Exec,
            model: None,
            approval_policy: Some("never"),
            sandbox: Some("workspace-write"),
            ephemeral: true,
            extra_args: Vec::new(),
            output_schema: Some(&output_schema),
            prompt: CodexPrompt::Stdin(&prompt),
            receipt: WorkerReceiptRequest {
                purpose: "pr_manager",
                plan_id: None,
                workflow_id: Some(&workflow.id),
                item_key: Some(&item.item_key),
                collect_git_metadata: false,
                collect_worktree_fingerprint: false,
            },
        },
    )?;
    if !worker.output.status.success() {
        bail!(
            "PR manager worker exited with status {}",
            worker.output.status.code().unwrap_or(1)
        );
    }

    let worker_output = parse_pr_worker_output(&worker.output.stdout)?;
    let push = commit_and_push(&worktree, &item.head_ref, &base_head)?;
    let review_thread_posts = post_review_thread_updates(ctx, pull_request, &worker_output)?;
    let status = if review_thread_posts.failed {
        "failed"
    } else {
        "attempted"
    };
    Ok(json!({
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
        "merge": merge,
        "worker_output": worker_output,
        "worker_receipt_id": worker.worker_receipt_id,
        "push": push,
        "review_thread_posts": review_thread_posts.posts,
        "error": if review_thread_posts.failed {
            Value::String("one or more review thread update intents failed".into())
        } else {
            Value::Null
        },
    }))
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
}

fn post_review_thread_updates(
    ctx: &RepoContext,
    pull_request: &Value,
    worker_output: &Value,
) -> Result<ReviewThreadPostResult> {
    let empty = Vec::new();
    let replies = worker_output
        .get("review_thread_replies")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let allowed_thread_ids = observed_review_thread_ids(pull_request);
    let mut posts = Vec::new();
    let mut failed = false;
    for reply in replies {
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
            match post_review_thread_reply(ctx, thread_id, body) {
                Ok(response) => Some(response),
                Err(error) => {
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
        let resolve_response = if resolve && thread_failed && !body.is_empty() {
            resolve_skipped = true;
            resolve_skip_reason = Value::String("reply_failed".into());
            None
        } else if resolve {
            match resolve_review_thread(ctx, thread_id) {
                Ok(response) => Some(response),
                Err(error) => {
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
            "status": if thread_failed { "failed" } else { "posted" },
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
    }
    Ok(ReviewThreadPostResult {
        posts: json!(posts),
        failed,
    })
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

fn post_review_thread_reply(ctx: &RepoContext, thread_id: &str, body: &str) -> Result<Value> {
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
    )
}

fn resolve_review_thread(ctx: &RepoContext, thread_id: &str) -> Result<Value> {
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
    )
}

fn add_review_thread_reply_mutation() -> &'static str {
    r#"
mutation($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $threadId, body: $body}) {
    comment {
      id
      url
    }
  }
}
"#
}

fn resolve_review_thread_mutation() -> &'static str {
    r#"
mutation($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) {
    thread {
      id
      isResolved
    }
  }
}
"#
}

fn prepare_worktree(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
) -> Result<PathBuf> {
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

    git_checked(ctx.root(), ["fetch", "origin", &item.head_ref])?;
    if worktree.join(".git").exists() {
        clean_reused_worktree(&worktree)?;
        git_checked(&worktree, ["fetch", "origin", &item.head_ref])?;
        git_checked(&worktree, ["checkout", "--detach", "FETCH_HEAD"])?;
    } else {
        git_checked(
            ctx.root(),
            vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--detach"),
                worktree.as_os_str().to_os_string(),
                OsString::from("FETCH_HEAD"),
            ],
        )?;
    }

    git_checked(&worktree, ["config", "user.name", "Jig PR Manager"])?;
    git_checked(
        &worktree,
        [
            "config",
            "user.email",
            "jig-pr-manager@users.noreply.github.com",
        ],
    )?;
    Ok(worktree)
}

fn clean_reused_worktree(worktree: &Path) -> Result<()> {
    let _ = git_output(worktree, ["merge", "--abort"]);
    git_checked(worktree, ["reset", "--hard"])?;
    git_checked(worktree, ["clean", "-fd"])?;
    Ok(())
}

fn start_base_merge(worktree: &Path, base_ref: &str) -> Result<Value> {
    let fetch = git_output(worktree, ["fetch", "origin", base_ref])?;
    if !fetch.status.success() {
        return Err(git_error("git fetch base branch failed", fetch));
    }
    let merge = git_output(worktree, ["merge", "--no-edit", "FETCH_HEAD"])?;
    Ok(json!({
        "exit_status": merge.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&merge.stdout),
        "stderr": String::from_utf8_lossy(&merge.stderr),
        "conflicts": !merge.status.success(),
    }))
}

fn commit_and_push(worktree: &Path, head_ref: &str, base_head: &str) -> Result<Value> {
    let dirty_before_commit = git_stdout(worktree, ["status", "--porcelain"])?;
    if !dirty_before_commit.trim().is_empty() {
        git_checked(worktree, ["add", "-A"])?;
        git_checked(
            worktree,
            [
                "commit",
                "-m",
                &format!("chore: update PR via Jig PR manager ({head_ref})"),
            ],
        )?;
    }
    let final_head = git_stdout(worktree, ["rev-parse", "HEAD"])?;
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
    let push = git_output(worktree, ["push", "origin", &push_ref])?;
    if !push.status.success() {
        return Err(git_error("git push failed without force", push));
    }

    Ok(json!({
        "status": "pushed",
        "pushed": true,
        "base_head": base_head.trim(),
        "final_head": final_head.trim(),
        "force": false,
    }))
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

fn git_checked<I, S>(cwd: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        return Err(git_error("git command failed", output));
    }
    Ok(())
}

fn git_stdout<I, S>(cwd: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        return Err(git_error("git command failed", output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_output<I, S>(cwd: &Path, args: I) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let git = std::env::var_os("JIG_GIT_BIN").unwrap_or_else(|| OsString::from("git"));
    Command::new(&git)
        .current_dir(cwd)
        .args(args)
        .output()
        .with_context(|| format!("Failed to start git in {}", cwd.display()))
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
