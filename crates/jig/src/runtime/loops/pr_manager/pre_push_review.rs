enum PrePushReviewAuthority {
    Current,
    Changed {
        thread_id: Option<String>,
        reason: &'static str,
    },
}

fn revalidate_observed_review_threads(
    ctx: &RepoContext,
    pull_request: &Value,
    worktree: &Path,
    head_ref: &str,
    expected_head: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PrePushReviewAuthority> {
    let remote_ref = remote_branch_ref(head_ref);
    let remote_head_before = remote_head_for_ref(ctx, worktree, &remote_ref, observer)?;
    if remote_head_before != expected_head {
        return Ok(PrePushReviewAuthority::Changed {
            thread_id: None,
            reason: "pr_head_changed",
        });
    }
    let current = github::github_pr_review_threads_snapshot(
        ctx,
        pull_request
            .get("number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                PrRepairStepError::failed(anyhow!(
                    "GitHub pull request snapshot did not include its number"
                ))
            })?,
        observer,
    )
    .map_err(PrRepairStepError::failed)?;
    let remote_head_after = remote_head_for_ref(ctx, worktree, &remote_ref, observer)?;
    if remote_head_after != expected_head || remote_head_after != remote_head_before {
        return Ok(PrePushReviewAuthority::Changed {
            thread_id: None,
            reason: "pr_head_changed",
        });
    }
    if current
        .pointer("/review_threads/page_info/truncated")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        return Err(PrRepairStepError::failed(anyhow!(
            "GitHub review feedback revalidation was incomplete"
        )));
    }

    let observed = observed_review_thread_witnesses(pull_request);
    let current = observed_review_thread_witnesses(&current);
    if observed.keys().ne(current.keys()) {
        let thread_id = observed
            .keys()
            .chain(current.keys())
            .find(|thread_id| !observed.contains_key(*thread_id) || !current.contains_key(*thread_id))
            .cloned()
            .unwrap_or_default();
        return Ok(PrePushReviewAuthority::Changed {
            thread_id: Some(thread_id),
            reason: "review_thread_membership_changed",
        });
    }
    for (thread_id, witness) in &observed {
        if !witness.same_feedback(&current[thread_id]) {
            return Ok(PrePushReviewAuthority::Changed {
                thread_id: Some(thread_id.clone()),
                reason: "review_thread_changed",
            });
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
            worktree.path(),
            &repair.item.head_ref,
            &repair.item.head_sha,
            observer,
        ) {
            Ok(PrePushReviewAuthority::Current) => return None,
            Ok(PrePushReviewAuthority::Changed { thread_id, reason }) => {
                let error = thread_id.as_deref().map_or_else(
                    || {
                        "The pull request head changed after the worker ran; the local repair was retained and was not pushed".to_string()
                    },
                    |thread_id| {
                        format!(
                            "Review thread {thread_id} changed after the worker ran; the local repair was retained and was not pushed"
                        )
                    },
                );
                (
                    if reason == "pr_head_changed" {
                        "pr_head_changed_before_push"
                    } else {
                        "review_feedback_changed_before_push"
                    },
                    error,
                    json!({"status": "changed", "thread_id": thread_id, "reason": reason}),
                )
            }
            Err(_) if observer.cancelled() => {
                return Some(PrRepairOutcome::WorkerCancelled {
                    before_start: false,
                    worker_receipt_id: worker_receipt_id.to_string(),
                    worktree: worktree.clone(),
                });
            }
            Err(PrRepairStepError::Cancelled(_)) => {
                return Some(PrRepairOutcome::WorkerCancelled {
                    before_start: false,
                    worker_receipt_id: worker_receipt_id.to_string(),
                    worktree: worktree.clone(),
                });
            }
            Err(PrRepairStepError::Failed(error)) => (
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
