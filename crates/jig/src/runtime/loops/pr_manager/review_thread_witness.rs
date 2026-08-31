#[derive(Clone, Default)]
struct ReviewThreadWitness {
    comment_count: u64,
    latest_comment_id: Option<String>,
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
            let latest_comment_id = thread
                .pointer("/comments/nodes")
                .and_then(Value::as_array)
                .and_then(|comments| comments.last())
                .and_then(|comment| comment.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Some((
                id,
                ReviewThreadWitness {
                    comment_count,
                    latest_comment_id,
                },
            ))
        })
        .collect()
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
    state: &Value,
    witness: &ReviewThreadWitness,
    reply_comment_id: Option<&str>,
) -> bool {
    let added_reply = reply_comment_id
        .is_some_and(|reply_id| witness.latest_comment_id.as_deref() != Some(reply_id));
    let expected_count = witness.comment_count.saturating_add(u64::from(added_reply));
    let expected_latest = reply_comment_id.or(witness.latest_comment_id.as_deref());
    state
        .pointer("/data/node/comments/totalCount")
        .and_then(Value::as_u64)
        == Some(expected_count)
        && state
            .pointer("/data/node/comments/nodes")
            .and_then(Value::as_array)
            .and_then(|comments| comments.last())
            .and_then(|comment| comment.get("id"))
            .and_then(Value::as_str)
            == expected_latest
}
