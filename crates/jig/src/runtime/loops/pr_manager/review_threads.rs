struct ReviewThreadPostResult {
    posts: Value,
    failed: bool,
    cancelled: bool,
}

const REVIEW_THREAD_COMMENT_PAGE_LIMIT: usize = 100;
const REVIEW_THREAD_UPDATE_REQUEST_LIMIT: usize = 256;
const REVIEW_THREAD_UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MUTATION_RECONCILIATION_TIMEOUT: Duration = Duration::from_secs(30);

struct ReviewThreadUpdateBudget {
    started_at: Instant,
    timeout: Duration,
    request_count: usize,
}

impl ReviewThreadUpdateBudget {
    fn new(command_timeout: CommandTimeout) -> Self {
        Self {
            started_at: Instant::now(),
            timeout: command_timeout.duration().min(REVIEW_THREAD_UPDATE_TIMEOUT),
            request_count: 0,
        }
    }

    fn reserve_request(
        &mut self,
        requested_timeout: CommandTimeout,
    ) -> std::result::Result<CommandTimeout, ExecutionCommandError> {
        if self.request_count >= REVIEW_THREAD_UPDATE_REQUEST_LIMIT {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates exceeded their {REVIEW_THREAD_UPDATE_REQUEST_LIMIT}-request budget"
            )));
        }
        let remaining = self
            .timeout
            .checked_sub(self.started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                ExecutionCommandError::failed(anyhow!(
                    "GitHub review thread updates exceeded their {}-second deadline",
                    self.timeout.as_secs()
                ))
            })?;
        let timeout = remaining.min(requested_timeout.duration());
        let seconds = timeout.as_secs();
        if seconds == 0 {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates exceeded their {}-second deadline",
                self.timeout.as_secs()
            )));
        }
        let timeout = CommandTimeout::from_seconds(seconds).ok_or_else(|| {
            ExecutionCommandError::failed(anyhow!(
                "GitHub review thread updates produced an invalid request timeout"
            ))
        })?;
        self.request_count += 1;
        Ok(timeout)
    }
}

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
    let thread_witnesses = observed_review_thread_witnesses(pull_request);
    let mut posts = Vec::new();
    let mut handled_thread_ids = BTreeSet::new();
    let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout());
    let mut failed = false;
    let mut cancelled = false;
    for (index, reply) in replies.iter().enumerate() {
        if observer.cancelled() {
            cancelled = true;
            posts.extend(replies[index..].iter().map(cancelled_review_thread_post));
            break;
        }
        let thread_id = review_thread_id(reply);
        let body = reply
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let resolve = reply
            .get("resolve")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if thread_id.is_empty() {
            posts.push(skipped_review_thread_post(
                thread_id,
                "missing_review_thread",
                "worker returned a review thread update without a thread ID",
            ));
            continue;
        }
        if !handled_thread_ids.insert(thread_id) {
            posts.push(skipped_review_thread_post(
                thread_id,
                "duplicate_review_thread",
                "worker returned more than one update intent for the same review thread",
            ));
            continue;
        };

        let Some(thread_witness) = thread_witnesses.get(thread_id) else {
            posts.push(skipped_review_thread_post(
                thread_id,
                "unknown_review_thread",
                "worker requested a review thread that was not present in the observed PR snapshot",
            ));
            continue;
        };

        let mut thread_failed = false;
        let mut reply_error = Value::Null;
        let mut reply_skipped = false;
        let mut reply_skip_reason = Value::Null;
        let reply_response = if body.is_empty() {
            None
        } else {
            match post_review_thread_reply(
                ctx,
                thread_id,
                body,
                repair_version,
                thread_witness,
                observer,
                &mut budget,
            ) {
                Ok(ReviewThreadReply::Posted(response)) => Some(response),
                Ok(ReviewThreadReply::Changed) => {
                    reply_skipped = true;
                    reply_skip_reason = Value::String("review_thread_changed".into());
                    None
                }
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
        } else if resolve && reply_skipped {
            resolve_skipped = true;
            resolve_skip_reason = Value::String("review_thread_changed".into());
            None
        } else if resolve && thread_failed && !body.is_empty() {
            resolve_skipped = true;
            resolve_skip_reason = Value::String("reply_failed".into());
            None
        } else if resolve {
            let reply_comment_id = reply_response
                .as_ref()
                .and_then(|value| value.pointer("/data/addPullRequestReviewThreadReply/comment/id"))
                .and_then(Value::as_str);
            match resolve_review_thread(
                ctx,
                thread_id,
                thread_witness,
                reply_comment_id,
                observer,
                &mut budget,
            ) {
                Ok(ReviewThreadResolution::Resolved(response)) => Some(response),
                Ok(ReviewThreadResolution::Changed) => {
                    resolve_skipped = true;
                    resolve_skip_reason = Value::String("review_thread_changed".into());
                    None
                }
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
        let (status, reason) = review_thread_post_status(
            cancelled,
            thread_failed,
            reply_skipped,
            &reply_skip_reason,
            resolve_skipped,
            &resolve_skip_reason,
        );
        posts.push(json!({
            "thread_id": thread_id,
            "status": status,
            "reason": reason,
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
            "reply_skipped": reply_skipped,
            "reply_skip_reason": reply_skip_reason,
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
            posts.extend(
                replies[index + 1..]
                    .iter()
                    .map(cancelled_review_thread_post),
            );
            break;
        }
    }
    ReviewThreadPostResult {
        posts: json!(posts),
        failed,
        cancelled,
    }
}

fn review_thread_post_status(
    cancelled: bool,
    failed: bool,
    reply_skipped: bool,
    reply_skip_reason: &Value,
    resolve_skipped: bool,
    resolve_skip_reason: &Value,
) -> (&'static str, Value) {
    if cancelled {
        ("cancelled", Value::Null)
    } else if failed {
        ("failed", Value::Null)
    } else if reply_skipped {
        ("skipped", reply_skip_reason.clone())
    } else if resolve_skipped {
        ("skipped", resolve_skip_reason.clone())
    } else {
        ("posted", Value::Null)
    }
}

fn skipped_review_thread_post(thread_id: &str, reason: &str, detail: &str) -> Value {
    json!({
        "thread_id": thread_id,
        "status": "skipped",
        "reason": reason,
        "detail": detail,
        "replied": false,
        "reply_comment_id": Value::Null,
        "reply_url": Value::Null,
        "reply_reconciled": false,
        "reply_error": Value::Null,
        "reply_skipped": false,
        "reply_skip_reason": Value::Null,
        "resolved": false,
        "is_resolved": Value::Null,
        "resolve_reconciled": false,
        "resolve_error": Value::Null,
        "resolve_skipped": false,
        "resolve_skip_reason": Value::Null,
    })
}

fn cancelled_review_thread_post(reply: &Value) -> Value {
    skipped_review_thread_post(
        review_thread_id(reply),
        "cancelled",
        "review thread update was not attempted because execution was cancelled",
    )
}

fn review_thread_id(reply: &Value) -> &str {
    reply
        .get("thread_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
}

fn resolve_review_thread(
    ctx: &RepoContext,
    thread_id: &str,
    witness: &ReviewThreadWitness,
    reply_comment_id: Option<&str>,
    observer: &mut dyn ExecutionControl,
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<ReviewThreadResolution, ExecutionCommandError> {
    let state = review_thread_resolution_state(ctx, thread_id, observer, budget)?;
    if state.is_resolved {
        return Ok(ReviewThreadResolution::Resolved(
            reconciled_resolve_response(thread_id),
        ));
    }
    if !review_thread_matches_witness(&state, witness, reply_comment_id) {
        return Ok(ReviewThreadResolution::Changed);
    }
    let timeout = budget.reserve_request(ctx.command_timeout())?;
    let result = github::gh_json_with_timeout(
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
        timeout,
        observer,
    )
    .and_then(validate_resolve_mutation_response);
    reconcile_resolve_mutation(ctx, thread_id, result, budget).map(ReviewThreadResolution::Resolved)
}

fn review_thread_reply_comment(
    ctx: &RepoContext,
    thread_id: &str,
    marker: &str,
    observer: &mut dyn ExecutionControl,
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<Option<Value>, ExecutionCommandError> {
    let total_timeout = ctx.command_timeout().duration();
    let deadline = Instant::now() + total_timeout;
    fetch_review_thread_reply_comment(thread_id, marker, |cursor| {
        let timeout = remaining_operation_timeout(
            deadline,
            total_timeout,
            "GitHub review thread reply lookup",
        )?;
        let timeout = budget.reserve_request(timeout)?;
        github::gh_json_with_timeout(
            ctx,
            review_thread_reply_state_args(thread_id, cursor),
            &[0],
            timeout,
            observer,
        )
    })
}

fn review_thread_reply_comment_for_reconciliation(
    ctx: &RepoContext,
    thread_id: &str,
    marker: &str,
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<Option<Value>, ExecutionCommandError> {
    let deadline = Instant::now() + MUTATION_RECONCILIATION_TIMEOUT;
    fetch_review_thread_reply_comment(thread_id, marker, |cursor| {
        let mut observer = NoopExecutionObserver;
        let timeout = budget.reserve_request(remaining_reconciliation_timeout(deadline)?)?;
        github::gh_json_with_timeout(
            ctx,
            review_thread_reply_state_args(thread_id, cursor),
            &[0],
            timeout,
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
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<LiveReviewThreadState, ExecutionCommandError> {
    let total_timeout = ctx.command_timeout().duration();
    let deadline = Instant::now() + total_timeout;
    fetch_review_thread_witness_state(thread_id, |cursor| {
        let timeout = remaining_operation_timeout(
            deadline,
            total_timeout,
            "GitHub review thread witness lookup",
        )?;
        let timeout = budget.reserve_request(timeout)?;
        github::gh_json_with_timeout(
            ctx,
            review_thread_witness_state_args(thread_id, cursor),
            &[0],
            timeout,
            observer,
        )
    })
}

fn review_thread_resolution_state_for_reconciliation(
    ctx: &RepoContext,
    thread_id: &str,
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<Value, ExecutionCommandError> {
    let deadline = Instant::now() + MUTATION_RECONCILIATION_TIMEOUT;
    let mut observer = NoopExecutionObserver;
    let timeout = budget.reserve_request(remaining_reconciliation_timeout(deadline)?)?;
    github::gh_json_with_timeout(
        ctx,
        review_thread_resolution_state_args(thread_id),
        &[0],
        timeout,
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

fn review_thread_witness_state_args(thread_id: &str, cursor: Option<&str>) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("api"),
        OsString::from("graphql"),
        OsString::from("-f"),
        OsString::from(format!("query={}", review_thread_witness_state_query())),
        OsString::from("-f"),
        OsString::from(format!("threadId={thread_id}")),
    ];
    if let Some(cursor) = cursor {
        args.push(OsString::from("-f"));
        args.push(OsString::from(format!("commentsBefore={cursor}")));
    }
    args
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
    let comment_count = value
        .pointer("/data/node/comments/totalCount")
        .and_then(Value::as_u64);
    let comments = value
        .pointer("/data/node/comments/nodes")
        .and_then(Value::as_array);
    if observed_id != Some(thread_id)
        || is_resolved.is_none()
        || comment_count.is_none()
        || comments.is_none()
    {
        return Err(ExecutionCommandError::failed(anyhow!(
            "GitHub review thread resolution query returned an invalid payload for {thread_id}"
        )));
    }
    Ok(value)
}

fn validate_review_thread_witness_page(
    value: Value,
    thread_id: &str,
) -> std::result::Result<Value, ExecutionCommandError> {
    let valid = value.pointer("/data/node/id").and_then(Value::as_str) == Some(thread_id)
        && value
            .pointer("/data/node/isResolved")
            .and_then(Value::as_bool)
            .is_some()
        && value
            .pointer("/data/node/comments/totalCount")
            .and_then(Value::as_u64)
            .is_some()
        && value
            .pointer("/data/node/comments/pageInfo/hasPreviousPage")
            .and_then(Value::as_bool)
            .is_some()
        && value
            .pointer("/data/node/comments/nodes")
            .and_then(Value::as_array)
            .is_some();
    if !valid {
        return Err(ExecutionCommandError::failed(anyhow!(
            "GitHub review thread witness query returned an invalid payload for {thread_id}"
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
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<Value, ExecutionCommandError> {
    let error = match result {
        Ok(value) => return Ok(value),
        Err(error @ ExecutionCommandError::CancelledBeforeStart) => return Err(error),
        Err(error) => error,
    };
    if let Ok(Some(comment)) = review_thread_reply_comment_for_reconciliation(
        ctx,
        thread_id,
        marker,
        budget,
    )
    {
        return Ok(reconciled_reply_response(&comment));
    }
    Err(error)
}

fn reconcile_resolve_mutation(
    ctx: &RepoContext,
    thread_id: &str,
    result: std::result::Result<Value, ExecutionCommandError>,
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<Value, ExecutionCommandError> {
    let error = match result {
        Ok(value) => return Ok(value),
        Err(error @ ExecutionCommandError::CancelledBeforeStart) => return Err(error),
        Err(error) => error,
    };
    if let Ok(state) =
        review_thread_resolution_state_for_reconciliation(ctx, thread_id, budget)
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
