enum PrePushReviewAuthority {
    Current,
    Changed {
        thread_id: String,
        reason: &'static str,
    },
}

fn revalidate_observed_review_threads(
    ctx: &RepoContext,
    pull_request: &Value,
    expected_head: &str,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<PrePushReviewAuthority, ExecutionCommandError> {
    let witnesses = observed_review_thread_witnesses(pull_request);
    let mut budget = ReviewThreadUpdateBudget::new(ctx.command_timeout(), witnesses.len());
    for (thread_id, witness) in witnesses {
        budget.begin_intent(ctx.command_timeout());
        let state = review_thread_resolution_state(ctx, &thread_id, observer, &mut budget)?;
        if let Some(reason) =
            review_thread_mutation_change_reason(&state, &witness, None, expected_head)
        {
            return Ok(PrePushReviewAuthority::Changed { thread_id, reason });
        }
    }
    Ok(PrePushReviewAuthority::Current)
}

fn pre_push_review_authority_outcome<L: serde::Serialize>(
    repair: &PrRepairContext<'_, L>,
    pull_request: &Value,
    worktree: &PreparedPrWorktree,
    worker_output: &Value,
    merge: Option<&Value>,
    worker_receipt_id: &str,
    observer: &mut dyn ExecutionControl,
) -> Option<PrRepairOutcome> {
    let (attention_kind, error, revalidation) =
        match revalidate_observed_review_threads(
            repair.repo,
            pull_request,
            &repair.item.head_sha,
            observer,
        ) {
            Ok(PrePushReviewAuthority::Current) => return None,
            Ok(PrePushReviewAuthority::Changed { thread_id, reason }) => (
                "review_feedback_changed_before_push",
                format!(
                    "Review thread {thread_id} changed after the worker ran; the local repair was retained and was not pushed"
                ),
                json!({"status": "changed", "thread_id": thread_id, "reason": reason}),
            ),
            Err(
                ExecutionCommandError::CancelledBeforeStart | ExecutionCommandError::Cancelled,
            ) => {
                return Some(PrRepairOutcome::WorkerCancelled {
                    before_start: false,
                    worker_receipt_id: worker_receipt_id.to_string(),
                    worktree: worktree.clone(),
                });
            }
            Err(ExecutionCommandError::Failed { error, .. }) => (
                "review_feedback_revalidation_failed",
                format!(
                    "Review feedback could not be revalidated after the worker ran, so the local repair was retained and was not pushed: {error:#}"
                ),
                json!({"status": "failed", "error": format!("{error:#}")}),
            ),
        };
    Some(PrRepairOutcome::NeedsAttention {
        action: json!({
            "kind": "pr_manager_worker",
            "status": "needs_attention",
            "attention_kind": attention_kind,
            "pr_number": repair.item.pr_number,
            "item_key": repair.item.item_key,
            "title": repair.item.title,
            "branch": repair.item.head_ref,
            "head_sha": repair.item.head_sha,
            "reasons": repair.item.reasons,
            "worktree": pr_worktree_value(worktree.path()),
            "lease": repair.lease,
            "codex_home_resolved": repair.codex_home.map(|home| home.display().to_string()),
            "merge": merge,
            "worker_output": worker_output,
            "worker_receipt_id": worker_receipt_id,
            "review_thread_revalidation": revalidation,
            "push": {
                "status": "not_attempted",
                "pushed": false,
                "expected_remote_head": repair.item.head_sha,
            },
            "review_thread_posts": [],
            "error": error,
        }),
        worktree: worktree.path().to_path_buf(),
    })
}
