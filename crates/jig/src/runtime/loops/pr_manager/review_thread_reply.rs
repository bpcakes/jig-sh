enum ReviewThreadReply {
    Posted(Value),
    Changed,
}

fn post_review_thread_reply(
    ctx: &RepoContext,
    thread_id: &str,
    body: &str,
    repair_version: &str,
    witness: &ReviewThreadWitness,
    observer: &mut dyn ExecutionControl,
    budget: &mut ReviewThreadUpdateBudget,
) -> std::result::Result<ReviewThreadReply, ExecutionCommandError> {
    let marker = review_thread_reply_marker(thread_id, repair_version, witness, body);
    if let Some(comment) =
        review_thread_reply_comment(ctx, thread_id, &marker, observer, budget)?
    {
        return Ok(ReviewThreadReply::Posted(reconciled_reply_response(
            &comment,
        )));
    }

    // The worker reasoned from a complete snapshot. Re-read the same witness
    // immediately before mutation so edited, added, or resolved feedback never
    // receives a response derived from stale input.
    let state = review_thread_resolution_state(ctx, thread_id, observer, budget)?;
    if state.is_resolved || !review_thread_matches_witness(&state, witness, None) {
        return Ok(ReviewThreadReply::Changed);
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
    let timeout = budget.reserve_request(ctx.command_timeout().duration())?;
    let result = github::gh_json_with_duration(
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
        timeout,
        observer,
    )
    .and_then(validate_reply_mutation_response);
    reconcile_reply_mutation(ctx, thread_id, &marker, result, budget)
        .map(ReviewThreadReply::Posted)
}
