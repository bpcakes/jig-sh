fn prepare_worktree(
    ctx: &RepoContext,
    workflow: &ResolvedWorkflow,
    item: &PrWorkItem,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<PathBuf> {
    let worktree = ctx
        .root()
        .join(LOOP_CACHE_DIR)
        .join("worktrees")
        .join(&workflow.id)
        .join(format!(
            "pr-{}-{}",
            item.pr_number,
            sanitize_path_component(&item.head_ref)
        ));
    let parent = worktree
        .parent()
        .ok_or_else(|| anyhow!("Worktree path has no parent: {}", worktree.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;

    git_checked(
        ctx,
        ctx.root(),
        ["fetch", "origin", &item.head_ref],
        observer,
    )?;
    if worktree.join(".git").exists() {
        clean_reused_worktree(ctx, &worktree, observer)?;
        git_checked(
            ctx,
            &worktree,
            ["fetch", "origin", &item.head_ref],
            observer,
        )?;
        git_checked(
            ctx,
            &worktree,
            ["checkout", "--detach", "FETCH_HEAD"],
            observer,
        )?;
    } else {
        git_checked(
            ctx,
            ctx.root(),
            vec![
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--detach"),
                worktree.as_os_str().to_os_string(),
                OsString::from("FETCH_HEAD"),
            ],
            observer,
        )?;
    }

    git_checked(
        ctx,
        &worktree,
        ["config", "user.name", "Jig PR Manager"],
        observer,
    )?;
    git_checked(
        ctx,
        &worktree,
        [
            "config",
            "user.email",
            "jig-pr-manager@users.noreply.github.com",
        ],
        observer,
    )?;
    Ok(worktree)
}
fn clean_reused_worktree(
    ctx: &RepoContext,
    worktree: &Path,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<()> {
    match git_output(ctx, worktree, ["merge", "--abort"], observer) {
        Ok(_) | Err(PrRepairStepError::Failed(_)) => {}
        Err(cancelled @ PrRepairStepError::Cancelled(_)) => return Err(cancelled),
    }
    git_checked(ctx, worktree, ["reset", "--hard"], observer)?;
    git_checked(ctx, worktree, ["clean", "-fd"], observer)?;
    Ok(())
}

fn start_base_merge(
    ctx: &RepoContext,
    worktree: &Path,
    base_ref: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Value> {
    let fetch = git_output(ctx, worktree, ["fetch", "origin", base_ref], observer)?;
    if !fetch.status.success() {
        return Err(PrRepairStepError::failed(git_error(
            "git fetch base branch failed",
            fetch,
        )));
    }
    let merge = git_output(
        ctx,
        worktree,
        ["merge", "--no-edit", "FETCH_HEAD"],
        observer,
    )?;
    Ok(json!({
        "exit_status": merge.status.code().unwrap_or(1),
        "stdout": String::from_utf8_lossy(&merge.stdout),
        "stderr": String::from_utf8_lossy(&merge.stderr),
        "conflicts": !merge.status.success(),
    }))
}

fn commit_and_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    base_head: &str,
    observer: &mut dyn ExecutionControl,
) -> PrRepairStepResult<Value> {
    let dirty_before_commit = git_stdout(ctx, worktree, ["status", "--porcelain"], observer)?;
    if !dirty_before_commit.trim().is_empty() {
        git_checked(ctx, worktree, ["add", "-A"], observer)?;
        git_checked(
            ctx,
            worktree,
            [
                "commit",
                "-m",
                &format!("chore: update PR via Jig PR manager ({head_ref})"),
            ],
            observer,
        )?;
    }
    let final_head = git_stdout(ctx, worktree, ["rev-parse", "HEAD"], observer)?;
    let changed = final_head.trim() != base_head.trim();
    if !changed {
        return Ok(json!({
            "status": "no_changes",
            "pushed": false,
            "base_head": base_head.trim(),
            "final_head": final_head.trim(),
        }));
    }

    let push_ref = format!("HEAD:refs/heads/{head_ref}");
    let push_args = ["push", "origin", &push_ref];
    let push_result = git_execution_output(worktree, push_args, ctx.command_timeout(), observer);
    let push_error = match push_result {
        Ok(push) if push.status.success() => None,
        Ok(push) => Some(PrRepairStepError::failed(git_error(
            "git push failed without force",
            push,
        ))),
        Err(error) => Some(pr_git_execution_error("git push", error)),
    };
    if let Some(push_error) = push_error {
        let reconciliation = reconcile_remote_push(ctx, worktree, head_ref, final_head.trim());
        if reconciliation.confirmed {
            return Ok(push_result_value(
                base_head,
                &final_head,
                Some(reconciliation.detail),
            ));
        }
        return Err(match push_error {
            PrRepairStepError::Cancelled(detail) => PrRepairStepError::Cancelled(format!(
                "{detail}; push outcome was not confirmed: {}",
                reconciliation.detail
            )),
            PrRepairStepError::Failed(error) => PrRepairStepError::Failed(error.context(format!(
                "push outcome was not confirmed: {}",
                reconciliation.detail
            ))),
        });
    }

    Ok(push_result_value(base_head, &final_head, None))
}

fn push_result_value(base_head: &str, final_head: &str, reconciliation: Option<String>) -> Value {
    let mut value = json!({
        "status": "pushed",
        "pushed": true,
        "base_head": base_head.trim(),
        "final_head": final_head.trim(),
        "force": false,
    });
    if let Some(reconciliation) = reconciliation {
        value["reconciliation"] = Value::String(reconciliation);
    }
    value
}

struct PushReconciliation {
    confirmed: bool,
    detail: String,
}

fn reconcile_remote_push(
    ctx: &RepoContext,
    worktree: &Path,
    head_ref: &str,
    final_head: &str,
) -> PushReconciliation {
    let remote_ref = format!("refs/heads/{head_ref}");
    let mut observer = NoopExecutionObserver;
    let timeout_seconds = ctx.command_timeout().as_secs().min(30);
    let timeout = CommandTimeout::from_seconds(timeout_seconds)
        .expect("the reconciliation timeout is nonzero and within the command timeout range");
    let output = git_execution_output(
        worktree,
        ["ls-remote", "--exit-code", "origin", &remote_ref],
        timeout,
        &mut observer,
    );
    match output {
        Ok(output) if output.status.success() => {
            let observed = remote_head_from_ls_remote(&output.stdout, &remote_ref);
            PushReconciliation {
                confirmed: observed == Some(final_head),
                detail: match observed {
                    Some(observed) if observed == final_head => {
                        format!("remote {remote_ref} confirmed at {observed}")
                    }
                    Some(observed) => {
                        format!("remote {remote_ref} resolved to {observed}; expected {final_head}")
                    }
                    None => format!("remote {remote_ref} returned no matching head"),
                },
            }
        }
        Ok(output) => PushReconciliation {
            confirmed: false,
            detail: format!(
                "remote {remote_ref} reconciliation exited with status {}",
                output.status.code().unwrap_or(1)
            ),
        },
        Err(error) => PushReconciliation {
            confirmed: false,
            detail: format!("remote {remote_ref} reconciliation failed: {error}"),
        },
    }
}

fn remote_head_from_ls_remote<'a>(stdout: &'a [u8], remote_ref: &str) -> Option<&'a str> {
    std::str::from_utf8(stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find_map(|(head, reference)| (reference.trim() == remote_ref).then_some(head.trim()))
}

fn pr_worker_prompt(
    ctx: &RepoContext,
    item: &PrWorkItem,
    pull_request: &Value,
    merge: Option<&Value>,
) -> String {
    format!(
        "You are Jig's PR manager worker for repository `{}`.\n\
         Work only on PR #{} (`{}`) on branch `{}`. Reasons: {}.\n\
         Resolve the reported PR issues in this isolated worktree. If merge conflicts are present, resolve them completely. \
         If CI is failing, inspect the failing checks and fix the underlying code. \
         If unresolved review threads are present, address the actionable feedback with code changes when possible. \
         Do not use `gh`, `curl`, or network access to reply to or resolve review threads. \
         Instead, return review-thread reply intents in the required structured output. \
         Include a reply intent only when a concise comment or resolution is needed after your code changes; set `resolve` only when the feedback is fully addressed.\n\
         Run relevant local tests when available. Do not merge the PR. Do not force-push. Keep changes minimal and commit them if you change files. \
         Always write structured output with `summary` and `review_thread_replies`.\n\n\
         Merge preparation result:\n{}\n\n\
         Normalized PR snapshot:\n{}\n",
        ctx.repo_name(),
        item.pr_number,
        item.title,
        item.head_ref,
        item.reasons.join(", "),
        merge
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".into()),
        serde_json::to_string_pretty(pull_request).unwrap_or_else(|_| pull_request.to_string()),
    )
}

fn with_attempt(mut action: Value, attempt: AttemptRecord) -> Value {
    if let Some(object) = action.as_object_mut() {
        object.insert("attempt".into(), json!(attempt));
    }
    action
}

fn with_branch_lease_result(mut action: Value, release_error: Option<&anyhow::Error>) -> Value {
    if let Some(release_error) = release_error {
        action["completed_status"] = action["status"].clone();
        action["status"] = json!("failed");
        action["lease_error"] = json!(format!("{release_error:#}"));
        action["error"] = json!(format!(
            "Branch repair completed, but lease renewal or release failed: {release_error:#}"
        ));
    }
    action
}
