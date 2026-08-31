#[derive(Clone, Default)]
struct ReviewThreadWitness {
    comment_count: u64,
    latest_comment_id: Option<String>,
    reply_generation: String,
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
            let reply_generation = review_reply_generation(thread);
            Some((
                id,
                ReviewThreadWitness {
                    comment_count,
                    latest_comment_id,
                    reply_generation,
                },
            ))
        })
        .collect()
}

fn review_reply_generation(thread: &Value) -> String {
    let mut digest = Sha256::new();
    for comment in thread
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
        })
    {
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
