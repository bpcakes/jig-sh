pub(super) fn pr_manager_tick(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    observer: &mut dyn ExecutionControl,
) -> Result<WorkflowTick> {
    let codex_home = match workflow
        .codex_home_configured
        .as_deref()
        .map(|home| crate::codex::resolve_configured_home_from_dir(home, ctx.root()))
        .transpose()
    {
        Ok(home) => home,
        Err(error) => {
            return Ok(pr_manager_unexecuted_tick(
                Value::Null,
                Vec::new(),
                error,
                UnexecutedReason::PreExecutionError,
            ));
        }
    };
    let observed = match github::github_pr_status_snapshot(ctx, observer) {
        Ok(observed) => observed,
        Err(error) => {
            let reason = if observer.cancelled() {
                UnexecutedReason::CancelledBeforeStart
            } else {
                UnexecutedReason::PreExecutionError
            };
            return Ok(pr_manager_unexecuted_tick(
                Value::Null,
                Vec::new(),
                error,
                reason,
            ));
        }
    };
    pr_manager_tick_from_snapshot(
        ctx,
        workflow,
        lease_store,
        attempt_store,
        observed,
        PrManagerExecution {
            codex_home: codex_home.as_deref(),
            observer,
        },
    )
}

fn pr_manager_tick_from_snapshot(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    lease_store: &mut LeaseStore,
    attempt_store: &mut AttemptStore,
    observed: Value,
    execution: PrManagerExecution<'_>,
) -> Result<WorkflowTick> {
    let Some(pull_requests) = observed.get("pull_requests").and_then(Value::as_array) else {
        return Ok(pr_manager_unexecuted_tick(
            observed,
            Vec::new(),
            anyhow!("GitHub PR snapshot did not include pull_requests array"),
            UnexecutedReason::PreExecutionError,
        ));
    };
    if let Some(action) = incomplete_snapshot_action(&observed, pull_requests) {
        let actions = vec![action];
        let completion = pr_manager_completion(&actions);
        return Ok(WorkflowTick::with_completion(
            observed, actions, completion,
        ));
    }
    let default_branch = observed
        .pointer("/repository/default_branch")
        .and_then(Value::as_str)
        .unwrap_or_else(|| ctx.default_branch());

    let mut actions = Vec::new();
    for pull_request in pull_requests {
        if execution.observer.cancelled() {
            return Ok(pr_manager_unexecuted_tick(
                observed,
                actions,
                anyhow!("PR manager tick was cancelled before its worker started"),
                UnexecutedReason::CancelledBeforeStart,
            ));
        }
        let candidate = classify_pull_request(pull_request, default_branch);
        match candidate {
            PrCandidate::Skip(action) => actions.push(action),
            PrCandidate::Idle(item) => {
                match clear_observed_healthy_attempt(workflow, attempt_store, &item) {
                    Ok(Some(action)) => actions.push(action),
                    Ok(None) => {}
                    Err(error) => {
                        return Ok(pr_manager_unexecuted_tick(
                            observed,
                            actions,
                            error,
                            UnexecutedReason::PreExecutionError,
                        ));
                    }
                }
            }
            PrCandidate::Pending(item) => actions.push(pending_checks_action(&item)),
            PrCandidate::Actionable(item) => {
                let action = match handle_actionable_pr(
                    ctx,
                    workflow,
                    lease_store,
                    attempt_store,
                    &item,
                    pull_request,
                    PrManagerExecution {
                        codex_home: execution.codex_home,
                        observer: &mut *execution.observer,
                    },
                ) {
                    Ok(action) => action,
                    Err(error) => {
                        let reason = if execution.observer.cancelled() {
                            UnexecutedReason::CancelledBeforeStart
                        } else {
                            UnexecutedReason::PreExecutionError
                        };
                        return Ok(pr_manager_unexecuted_tick(observed, actions, error, reason));
                    }
                };
                let consumed_tick = pr_manager_action_consumed_tick(&action);
                actions.push(action);
                if consumed_tick {
                    break;
                }
            }
        }
    }

    let completion = pr_manager_completion(&actions);
    Ok(WorkflowTick::with_completion(observed, actions, completion))
}

fn pr_manager_completion(actions: &[Value]) -> WorkflowCompletion {
    let mut completion = WorkflowCompletion::from_actions(actions);
    if let Some(reason) = actions.iter().find_map(|action| {
        match action.get("unexecuted_reason").and_then(Value::as_str) {
            Some("cancelled_before_start") => Some(UnexecutedReason::CancelledBeforeStart),
            Some("pre_execution_error") => Some(UnexecutedReason::PreExecutionError),
            _ => None,
        }
    }) {
        completion.execution = WorkflowExecution::Unexecuted(reason);
    }
    completion
}

fn pr_manager_unexecuted_tick(
    observed: Value,
    mut actions: Vec<Value>,
    error: anyhow::Error,
    reason: UnexecutedReason,
) -> WorkflowTick {
    actions.push(json!({
        "kind": "pr_manager_pre_execution",
        "status": "failed",
        "unexecuted_reason": reason.as_str(),
        "error": format!("{error:#}"),
    }));
    let completion = pr_manager_completion(&actions);
    WorkflowTick::with_completion(observed, actions, completion)
}

fn incomplete_snapshot_action(observed: &Value, pull_requests: &[Value]) -> Option<Value> {
    let pr_list_truncated = observed
        .pointer("/summary/pr_list_truncated")
        .and_then(Value::as_bool)
        == Some(true);
    let review_threads_truncated = pull_requests.iter().any(|pull_request| {
        pull_request
            .pointer("/review_threads/page_info/truncated")
            .and_then(Value::as_bool)
            == Some(true)
    });
    let truncated_review_threads = pull_requests
        .iter()
        .filter(|pull_request| {
            pull_request
                .pointer("/review_threads/page_info/truncated")
                .and_then(Value::as_bool)
                == Some(true)
        })
        .filter_map(|pull_request| pull_request.get("number").and_then(Value::as_u64))
        .collect::<Vec<_>>();
    if !pr_list_truncated && !review_threads_truncated {
        return None;
    }
    Some(json!({
        "kind": "pr_manager_observation",
        "status": "failed",
        "reason": "incomplete_github_snapshot",
        "unexecuted_reason": UnexecutedReason::PreExecutionError.as_str(),
        "pr_list_truncated": pr_list_truncated,
        "review_thread_prs_truncated": truncated_review_threads,
        "error": "PR manager refused to mutate attempts or branches from an incomplete GitHub snapshot",
    }))
}
