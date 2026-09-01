#[derive(Clone)]
struct ReviewThreadWitness {
    comment_count: u64,
    comment_ids: BTreeSet<String>,
    resolution_generation: String,
    reply_generation: String,
}

impl Default for ReviewThreadWitness {
    fn default() -> Self {
        let empty_generation = review_comment_generation(std::iter::empty(), None);
        Self {
            comment_count: 0,
            comment_ids: BTreeSet::new(),
            resolution_generation: empty_generation.clone(),
            reply_generation: empty_generation,
        }
    }
}

struct LiveReviewThreadState {
    is_resolved: bool,
    total_count: u64,
    comments: Vec<Value>,
}

enum ReviewThreadResolution {
    Resolved(Value),
    Changed,
}

fn observed_review_thread_witnesses(
    pull_request: &Value,
) -> BTreeMap<String, ReviewThreadWitness> {
    actionable_review_threads(pull_request)
        .filter_map(|thread| {
            let id = thread.get("id").and_then(Value::as_str)?.to_string();
            let comment_count = thread.pointer("/comments/total_count")?.as_u64()?;
            let comments = thread
                .pointer("/comments/nodes")
                .and_then(Value::as_array)?;
            let comment_ids = comments
                .iter()
                .filter_map(|comment| comment.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect();
            let resolution_generation = review_comment_generation(comments.iter(), None);
            let reply_generation = review_reply_generation(thread);
            Some((
                id,
                ReviewThreadWitness {
                    comment_count,
                    comment_ids,
                    resolution_generation,
                    reply_generation,
                },
            ))
        })
        .collect()
}

fn review_reply_generation(thread: &Value) -> String {
    review_comment_generation(
        thread
        .pointer("/comments/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|comment| {
            comment
                .pointer("/author/trusted")
                .and_then(Value::as_bool)
                == Some(true)
                && !comment
                    .get("body")
                    .and_then(Value::as_str)
                    .is_some_and(|body| body.contains("<!-- jig-pr-manager:review-reply:"))
        }),
        None,
    )
}

fn review_comment_generation<'a>(
    comments: impl IntoIterator<Item = &'a Value>,
    excluded_comment_id: Option<&str>,
) -> String {
    let mut digest = Sha256::new();
    for comment in comments.into_iter().filter(|comment| {
        excluded_comment_id
            != comment.get("id").and_then(Value::as_str)
    }) {
        for field in ["id", "updatedAt", "body"] {
            let value = comment
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .as_bytes();
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value);
        }
    }
    format!("{:x}", digest.finalize())
}

fn review_thread_reply_marker(
    thread_id: &str,
    repair_version: &str,
    witness: &ReviewThreadWitness,
    body: &str,
) -> String {
    let mut digest = Sha256::new();
    for value in [
        thread_id.as_bytes(),
        repair_version.as_bytes(),
        witness.reply_generation.as_bytes(),
        body.trim().as_bytes(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    format!(
        "<!-- jig-pr-manager:review-reply:v2:{:x} -->",
        digest.finalize()
    )
}

fn observed_review_thread_ids(pull_request: &Value) -> BTreeSet<String> {
    actionable_review_threads(pull_request)
        .filter_map(|thread| thread.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn actionable_review_threads(pull_request: &Value) -> impl Iterator<Item = &Value> {
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
                && !thread
                    .get("is_resolved")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
}

fn review_thread_matches_witness(
    state: &LiveReviewThreadState,
    witness: &ReviewThreadWitness,
    reply_comment_id: Option<&str>,
) -> bool {
    let added_reply = reply_comment_id.is_some_and(|reply_id| !witness.comment_ids.contains(reply_id));
    let expected_count = witness.comment_count.saturating_add(u64::from(added_reply));
    state.total_count == expected_count
        && review_comment_generation(
            state.comments.iter(),
            added_reply.then_some(reply_comment_id).flatten(),
        ) == witness.resolution_generation
}

fn fetch_review_thread_witness_state(
    thread_id: &str,
    mut fetch: impl FnMut(Option<&str>) -> std::result::Result<Value, ExecutionCommandError>,
) -> std::result::Result<LiveReviewThreadState, ExecutionCommandError> {
    let mut cursor = None;
    let mut pages = Vec::new();
    let mut total_count = None;
    let mut is_resolved = None;
    let mut cursors = BTreeSet::new();
    for _ in 0..REVIEW_THREAD_COMMENT_PAGE_LIMIT {
        let page = validate_review_thread_witness_page(fetch(cursor.as_deref())?, thread_id)?;
        let page_total = page
            .pointer("/data/node/comments/totalCount")
            .and_then(Value::as_u64)
            .unwrap();
        let page_resolved = page
            .pointer("/data/node/isResolved")
            .and_then(Value::as_bool)
            .unwrap();
        if total_count.replace(page_total).is_some_and(|count| count != page_total)
            || is_resolved
                .replace(page_resolved)
                .is_some_and(|resolved| resolved != page_resolved)
        {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread changed while its comment witness was collected for {thread_id}"
            )));
        }
        pages.push(
            page.pointer("/data/node/comments/nodes")
                .and_then(Value::as_array)
                .unwrap()
                .clone(),
        );
        if !review_thread_comments_have_previous_page(&page) {
            pages.reverse();
            let comments = pages.into_iter().flatten().collect::<Vec<_>>();
            let total_count = total_count.unwrap_or_default();
            let ids = comments
                .iter()
                .filter_map(|comment| comment.get("id").and_then(Value::as_str))
                .collect::<BTreeSet<_>>();
            if comments.len() as u64 != total_count || ids.len() != comments.len() {
                return Err(ExecutionCommandError::failed(anyhow!(
                    "GitHub review thread comment witness was incomplete for {thread_id}"
                )));
            }
            return Ok(LiveReviewThreadState {
                is_resolved: is_resolved.unwrap_or(false),
                total_count,
                comments,
            });
        }
        let next = page
            .pointer("/data/node/comments/pageInfo/startCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .ok_or_else(|| {
                ExecutionCommandError::failed(anyhow!(
                    "GitHub review thread comment page reported earlier results without a start cursor for {thread_id}"
                ))
            })?
            .to_string();
        if !cursors.insert(next.clone()) {
            return Err(ExecutionCommandError::failed(anyhow!(
                "GitHub review thread comment pagination repeated a cursor for {thread_id}"
            )));
        }
        cursor = Some(next);
    }
    Err(ExecutionCommandError::failed(anyhow!(
        "GitHub review thread comment history exceeded the {REVIEW_THREAD_COMMENT_PAGE_LIMIT}-page safety limit for {thread_id}"
    )))
}
