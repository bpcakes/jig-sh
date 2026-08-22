use std::collections::BTreeSet;
use std::ffi::OsString;

use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;

use super::super::github;

pub(super) struct ReviewThreadPostResult {
    pub(super) posts: Value,
    pub(super) failed: bool,
}

pub(super) fn post_review_thread_updates(
    ctx: &RepoContext,
    pull_request: &Value,
    worker_output: &Value,
) -> ReviewThreadPostResult {
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
    ReviewThreadPostResult {
        posts: json!(posts),
        failed,
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
