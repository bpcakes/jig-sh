struct ReviewThreadPostResult {
    posts: Value,
    failed: bool,
    cancelled: bool,
}

const REVIEW_THREAD_COMMENT_PAGE_LIMIT: usize = 100;
const MUTATION_RECONCILIATION_TIMEOUT: Duration = Duration::from_secs(30);
fn post_review_thread_updates(
    ctx: &RepoContext,
    pull_request: &Value,
    worker_output: &Value,
    repair_version: &str,
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
            match post_review_thread_reply(ctx, thread_id, body, repair_version, observer) {
                Ok(response) => Some(response),
                Err(
                    ExecutionCommandError::CancelledBeforeStart | ExecutionCommandError::Cancelled,
                ) => {
                    cancelled = true;
                    thread_failed = true;
                    reply_error = Value::String("review thread reply was cancelled".into());
                    None
                }
                Err(ExecutionCommandError::Failed { error, .. }) => {
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
                Err(ExecutionCommandError::Failed { error, .. }) => {
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
            "reply_reconciled": reply_response
                .as_ref()
                .and_then(|value| value.pointer("/_jig/reconciled"))
                .cloned()
                .unwrap_or(Value::Bool(false)),
            "reply_error": reply_error,
            "resolved": resolve_response.is_some(),
            "is_resolved": resolve_response
                .as_ref()
                .and_then(|value| value.pointer("/data/resolveReviewThread/thread/isResolved"))
                .cloned()
                .unwrap_or(Value::Null),
            "resolve_reconciled": resolve_response
                .as_ref()
                .and_then(|value| value.pointer("/_jig/reconciled"))
                .cloned()
                .unwrap_or(Value::Bool(false)),
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
        .filter(|thread| {
            thread
                .get("has_trusted_comment")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|thread| thread.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn post_review_thread_reply(
    ctx: &RepoContext,
    thread_id: &str,
    body: &str,
    repair_version: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    let marker = review_thread_reply_marker(thread_id, repair_version);
    if let Some(comment) = review_thread_reply_comment(ctx, thread_id, &marker, observer)? {
        return Ok(reconciled_reply_response(&comment));
    }
    let body = format!("{body}\n\n{marker}");
    let mut body_file = tempfile::NamedTempFile::new()
        .context("Failed to create a temporary GitHub review reply file")
        .map_err(ExecutionCommandError::failed)?;
    use std::io::Write as _;
    body_file
        .write_all(body.as_bytes())
        .context("Failed to write the temporary GitHub review reply file")
        .map_err(ExecutionCommandError::failed)?;
    body_file
        .flush()
        .context("Failed to flush the temporary GitHub review reply file")
        .map_err(ExecutionCommandError::failed)?;
    let mut body_field = OsString::from("body=@");
    body_field.push(body_file.path());
    let result = github::gh_json(
        ctx,
        vec![
            OsString::from("api"),
            OsString::from("graphql"),
            OsString::from("-f"),
            OsString::from(format!("query={}", add_review_thread_reply_mutation())),
            OsString::from("-f"),
            OsString::from(format!("threadId={thread_id}")),
            OsString::from("-F"),
            body_field,
        ],
        &[0],
        observer,
    )
    .and_then(validate_reply_mutation_response);
    reconcile_reply_mutation(ctx, thread_id, &marker, result)
}

fn resolve_review_thread(
    ctx: &RepoContext,
    thread_id: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    let state = review_thread_resolution_state(ctx, thread_id, observer)?;
    if state
        .pointer("/data/node/isResolved")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(reconciled_resolve_response(thread_id));
    }
    let result = github::gh_json(
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
    .and_then(validate_resolve_mutation_response);
    reconcile_resolve_mutation(ctx, thread_id, result)
}

fn review_thread_reply_marker(thread_id: &str, repair_version: &str) -> String {
    format!("<!-- jig-pr-manager:review-reply:{thread_id}:{repair_version} -->")
}

fn review_thread_reply_comment(
    ctx: &RepoContext,
    thread_id: &str,
    marker: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Option<Value>, ExecutionCommandError> {
    let total_timeout = ctx.command_timeout().duration();
    let deadline = Instant::now() + total_timeout;
    fetch_review_thread_reply_comment(thread_id, marker, |cursor| {
        github::gh_json_with_timeout(
            ctx,
            review_thread_reply_state_args(thread_id, cursor),
            &[0],
            remaining_operation_timeout(
                deadline,
                total_timeout,
                "GitHub review thread reply lookup",
            )?,
            observer,
        )
    })
}

fn review_thread_reply_comment_for_reconciliation(
    ctx: &RepoContext,
    thread_id: &str,
    marker: &str,
) -> std::result::Result<Option<Value>, ExecutionCommandError> {
    let deadline = Instant::now() + MUTATION_RECONCILIATION_TIMEOUT;
    fetch_review_thread_reply_comment(thread_id, marker, |cursor| {
        let mut observer = NoopExecutionObserver;
        github::gh_json_with_timeout(
            ctx,
            review_thread_reply_state_args(thread_id, cursor),
            &[0],
            remaining_reconciliation_timeout(deadline)?,
            &mut observer,
        )
    })
}

fn review_thread_reply_state_args(thread_id: &str, cursor: Option<&str>) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("api"),
        OsString::from("graphql"),
        OsString::from("-f"),
        OsString::from(format!("query={}", review_thread_reply_state_query())),
        OsString::from("-f"),
        OsString::from(format!("threadId={thread_id}")),
    ];
    if let Some(cursor) = cursor {
        args.push(OsString::from("-f"));
        args.push(OsString::from(format!("commentsBefore={cursor}")));
    }
    args
}

fn review_thread_resolution_state(
    ctx: &RepoContext,
    thread_id: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<Value, ExecutionCommandError> {
    github::gh_json(
        ctx,
        review_thread_resolution_state_args(thread_id),
        &[0],
        observer,
    )
    .and_then(|value| validate_review_thread_resolution_state(value, thread_id))
}

fn review_thread_resolution_state_for_reconciliation(
    ctx: &RepoContext,
    thread_id: &str,
) -> std::result::Result<Value, ExecutionCommandError> {
    let deadline = Instant::now() + MUTATION_RECONCILIATION_TIMEOUT;
    let mut observer = NoopExecutionObserver;
    github::gh_json_with_timeout(
        ctx,
        review_thread_resolution_state_args(thread_id),
        &[0],
        remaining_reconciliation_timeout(deadline)?,
        &mut observer,
    )
    .and_then(|value| validate_review_thread_resolution_state(value, thread_id))
}

fn remaining_reconciliation_timeout(
    deadline: Instant,
) -> std::result::Result<CommandTimeout, ExecutionCommandError> {
    remaining_operation_timeout(
        deadline,
        MUTATION_RECONCILIATION_TIMEOUT,
        "GitHub mutation reconciliation",
    )
}

fn remaining_operation_timeout(
    deadline: Instant,
    total_timeout: Duration,
    operation: &str,
) -> std::result::Result<CommandTimeout, ExecutionCommandError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ExecutionCommandError::failed(anyhow!(
            "{operation} exceeded its {} second total timeout",
            total_timeout.as_secs()
        )));
    }
    let seconds = remaining
        .as_secs()
        .saturating_add(u64::from(remaining.subsec_nanos() != 0));
    CommandTimeout::from_seconds(seconds).ok_or_else(|| {
        ExecutionCommandError::failed(anyhow!(
            "{operation} has an invalid remaining timeout"
        ))
    })
}

fn review_thread_resolution_state_args(thread_id: &str) -> Vec<OsString> {
    vec![
        OsString::from("api"),
        OsString::from("graphql"),
        OsString::from("-f"),
        OsString::from(format!(
            "query={}",
            review_thread_resolution_state_query()
        )),
        OsString::from("-f"),
        OsString::from(format!("threadId={thread_id}")),
    ]
}

fn fetch_review_thread_reply_comment(
    thread_id: &str,
    marker: &str,
    mut fetch: impl FnMut(Option<&str>) -> std::result::Result<Value, ExecutionCommandError>,
) -> std::result::Result<Option<Value>, ExecutionCommandError> {
    let mut cursor = None;
    for _ in 0..REVIEW_THREAD_COMMENT_PAGE_LIMIT {
        let state = validate_review_thread_reply_state(fetch(cursor.as_deref())?, thread_id)?;
        if let Some(comment) = review_thread_comment_with_marker(&state, marker) {
            return Ok(Some(comment.clone()));
        }
        if !review_thread_comments_have_previous_page(&state) {
            return Ok(None);
        }
        cursor = Some(
            state
                .pointer("/data/node/comments/pageInfo/startCursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty())
                .ok_or_else(|| {
                    ExecutionCommandError::failed(anyhow!(
                        "GitHub review thread comment page reported earlier results without a start cursor for {thread_id}"
                    ))
                })?
                .to_string(),
        );
    }
    Err(ExecutionCommandError::failed(anyhow!(
        "GitHub review thread comment history exceeded the {REVIEW_THREAD_COMMENT_PAGE_LIMIT}-page safety limit for {thread_id}"
    )))
}

fn review_thread_comments_have_previous_page(state: &Value) -> bool {
    state
        .pointer("/data/node/comments/pageInfo/hasPreviousPage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn validate_review_thread_reply_state(
    value: Value,
    thread_id: &str,
) -> std::result::Result<Value, ExecutionCommandError> {
    let observed_id = value.pointer("/data/node/id").and_then(Value::as_str);
    let comments = value
        .pointer("/data/node/comments/nodes")
        .and_then(Value::as_array);
    let has_previous_page = value
        .pointer("/data/node/comments/pageInfo/hasPreviousPage")
        .and_then(Value::as_bool);
    if observed_id != Some(thread_id)
        || comments.is_none()
        || has_previous_page.is_none()
    {
        return Err(ExecutionCommandError::failed(anyhow!(
            "GitHub review thread state query returned an invalid payload for {thread_id}"
        )));
    }
    Ok(value)
}

fn validate_review_thread_resolution_state(
    value: Value,
    thread_id: &str,
) -> std::result::Result<Value, ExecutionCommandError> {
    let observed_id = value.pointer("/data/node/id").and_then(Value::as_str);
    let is_resolved = value
        .pointer("/data/node/isResolved")
        .and_then(Value::as_bool);
    if observed_id != Some(thread_id) || is_resolved.is_none() {
        return Err(ExecutionCommandError::failed(anyhow!(
            "GitHub review thread resolution query returned an invalid payload for {thread_id}"
        )));
    }
    Ok(value)
}

fn validate_reply_mutation_response(
    value: Value,
) -> std::result::Result<Value, ExecutionCommandError> {
    let id = value
        .pointer("/data/addPullRequestReviewThreadReply/comment/id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let url = value
        .pointer("/data/addPullRequestReviewThreadReply/comment/url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty());
    if id.is_none() || url.is_none() {
        return Err(ExecutionCommandError::failed(anyhow!(
            "GitHub review thread reply mutation returned an invalid payload"
        )));
    }
    Ok(value)
}

fn validate_resolve_mutation_response(
    value: Value,
) -> std::result::Result<Value, ExecutionCommandError> {
    if value
        .pointer("/data/resolveReviewThread/thread/isResolved")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err(ExecutionCommandError::failed(anyhow!(
            "GitHub review thread resolve mutation did not report a resolved thread"
        )));
    }
    Ok(value)
}

fn reconcile_reply_mutation(
    ctx: &RepoContext,
    thread_id: &str,
    marker: &str,
    result: std::result::Result<Value, ExecutionCommandError>,
) -> std::result::Result<Value, ExecutionCommandError> {
    let error = match result {
        Ok(value) => return Ok(value),
        Err(error @ ExecutionCommandError::CancelledBeforeStart) => return Err(error),
        Err(error) => error,
    };
    if let Ok(Some(comment)) =
        review_thread_reply_comment_for_reconciliation(ctx, thread_id, marker)
    {
        return Ok(reconciled_reply_response(&comment));
    }
    Err(error)
}

fn reconcile_resolve_mutation(
    ctx: &RepoContext,
    thread_id: &str,
    result: std::result::Result<Value, ExecutionCommandError>,
) -> std::result::Result<Value, ExecutionCommandError> {
    let error = match result {
        Ok(value) => return Ok(value),
        Err(error @ ExecutionCommandError::CancelledBeforeStart) => return Err(error),
        Err(error) => error,
    };
    if let Ok(state) = review_thread_resolution_state_for_reconciliation(ctx, thread_id)
        && state
            .pointer("/data/node/isResolved")
            .and_then(Value::as_bool)
            == Some(true)
    {
        return Ok(reconciled_resolve_response(thread_id));
    }
    Err(error)
}

fn review_thread_comment_with_marker<'a>(state: &'a Value, marker: &str) -> Option<&'a Value> {
    state
        .pointer("/data/node/comments/nodes")?
        .as_array()?
        .iter()
        .find(|comment| {
            let has_marker = comment
                .get("body")
                .and_then(Value::as_str)
                .is_some_and(|body| body.contains(marker));
            let has_id = comment
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty());
            let has_url = comment
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|url| !url.is_empty());
            let owned_by_viewer = comment["viewerDidAuthor"].as_bool() == Some(true);
            has_marker && has_id && has_url && owned_by_viewer
        })
}

fn reconciled_reply_response(comment: &Value) -> Value {
    json!({
        "data": {
            "addPullRequestReviewThreadReply": {
                "comment": {
                    "id": comment.get("id").cloned().unwrap_or(Value::Null),
                    "url": comment.get("url").cloned().unwrap_or(Value::Null),
                }
            }
        },
        "_jig": {"reconciled": true},
    })
}

fn reconciled_resolve_response(thread_id: &str) -> Value {
    json!({
        "data": {
            "resolveReviewThread": {
                "thread": {"id": thread_id, "isResolved": true}
            }
        },
        "_jig": {"reconciled": true},
    })
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

const fn review_thread_reply_state_query() -> &'static str {
    r"
query ReviewThreadState($threadId: ID!, $commentsBefore: String) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      id
      comments(last: 100, before: $commentsBefore) {
        pageInfo {
          hasPreviousPage
          startCursor
        }
        nodes {
          id
          url
          body
          viewerDidAuthor
        }
      }
    }
  }
}
"
}

const fn review_thread_resolution_state_query() -> &'static str {
    r"
query ReviewThreadState($threadId: ID!) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      id
      isResolved
    }
  }
}
"
}
