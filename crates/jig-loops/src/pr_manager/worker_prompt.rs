//! The prompt a PR worker receives.

use jig_context::RepoContext;
use serde_json::{Value, json};

use super::PrWorkItem;
use super::review_thread_witness::actionable_review_threads;

pub(super) fn pr_worker_prompt(
    ctx: &RepoContext,
    item: &PrWorkItem,
    pull_request: &Value,
    merge: Option<&Value>,
) -> String {
    let worker_snapshot = worker_pull_request_snapshot(pull_request);
    format!(
        "You are Jig's PR manager worker for repository `{}`.\n\
         Work only on PR #{} on branch `{}`. Reasons: {}.\n\
         Resolve the reported PR issues in this isolated worktree. If merge conflicts are present, resolve them completely. \
         If CI is failing, inspect the failing checks and fix the underlying code. \
         If unresolved review threads are present, address the actionable feedback with code changes when possible. \
         Do not use `gh`, `curl`, or network access to reply to or resolve review threads. \
         Instead, return review-thread reply intents in the required structured output. \
         Include a reply intent only when a concise comment or resolution is needed after your code changes; set `resolve` only when the feedback is fully addressed.\n\
         Run relevant local tests when available. Do not merge the PR. Do not force-push. Keep changes minimal. \
         Do not stage or commit changes; Jig owns Git metadata and will stage, validate, and commit after you exit. \
         Always write structured output with `summary` and `review_thread_replies`.\n\n\
         Merge preparation result:\n{}\n\n\
         Normalized PR snapshot:\n{}\n",
        ctx.repo_name(),
        item.pr_number,
        item.head_ref,
        item.reasons.join(", "),
        merge
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".into()),
        serde_json::to_string_pretty(&worker_snapshot)
            .unwrap_or_else(|_| worker_snapshot.to_string()),
    )
}

pub(super) fn worker_pull_request_snapshot(pull_request: &Value) -> Value {
    let checks = pull_request
        .pointer("/checks/runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|check| {
            json!({
                "name": check.get("name").cloned().unwrap_or(Value::Null),
                "workflow": check.get("workflow").cloned().unwrap_or(Value::Null),
                "state": check.get("state").cloned().unwrap_or(Value::Null),
                "bucket": check.get("bucket").cloned().unwrap_or(Value::Null),
                "event": check.get("event").cloned().unwrap_or(Value::Null),
                "link": check.get("link").cloned().unwrap_or(Value::Null),
                "started_at": check.get("started_at").cloned().unwrap_or(Value::Null),
                "completed_at": check.get("completed_at").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let trusted_threads = actionable_review_threads(pull_request)
        .map(|thread| {
            let comments = thread
                .pointer("/comments/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|comment| {
                    comment
                        .pointer("/author/trusted")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .cloned()
                .collect::<Vec<_>>();
            json!({
                "id": thread.get("id").cloned().unwrap_or(Value::Null),
                "is_resolved": thread.get("is_resolved").cloned().unwrap_or(Value::Null),
                "is_outdated": thread.get("is_outdated").cloned().unwrap_or(Value::Null),
                "path": thread.get("path").cloned().unwrap_or(Value::Null),
                "line": thread.get("line").cloned().unwrap_or(Value::Null),
                "start_line": thread.get("start_line").cloned().unwrap_or(Value::Null),
                "subject_type": thread.get("subject_type").cloned().unwrap_or(Value::Null),
                "diff_side": thread.get("diff_side").cloned().unwrap_or(Value::Null),
                "viewer_can_reply": thread.get("viewer_can_reply").cloned().unwrap_or(Value::Null),
                "viewer_can_resolve": thread.get("viewer_can_resolve").cloned().unwrap_or(Value::Null),
                "comments": { "nodes": comments },
            })
        })
        .collect::<Vec<_>>();
    json!({
        "number": pull_request.get("number").cloned().unwrap_or(Value::Null),
        "state": pull_request.get("state").cloned().unwrap_or(Value::Null),
        "base": pull_request.get("base").cloned().unwrap_or(Value::Null),
        "head": pull_request.get("head").cloned().unwrap_or(Value::Null),
        "stack": pull_request.get("stack").cloned().unwrap_or(Value::Null),
        "mergeability": pull_request.get("mergeability").cloned().unwrap_or(Value::Null),
        "checks": {
            "summary": pull_request.pointer("/checks/summary").cloned().unwrap_or(Value::Null),
            "runs": checks,
        },
        "review_threads": {
            "summary": pull_request.pointer("/review_threads/summary").cloned().unwrap_or(Value::Null),
            "nodes": trusted_threads,
        },
    })
}
